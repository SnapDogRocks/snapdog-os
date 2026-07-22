// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

use crate::error::{Result, UpgradeError};
use futures_util::StreamExt;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use reqwest::{Client, RequestBuilder, Response, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};
use std::path::Path;
use tokio::fs::File;
use tokio_util::io::ReaderStream;

pub struct UpdateClient {
    client: Client,
    base_url: String,
    token: Option<String>,
}

#[derive(Deserialize)]
struct AuthStatus {
    enabled: bool,
    authenticated: bool,
}

#[derive(Serialize)]
struct LoginRequest<'a> {
    password: &'a str,
}

#[derive(Deserialize)]
struct LoginResponse {
    token: String,
}

#[derive(Deserialize, Clone)]
pub struct SystemInfo {
    pub hostname: String,
    pub version: String,
    pub board_model: String,
    pub uptime_seconds: u64,
}

#[derive(Deserialize)]
pub struct HealthResponse {
    pub ok: bool,
    pub warnings: Vec<HealthWarning>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct HealthWarning {
    pub id: String,
    pub severity: String,
}

#[derive(Deserialize, Clone)]
pub struct RaucProgress {
    // The device serializes this as `percentage` (an integer); the WebUI reads the
    // same field. Keep the wire name in sync with snapdog-ctrl's InstallProgress.
    pub percentage: i32,
    pub message: String,
}

/// Best-effort deserialize of the display-only `progress` field.
///
/// `progress` is populated ONLY while `operation == "installing"`. It is purely
/// cosmetic ("Installing: 40%"), so a shape mismatch must never fail the whole
/// `UpdateStatus` decode — otherwise every poll during the install window errors,
/// `saw_installing` is never set, and the client waits forever for an install
/// that already finished. This tolerates any shape (or null) by degrading to
/// `None` instead of erroring. (Historically a `percent` vs `percentage` field
/// mismatch caused exactly that hang.)
fn lenient_progress<'de, D>(deserializer: D) -> std::result::Result<Option<RaucProgress>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value.and_then(|v| serde_json::from_value(v).ok()))
}

#[derive(Deserialize, Clone)]
pub struct SlotStatus {
    pub name: String,
    #[serde(default)]
    pub class: String,
    pub device: String,
    pub state: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub booted: bool,
}

#[derive(Deserialize, Clone)]
pub struct UpdateStatus {
    pub operation: String,
    // Cosmetic + fragile (nested object, only present mid-install) — decode it
    // leniently so it can never block reading `operation`, which drives the
    // install state machine. See lenient_progress.
    #[serde(default, deserialize_with = "lenient_progress")]
    pub progress: Option<RaucProgress>,
    pub last_error: String,
    pub slots: Vec<SlotStatus>,
}

impl UpdateClient {
    pub fn new(base_url: &str) -> Result<Self> {
        let base_url = base_url.trim_end_matches('/').to_string();
        let parsed = reqwest::Url::parse(&base_url)
            .map_err(|_| UpgradeError::InvalidBaseUrl(base_url.clone()))?;
        if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
            return Err(UpgradeError::InvalidBaseUrl(base_url));
        }

        Ok(Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()?,
            base_url,
            token: None,
        })
    }

    fn headers(&self) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        if let Some(ref token) = self.token {
            let val = HeaderValue::from_str(&format!("Bearer {token}"))
                .map_err(|_| UpgradeError::InvalidAuthToken)?;
            headers.insert(AUTHORIZATION, val);
        }
        Ok(headers)
    }

    pub async fn preflight_auth(
        &mut self,
        password: Option<&str>,
        interactive: bool,
    ) -> Result<()> {
        let url = format!("{}/api/auth/status", self.base_url);
        let res: AuthStatus = self
            .send_json(self.client.get(&url), "check authentication status")
            .await?;

        if res.enabled && !res.authenticated {
            let pwd = if let Some(p) = password {
                p.to_string()
            } else if !interactive {
                return Err(UpgradeError::NonInteractiveInputRequired {
                    input: "password",
                    hint: "Pass --password or set SNAPDOG_PASSWORD.",
                });
            } else {
                let prompt_task = tokio::task::spawn_blocking(|| {
                    rpassword::prompt_password("Enter password for SnapDog control panel: ")
                });
                prompt_task
                    .await
                    .map_err(|_| UpgradeError::Unauthorized)?
                    .map_err(|_| UpgradeError::Unauthorized)?
            };
            let login_url = format!("{}/api/auth/login", self.base_url);
            let response = self
                .client
                .post(&login_url)
                .json(&LoginRequest { password: &pwd })
                .send()
                .await?;

            if response.status() == StatusCode::UNAUTHORIZED {
                return Err(UpgradeError::Unauthorized);
            }
            let login: LoginResponse = Self::check_response(response, "login")
                .await?
                .json()
                .await?;
            self.token = Some(login.token);
        }
        Ok(())
    }

    pub async fn system_info(&self) -> Result<SystemInfo> {
        let url = format!("{}/api/system", self.base_url);
        let info = self
            .send_json(
                self.client.get(&url).headers(self.headers()?),
                "fetch system information",
            )
            .await?;
        Ok(info)
    }

    pub async fn system_health(&self) -> Result<HealthResponse> {
        let url = format!("{}/api/system/health", self.base_url);
        let health = self
            .send_json(
                self.client.get(&url).headers(self.headers()?),
                "fetch system health",
            )
            .await?;
        Ok(health)
    }

    pub async fn upload_bundle<F>(
        &self,
        file_path: &Path,
        progress_cb: F,
    ) -> Result<reqwest::Response>
    where
        F: Fn(u64) + Send + Sync + 'static,
    {
        let file = File::open(file_path).await?;
        let metadata = file.metadata().await?;
        let total_size = metadata.len();

        let file_stream = ReaderStream::new(file);
        let mut bytes_sent = 0;

        let tracking_stream = file_stream.map(move |chunk| {
            if let Ok(ref c) = chunk {
                bytes_sent += c.len() as u64;
                progress_cb(bytes_sent);
            }
            chunk
        });

        let body = reqwest::Body::wrap_stream(tracking_stream);
        let file_name = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("firmware.raucb")
            .to_string();

        let part = reqwest::multipart::Part::stream_with_length(body, total_size)
            .file_name(file_name)
            .mime_str("application/octet-stream")?;

        let form = reqwest::multipart::Form::new().part("file", part);
        let url = format!("{}/api/system/update/upload", self.base_url);

        let upload_client = Client::builder()
            .timeout(std::time::Duration::from_secs(900))
            .build()?;

        let res = upload_client
            .post(&url)
            .headers(self.headers()?)
            .multipart(form)
            .send()
            .await?;

        Self::check_response(res, "upload RAUC bundle").await
    }

    pub async fn trigger_install(&self) -> Result<()> {
        let url = format!("{}/api/system/update/install", self.base_url);
        self.send_empty(
            self.client.post(&url).headers(self.headers()?),
            "trigger installation",
        )
        .await?;
        Ok(())
    }

    /// Reboot the device (into the freshly-installed slot after an install).
    pub async fn reboot(&self) -> Result<()> {
        let url = format!("{}/api/system/reboot", self.base_url);
        self.send_empty(
            self.client.post(&url).headers(self.headers()?),
            "trigger reboot",
        )
        .await?;
        Ok(())
    }

    pub async fn update_status(&self) -> Result<UpdateStatus> {
        let url = format!("{}/api/system/update/status", self.base_url);
        let status = self
            .send_json(
                self.client.get(&url).headers(self.headers()?),
                "fetch update status",
            )
            .await?;
        Ok(status)
    }

    async fn send_json<T>(&self, request: RequestBuilder, action: &'static str) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let response = request.send().await?;
        let response = Self::check_response(response, action).await?;
        Ok(response.json().await?)
    }

    async fn send_empty(&self, request: RequestBuilder, action: &'static str) -> Result<()> {
        let response = request.send().await?;
        Self::check_response(response, action).await?;
        Ok(())
    }

    async fn check_response(response: Response, action: &'static str) -> Result<Response> {
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }

        let body = response.text().await.unwrap_or_default();
        Err(UpgradeError::HttpStatus {
            action,
            status,
            body: truncate_body(body),
        })
    }
}

fn truncate_body(body: String) -> String {
    const MAX_BODY_CHARS: usize = 4096;
    if body.chars().count() <= MAX_BODY_CHARS {
        return body;
    }

    let mut truncated: String = body.chars().take(MAX_BODY_CHARS).collect();
    truncated.push_str("...");
    truncated
}
