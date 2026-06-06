// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

use crate::error::{Result, UpgradeError};
use futures_util::StreamExt;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
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
    pub percent: f64,
    pub message: String,
}

#[derive(Deserialize, Clone)]
pub struct SlotStatus {
    pub name: String,
    pub device: String,
    pub state: String,
}

#[derive(Deserialize, Clone)]
pub struct UpdateStatus {
    pub operation: String,
    pub progress: Option<RaucProgress>,
    pub last_error: String,
    pub slots: Vec<SlotStatus>,
}

#[derive(Deserialize, Clone)]
pub struct FlashChallenge {
    pub challenge: String,
    pub expires_in_seconds: u64,
}

#[derive(Serialize)]
struct FlashConfirm {
    challenge: String,
}

impl UpdateClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap(),
            base_url: base_url.trim_end_matches('/').to_string(),
            token: None,
        }
    }

    fn headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Some(ref token) = self.token {
            if let Ok(val) = HeaderValue::from_str(&format!("Bearer {token}")) {
                headers.insert(AUTHORIZATION, val);
            }
        }
        headers
    }

    pub async fn preflight_auth(&mut self, password: Option<&str>) -> Result<()> {
        let url = format!("{}/api/auth/status", self.base_url);
        let res: AuthStatus = self.client.get(&url).send().await?.json().await?;

        if res.enabled && !res.authenticated {
            let pwd = if let Some(p) = password {
                p.to_string()
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
            let login: LoginResponse = response.error_for_status()?.json().await?;
            self.token = Some(login.token);
        }
        Ok(())
    }

    pub async fn system_info(&self) -> Result<SystemInfo> {
        let url = format!("{}/api/system", self.base_url);
        let info: SystemInfo = self
            .client
            .get(&url)
            .headers(self.headers())
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(info)
    }

    pub async fn system_health(&self) -> Result<HealthResponse> {
        let url = format!("{}/api/system/health", self.base_url);
        let health: HealthResponse = self
            .client
            .get(&url)
            .headers(self.headers())
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(health)
    }

    pub async fn upload_image<F>(
        &self,
        file_path: &Path,
        endpoint: &str,
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
            .unwrap_or("image.bin")
            .to_string();

        let part = reqwest::multipart::Part::stream_with_length(body, total_size)
            .file_name(file_name)
            .mime_str("application/octet-stream")
            .unwrap();

        let form = reqwest::multipart::Form::new().part("file", part);
        let url = format!("{}{}", self.base_url, endpoint);

        let upload_client = Client::builder()
            .timeout(std::time::Duration::from_secs(900))
            .build()
            .unwrap();

        let res = upload_client
            .post(&url)
            .headers(self.headers())
            .multipart(form)
            .send()
            .await?;

        if !res.status().is_success() {
            return Err(UpgradeError::UploadFailed(res.status()));
        }
        Ok(res)
    }

    pub async fn trigger_install(&self) -> Result<()> {
        let url = format!("{}/api/system/update/install", self.base_url);
        self.client
            .post(&url)
            .headers(self.headers())
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    pub async fn update_status(&self) -> Result<UpdateStatus> {
        let url = format!("{}/api/system/update/status", self.base_url);
        let status: UpdateStatus = self
            .client
            .get(&url)
            .headers(self.headers())
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(status)
    }

    pub async fn trigger_flash_raw(
        &self,
        file_path: &Path,
        progress_cb: impl Fn(u64) + Send + Sync + 'static,
    ) -> Result<FlashChallenge> {
        let res = self
            .upload_image(file_path, "/api/system/update/flash-raw", progress_cb)
            .await?;
        let challenge: FlashChallenge = res.json().await?;
        Ok(challenge)
    }

    pub async fn confirm_flash_raw(&self, challenge: &str) -> Result<()> {
        let url = format!("{}/api/system/update/flash-raw/confirm", self.base_url);
        self.client
            .post(&url)
            .headers(self.headers())
            .json(&FlashConfirm {
                challenge: challenge.to_string(),
            })
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }
}
