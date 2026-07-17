// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

use axum::{
    Extension, Json, Router,
    extract::{DefaultBodyLimit, Query, Request},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use rust_embed::Embed;
use serde::{Deserialize, Serialize};

use crate::server_config::{self, ServerConfig};
use crate::system;
use rand::distr::SampleString;

// --- Static files ---

#[derive(Embed)]
#[folder = "webui/out/"]
pub struct Assets;

pub async fn static_files(req: Request) -> Response {
    let path = req.uri().path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    match Assets::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            (
                [(axum::http::header::CONTENT_TYPE, mime.as_ref())],
                content.data,
            )
                .into_response()
        }
        None => {
            // SPA fallback
            match Assets::get("index.html") {
                Some(content) => (
                    [(axum::http::header::CONTENT_TYPE, "text/html")],
                    content.data,
                )
                    .into_response(),
                None => StatusCode::NOT_FOUND.into_response(),
            }
        }
    }
}

// --- API router ---

pub fn api() -> Router {
    Router::new()
        .route("/ws", get(crate::ws::ws_handler))
        // Auth
        .route("/auth/status", get(get_auth_status))
        .route("/auth/login", post(post_auth_login))
        .route("/auth/logout", post(post_auth_logout))
        .route("/auth/password", put(put_auth_password))
        // System
        .route("/system", get(get_system).put(put_system))
        .route("/system/tuning", get(get_tuning).put(put_tuning))
        .route("/system/health", get(get_health))
        .route("/system/reboot", post(post_reboot))
        .route("/system/update", post(post_update))
        .route("/system/update/check", get(get_update_check))
        .route("/system/update/status", get(get_update_status))
        // Firmware/image uploads stream a whole rootfs (tens of MB to ~GB) to
        // disk — lift axum's 2 MB DefaultBodyLimit or the body is silently
        // truncated (rauc then rejects it as "Signature size exceeds bundle size").
        .route(
            "/system/update/upload",
            post(post_update_upload).layer(DefaultBodyLimit::disable()),
        )
        .route("/system/update/install", post(post_update_install))
        .route(
            "/system/update/flash-raw",
            post(post_flash_raw_upload).layer(DefaultBodyLimit::disable()),
        )
        .route(
            "/system/update/flash-raw/confirm",
            post(post_flash_raw_confirm),
        )
        .route(
            "/system/update/auto",
            get(get_auto_update).put(put_auto_update),
        )
        .route("/system/factory-reset", post(post_factory_reset))
        .route("/system/logs", get(get_logs))
        .route("/system/timezone", get(get_timezone).put(put_timezone))
        // Network
        .route("/network", get(get_network))
        .route("/network/ethernet", get(get_ethernet).put(put_ethernet))
        .route(
            "/network/wifi",
            get(get_wifi).put(put_wifi).delete(delete_wifi),
        )
        .route("/network/wifi/scan", post(post_wifi_scan))
        .route("/network/softap", get(get_softap).put(put_softap))
        // Audio
        .route("/audio", get(get_audio).put(put_audio))
        // Client
        .route("/client", get(get_client).put(put_client))
        .route("/client/scan-servers", post(post_scan_servers))
        .route("/client/test-server", post(post_test_server))
        // SSH
        .route("/ssh", get(get_ssh).put(put_ssh))
        // Server
        .route("/server", get(get_server).put(put_server))
        .route("/server/status", get(get_server_status))
        .route("/server/enable", post(post_server_enable))
        .route("/server/disable", post(post_server_disable))
        // Settings export/import
        .route("/settings/export", get(get_settings_export))
        .route("/settings/preview", post(post_settings_preview))
        .route("/settings/import", post(post_settings_import))
        // Now Playing
        .route("/now-playing", get(get_now_playing))
        .route("/now-playing/command", post(post_now_playing_command))
        .route("/now-playing/volume", put(put_now_playing_volume))
        .route("/now-playing/seek", post(post_now_playing_seek))
        // 404 for unknown API routes
        .fallback(api_not_found)
}

async fn api_not_found() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({"error": "not found"})),
    )
}

// --- Auth handlers ---

#[derive(Serialize)]
struct AuthStatusResponse {
    enabled: bool,
    authenticated: bool,
}

async fn get_auth_status(
    Extension(auth): Extension<crate::auth::AuthState>,
    req: Request,
) -> Json<AuthStatusResponse> {
    let authenticated = if auth.is_enabled().await {
        let token = req
            .headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .unwrap_or("");
        if token.is_empty() {
            false
        } else {
            auth.is_valid_token(token).await
        }
    } else {
        true
    };
    Json(AuthStatusResponse {
        enabled: auth.is_enabled().await,
        authenticated,
    })
}

#[derive(Deserialize)]
struct LoginRequest {
    password: String,
}

#[derive(Serialize)]
struct LoginResponse {
    token: String,
}

async fn post_auth_login(
    Extension(auth): Extension<crate::auth::AuthState>,
    Json(body): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, Response> {
    if !auth.is_enabled().await {
        return Err(StatusCode::BAD_REQUEST.into_response());
    }
    // Reject outright while locked out, without even checking the password —
    // keeps a hammering client from probing during its own timeout.
    if let Some(retry_after) = auth.lockout_remaining().await {
        return Err(too_many_login_attempts(retry_after));
    }
    if auth.verify_password(&body.password).await {
        auth.record_successful_login().await;
        let token = auth.create_token().await;
        Ok(Json(LoginResponse { token }))
    } else {
        auth.record_failed_login().await;
        Err(StatusCode::UNAUTHORIZED.into_response())
    }
}

/// 429 with a `Retry-After` header the client can turn into a countdown.
fn too_many_login_attempts(retry_after_secs: u64) -> Response {
    let mut res = (
        StatusCode::TOO_MANY_REQUESTS,
        Json(serde_json::json!({ "error": "too_many_attempts", "retry_after": retry_after_secs })),
    )
        .into_response();
    res.headers_mut().insert(
        header::RETRY_AFTER,
        HeaderValue::from_str(&retry_after_secs.to_string())
            .unwrap_or_else(|_| HeaderValue::from_static("60")),
    );
    res
}

async fn post_auth_logout(
    Extension(auth): Extension<crate::auth::AuthState>,
    req: Request,
) -> StatusCode {
    if let Some(token) = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    {
        auth.revoke_token(token).await;
    }
    StatusCode::NO_CONTENT
}

#[derive(Deserialize)]
struct SetPasswordRequest {
    /// Current password (required when changing, not when setting for first time).
    current: Option<String>,
    /// New password. Empty string or null disables auth.
    new: Option<String>,
}

async fn put_auth_password(
    Extension(auth): Extension<crate::auth::AuthState>,
    Json(body): Json<SetPasswordRequest>,
) -> Result<StatusCode, StatusCode> {
    // If auth is already enabled, require current password
    if auth.is_enabled().await {
        let current = body.current.as_deref().unwrap_or("");
        if !auth.verify_password(current).await {
            return Err(StatusCode::UNAUTHORIZED);
        }
    }

    match body.new.as_deref() {
        Some(pw) if !pw.is_empty() => {
            auth.set_password(pw).await.map_err(|e| {
                tracing::error!("failed to set password: {e}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
            // Sync to system root password (console login)
            set_system_password(pw).await;
            // Revoke all existing tokens (force re-login)
            auth.revoke_all().await;
        }
        _ => {
            auth.remove_password().await.map_err(|e| {
                tracing::error!("failed to remove password: {e}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
            // Reset system password to default
            set_system_password("snapdog").await;
        }
    }

    Ok(StatusCode::NO_CONTENT)
}

async fn set_system_password(password: &str) {
    use tokio::io::AsyncWriteExt;
    use tokio::process::Command;
    let input = format!("root:{password}\n");
    let child = Command::new("chpasswd")
        .kill_on_drop(true)
        .stdin(std::process::Stdio::piped())
        .spawn();
    match child {
        Ok(mut c) => {
            if let Some(ref mut stdin) = c.stdin {
                let _ = stdin.write_all(input.as_bytes()).await;
            }
            let _ = c.wait().await;
        }
        Err(e) => tracing::warn!("failed to set system password: {e}"),
    }
}

/// Captive portal detection routes — redirect all OS probes to the setup UI.
pub fn captive_portal_routes() -> Router {
    async fn redirect_to_setup() -> Response {
        axum::response::Redirect::temporary("/").into_response()
    }

    async fn android_204() -> Response {
        // Return non-204 to trigger captive portal
        axum::response::Redirect::temporary("/").into_response()
    }

    Router::new()
        .route("/hotspot-detect.html", get(redirect_to_setup)) // Apple
        .route("/library/test/success.html", get(redirect_to_setup)) // Apple alt
        .route("/generate_204", get(android_204)) // Android
        .route("/gen_204", get(android_204)) // Android alt
        .route("/connecttest.txt", get(redirect_to_setup)) // Windows
        .route("/redirect", get(redirect_to_setup)) // Windows alt
        .route("/ncsi.txt", get(redirect_to_setup)) // Windows NCSI
}

#[cfg(debug_assertions)]
mod mock_handlers {
    use axum::{
        Extension, Json,
        extract::{Multipart, Query, State},
        http::StatusCode,
    };

    use super::{
        AudioConfig, AudioInfo, AutoUpdateConfig, ClientConfig, EthernetConfig, EthernetInfo,
        LogsResponse, NetworkOverview, SshConfig, SystemInfo, SystemUpdate, TimezoneInfo,
        TimezoneUpdate, UpdateCheckResponse, UpdateStatus, WifiConfig, WifiInfo, WifiScanResult,
    };

    pub async fn get_system(State(m): State<crate::mock::MockState>) -> Json<SystemInfo> {
        Json(m.get_system_info().await)
    }
    pub async fn put_system(
        State(m): State<crate::mock::MockState>,
        Json(b): Json<SystemUpdate>,
    ) -> StatusCode {
        m.set_system(b.hostname, b.channel)
            .await
            .map_or(StatusCode::INTERNAL_SERVER_ERROR, |()| StatusCode::OK)
    }
    pub async fn get_tuning(
        State(m): State<crate::mock::MockState>,
    ) -> Json<crate::tuning::TuningConfig> {
        Json(m.get_tuning().await)
    }
    pub async fn put_tuning(
        State(m): State<crate::mock::MockState>,
        Extension(crate::ws::WsSender(tx)): Extension<crate::ws::WsSender>,
        Json(body): Json<crate::tuning::TuningConfig>,
    ) -> StatusCode {
        m.set_tuning(body).await;
        let _ = tx.send("system_changed".to_string());
        StatusCode::OK
    }
    pub async fn reboot(State(m): State<crate::mock::MockState>) -> StatusCode {
        m.reboot().await;
        StatusCode::ACCEPTED
    }
    pub async fn update(State(m): State<crate::mock::MockState>) -> StatusCode {
        m.trigger_update()
            .await
            .map_or(StatusCode::INTERNAL_SERVER_ERROR, |()| StatusCode::ACCEPTED)
    }
    pub async fn get_update_check() -> Json<UpdateCheckResponse> {
        Json(UpdateCheckResponse {
            available: true,
            installable: true,
            current_version: env!("CARGO_PKG_VERSION").into(),
            latest_version: "9.9.9".into(),
            channel: "release".into(),
            is_downgrade: false,
            signature_verified: true,
            bundle_url: "https://update.snapdog.cc/os/bundles/pi4.raucb".into(),
            staged_version: None,
        })
    }
    pub async fn update_upload(
        State(_m): State<crate::mock::MockState>,
        mut multipart: Multipart,
    ) -> StatusCode {
        tracing::info!("[mock] OTA manual upload started");
        while let Ok(Some(mut field)) = multipart.next_field().await {
            while let Ok(Some(chunk)) = field.chunk().await {
                let _len = chunk.len();
            }
        }
        tracing::info!("[mock] OTA manual upload completed");
        StatusCode::OK
    }
    pub async fn update_install(State(m): State<crate::mock::MockState>) -> StatusCode {
        m.mock_install().await;
        StatusCode::ACCEPTED
    }
    pub async fn m_get_auto_update() -> Json<AutoUpdateConfig> {
        Json(AutoUpdateConfig {
            enabled: true,
            channel: "release".into(),
            interval: "daily".into(),
            time: "04:00".into(),
        })
    }
    pub async fn m_put_auto_update(Json(_body): Json<AutoUpdateConfig>) -> StatusCode {
        tracing::info!("[mock] set auto-update");
        StatusCode::OK
    }
    pub async fn get_update_status(State(m): State<crate::mock::MockState>) -> Json<UpdateStatus> {
        // Scripted lifecycle so the dev UI exercises the real polling path: after a
        // mock install is triggered, report "installing" with climbing progress for a
        // few seconds, then "idle". Without this the frontend (which now polls until
        // installing→idle) would never observe an install and hang until timeout.
        Json(m.update_status().await)
    }
    pub async fn factory_reset(State(_m): State<crate::mock::MockState>) -> StatusCode {
        tracing::info!("[mock] factory reset");
        StatusCode::ACCEPTED
    }
    pub async fn get_logs(Query(query): Query<super::LogsQuery>) -> Json<LogsResponse> {
        let svc = query.service.as_deref().unwrap_or("all");
        Json(LogsResponse {
            lines: vec![
                format!("[mock] [{svc}] snapdog-ctrl started"),
                format!("[mock] [{svc}] snapdog-client connected"),
            ],
        })
    }
    pub async fn get_timezone() -> Json<TimezoneInfo> {
        Json(TimezoneInfo {
            timezone: "Europe/Berlin".into(),
            available: vec![
                "Europe/Berlin".into(),
                "Europe/London".into(),
                "America/New_York".into(),
                "Asia/Tokyo".into(),
                "UTC".into(),
            ],
        })
    }
    pub async fn put_timezone(Json(b): Json<TimezoneUpdate>) -> StatusCode {
        tracing::info!("[mock] set timezone: {}", b.timezone);
        StatusCode::OK
    }
    pub async fn get_network(State(m): State<crate::mock::MockState>) -> Json<NetworkOverview> {
        Json(m.get_network_overview().await)
    }
    pub async fn get_ethernet(State(m): State<crate::mock::MockState>) -> Json<EthernetInfo> {
        Json(m.get_ethernet().await)
    }
    pub async fn put_ethernet(
        State(m): State<crate::mock::MockState>,
        Json(b): Json<EthernetConfig>,
    ) -> StatusCode {
        m.set_ethernet(b)
            .await
            .map_or(StatusCode::INTERNAL_SERVER_ERROR, |()| StatusCode::OK)
    }
    pub async fn get_wifi(State(m): State<crate::mock::MockState>) -> Json<WifiInfo> {
        Json(m.get_wifi().await)
    }
    pub async fn put_wifi(
        State(m): State<crate::mock::MockState>,
        Json(b): Json<WifiConfig>,
    ) -> StatusCode {
        m.set_wifi(&b.ssid, &b.password, None)
            .await
            .map_or(StatusCode::INTERNAL_SERVER_ERROR, |()| StatusCode::OK)
    }
    pub async fn delete_wifi(State(m): State<crate::mock::MockState>) -> StatusCode {
        m.delete_wifi()
            .await
            .map_or(StatusCode::INTERNAL_SERVER_ERROR, |()| StatusCode::OK)
    }
    pub async fn wifi_scan(State(m): State<crate::mock::MockState>) -> Json<WifiScanResult> {
        Json(m.wifi_scan().await)
    }
    pub async fn get_audio(State(m): State<crate::mock::MockState>) -> Json<AudioInfo> {
        Json(m.get_audio().await)
    }
    pub async fn put_audio(
        State(m): State<crate::mock::MockState>,
        Extension(crate::ws::WsSender(tx)): Extension<crate::ws::WsSender>,
        Json(b): Json<AudioConfig>,
    ) -> StatusCode {
        let status = m
            .set_audio_overlay(&b.overlay)
            .await
            .map_or(StatusCode::INTERNAL_SERVER_ERROR, |()| StatusCode::OK);
        if status == StatusCode::OK {
            let _ = tx.send("audio_changed".to_string());
        }
        status
    }
    pub async fn get_client(State(m): State<crate::mock::MockState>) -> Json<ClientConfig> {
        Json(m.get_client().await)
    }
    pub async fn put_client(
        State(m): State<crate::mock::MockState>,
        Extension(crate::ws::WsSender(tx)): Extension<crate::ws::WsSender>,
        Json(b): Json<ClientConfig>,
    ) -> StatusCode {
        let status = m
            .set_client(b)
            .await
            .map_or(StatusCode::INTERNAL_SERVER_ERROR, |()| StatusCode::OK);
        if status == StatusCode::OK {
            let _ = tx.send("client_changed".to_string());
        }
        status
    }
    pub async fn get_ssh(State(m): State<crate::mock::MockState>) -> Json<SshConfig> {
        Json(m.get_ssh().await)
    }
    pub async fn put_ssh(
        State(m): State<crate::mock::MockState>,
        Json(b): Json<SshConfig>,
    ) -> StatusCode {
        m.set_ssh(b)
            .await
            .map_or(StatusCode::INTERNAL_SERVER_ERROR, |()| StatusCode::OK)
    }
    pub async fn get_server() -> Json<crate::server_config::ServerConfig> {
        use crate::server_config::{ClientEntry, RadioStation, ServerConfig, ZoneConfig};
        Json(ServerConfig {
            zones: vec![ZoneConfig {
                name: "Living Room".into(),
                icon: "🛋️".into(),
                knx: None,
            }],
            clients: vec![ClientEntry {
                name: "Kitchen".into(),
                mac: "aa:bb:cc:dd:ee:ff".into(),
                zone: "Living Room".into(),
                icon: "🍽️".into(),
                max_volume: 100,
                knx: None,
            }],
            radio: vec![RadioStation {
                name: "SWR3".into(),
                url: "https://swr3.de/stream".into(),
                cover: None,
            }],
            ..ServerConfig::default()
        })
    }
    pub async fn put_server(
        Extension(crate::ws::WsSender(tx)): Extension<crate::ws::WsSender>,
        Json(_body): Json<crate::server_config::ServerConfig>,
    ) -> StatusCode {
        tracing::info!("[mock] put_server");
        let _ = tx.send("server_changed".to_string());
        StatusCode::OK
    }
    static SERVER_ENABLED: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);
    pub async fn get_server_status() -> Json<super::ServerStatus> {
        let enabled = SERVER_ENABLED.load(std::sync::atomic::Ordering::Relaxed);
        Json(super::ServerStatus {
            enabled,
            running: enabled,
        })
    }
    pub async fn post_server_enable(
        Extension(crate::ws::WsSender(tx)): Extension<crate::ws::WsSender>,
    ) -> StatusCode {
        SERVER_ENABLED.store(true, std::sync::atomic::Ordering::Relaxed);
        tracing::info!("[mock] server enabled");
        let _ = tx.send("server_changed".to_string());
        StatusCode::ACCEPTED
    }
    pub async fn post_server_disable(
        Extension(crate::ws::WsSender(tx)): Extension<crate::ws::WsSender>,
    ) -> StatusCode {
        SERVER_ENABLED.store(false, std::sync::atomic::Ordering::Relaxed);
        tracing::info!("[mock] server disabled");
        let _ = tx.send("server_changed".to_string());
        StatusCode::ACCEPTED
    }

    // Auth (no-op in mock — always authenticated)
    pub async fn get_auth_status() -> Json<serde_json::Value> {
        Json(serde_json::json!({"enabled": false, "authenticated": true}))
    }
    pub async fn post_auth_login() -> StatusCode {
        StatusCode::BAD_REQUEST
    }
    pub async fn post_auth_logout() -> StatusCode {
        StatusCode::NO_CONTENT
    }
    pub async fn put_auth_password() -> StatusCode {
        StatusCode::NO_CONTENT
    }

    // SoftAP
    pub async fn get_softap() -> Json<serde_json::Value> {
        Json(serde_json::json!({"enabled": true, "password": "snapdog123"}))
    }
    pub async fn put_softap(Json(_body): Json<serde_json::Value>) -> StatusCode {
        tracing::info!("[mock] set softap config");
        StatusCode::OK
    }

    // Client discovery
    pub async fn scan_servers() -> Json<serde_json::Value> {
        Json(serde_json::json!({"servers": [
            {"name": "Living Room", "host": "192.168.1.100", "port": 1780},
            {"name": "Kitchen", "host": "192.168.1.101", "port": 1780}
        ]}))
    }
    pub async fn test_server(Json(_body): Json<serde_json::Value>) -> Json<serde_json::Value> {
        Json(serde_json::json!({"reachable": true}))
    }
}

#[cfg(debug_assertions)]
pub fn api_mock(state: crate::mock::MockState) -> Router {
    use mock_handlers as h;

    Router::new()
        .route("/ws", get(crate::ws::ws_handler))
        // Auth
        .route("/auth/status", get(h::get_auth_status))
        .route("/auth/login", post(h::post_auth_login))
        .route("/auth/logout", post(h::post_auth_logout))
        .route("/auth/password", put(h::put_auth_password))
        // System
        .route("/system", get(h::get_system).put(h::put_system))
        .route("/system/tuning", get(h::get_tuning).put(h::put_tuning))
        .route("/system/health", get(get_health))
        .route("/system/reboot", post(h::reboot))
        .route("/system/update", post(h::update))
        .route("/system/update/check", get(h::get_update_check))
        .route("/system/update/status", get(h::get_update_status))
        .route(
            "/system/update/upload",
            // Match production: lift the 2 MB DefaultBodyLimit so large dev uploads
            // behave like a device instead of 413-ing.
            post(h::update_upload).layer(DefaultBodyLimit::disable()),
        )
        .route("/system/update/install", post(h::update_install))
        .route(
            "/system/update/auto",
            get(h::m_get_auto_update).put(h::m_put_auto_update),
        )
        .route("/system/factory-reset", post(h::factory_reset))
        .route("/system/logs", get(h::get_logs))
        .route(
            "/system/timezone",
            get(h::get_timezone).put(h::put_timezone),
        )
        // Network
        .route("/network", get(h::get_network))
        .route(
            "/network/ethernet",
            get(h::get_ethernet).put(h::put_ethernet),
        )
        .route(
            "/network/wifi",
            get(h::get_wifi).put(h::put_wifi).delete(h::delete_wifi),
        )
        .route("/network/wifi/scan", post(h::wifi_scan))
        .route("/network/softap", get(h::get_softap).put(h::put_softap))
        // Audio
        .route("/audio", get(h::get_audio).put(h::put_audio))
        // Client
        .route("/client", get(h::get_client).put(h::put_client))
        .route("/client/scan-servers", post(h::scan_servers))
        .route("/client/test-server", post(h::test_server))
        // SSH
        .route("/ssh", get(h::get_ssh).put(h::put_ssh))
        // Server
        .route("/server", get(h::get_server).put(h::put_server))
        .route("/server/status", get(h::get_server_status))
        .route("/server/enable", post(h::post_server_enable))
        .route("/server/disable", post(h::post_server_disable))
        .with_state(state)
        .fallback(api_not_found)
}

// --- System ---

#[derive(Serialize)]
pub struct SystemInfo {
    pub hostname: String,
    pub version: String,
    pub channel: String,
    pub uptime_seconds: u64,
    pub board_model: String,
    pub components: ComponentVersions,
}

#[derive(Serialize, Clone)]
pub struct ComponentVersions {
    pub server: String,
    pub client: String,
    pub ctrl: String,
    pub kernel: String,
}

#[derive(Deserialize)]
pub struct SystemUpdate {
    pub hostname: Option<String>,
    pub channel: Option<String>,
}

#[derive(Serialize)]
pub struct LogsResponse {
    pub lines: Vec<String>,
}

#[derive(Deserialize)]
pub struct LogsQuery {
    pub service: Option<String>,
}

#[derive(Serialize)]
pub struct TimezoneInfo {
    pub timezone: String,
    pub available: Vec<String>,
}

#[derive(Deserialize)]
pub struct TimezoneUpdate {
    pub timezone: String,
}

#[derive(Clone)]
pub struct HealthState(pub std::sync::Arc<Vec<system::HealthWarning>>);

impl HealthState {
    pub fn is_critical(&self) -> bool {
        self.0.iter().any(|w| w.severity == "critical")
    }
}

/// Middleware: in critical mode, only /api/system/health and /api/system/reboot are allowed.
pub async fn degraded_mode_guard(
    health: HealthState,
    req: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> Result<axum::response::Response, StatusCode> {
    if !health.is_critical() {
        return Ok(next.run(req).await);
    }

    let path = req.uri().path();
    if path == "/api/system/health" || path == "/api/system/reboot" || !path.starts_with("/api/") {
        Ok(next.run(req).await)
    } else {
        Err(StatusCode::SERVICE_UNAVAILABLE)
    }
}

async fn get_health(Extension(health): Extension<HealthState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "ok": health.0.is_empty(),
        "warnings": *health.0,
    }))
}

async fn get_system() -> Json<SystemInfo> {
    Json(system::get_system_info().await)
}

async fn put_system(Json(body): Json<SystemUpdate>) -> StatusCode {
    if let Err(e) = system::set_system(body.hostname, body.channel).await {
        tracing::error!("put_system: {e}");
        return StatusCode::INTERNAL_SERVER_ERROR;
    }
    StatusCode::OK
}

async fn get_tuning() -> Result<Json<crate::tuning::TuningConfig>, StatusCode> {
    let driver = crate::tuning::get_active_driver().await;
    match driver.get_config().await {
        Ok(cfg) => Ok(Json(cfg)),
        Err(e) => {
            tracing::error!("get_tuning error: {e}");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn put_tuning(
    Extension(crate::ws::WsSender(tx)): Extension<crate::ws::WsSender>,
    Json(body): Json<crate::tuning::TuningConfig>,
) -> StatusCode {
    let driver = crate::tuning::get_active_driver().await;
    match driver.set_config(&body).await {
        Ok(()) => {
            let _ = tx.send("system_changed".to_string());
            StatusCode::OK
        }
        Err(e) => {
            tracing::error!("put_tuning error: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

async fn post_reboot() -> StatusCode {
    system::reboot().await;
    StatusCode::ACCEPTED
}

// Serialized API DTO consumed by the web UI — the boolean fields mirror the
// TypeScript `UpdateCheck` shape, so they can't be collapsed into an enum.
#[allow(clippy::struct_excessive_bools)]
#[derive(Serialize)]
pub struct UpdateCheckResponse {
    pub available: bool,
    pub installable: bool,
    pub current_version: String,
    pub latest_version: String,
    pub channel: String,
    pub is_downgrade: bool,
    pub signature_verified: bool,
    pub bundle_url: String,
    /// Version already installed to the boot slot and awaiting a reboot to activate
    /// (RAUC `primary` != booted). `None` in the normal state. Lets the UI offer a
    /// reboot instead of re-installing the same version.
    pub staged_version: Option<String>,
}

#[derive(Serialize)]
pub struct UpdateStatus {
    pub operation: String,
    pub progress: Option<crate::rauc::InstallProgress>,
    pub last_error: String,
    /// True when the most recently installed bundle failed to boot and the
    /// bootloader auto-reverted to the previous slot (a persisted failed-update
    /// marker). Lets the UI surface a rollback that happened while it was offline.
    pub rolled_back: bool,
    pub slots: Vec<crate::rauc::SlotStatus>,
}

async fn get_update_check() -> Json<UpdateCheckResponse> {
    Json(system::check_update().await)
}

async fn get_update_status() -> Result<Json<UpdateStatus>, StatusCode> {
    let operation = system::rauc_operation()
        .await
        .unwrap_or_else(|_| "unknown".into());
    let progress = if operation == "installing" {
        system::rauc_progress().await.ok()
    } else {
        None
    };
    // Await the property read: `now_or_never` polled the pending D-Bus Get once and
    // almost always got `""`, so a genuine async install failure was never surfaced.
    let last_error = match crate::rauc::Rauc::connect().await {
        Ok(r) => r.last_error().await.unwrap_or_default(),
        Err(_) => String::new(),
    };
    let rolled_back = system::last_failed_update().await.is_some();
    let slots = system::rauc_slot_status().await.unwrap_or_default();
    Ok(Json(UpdateStatus {
        operation,
        progress,
        last_error,
        rolled_back,
        slots,
    }))
}

#[derive(Serialize, Deserialize, Clone)]
pub struct AutoUpdateConfig {
    pub enabled: bool,
    pub channel: String,
    pub interval: String,
    pub time: String,
}

async fn get_auto_update() -> Json<AutoUpdateConfig> {
    Json(system::get_auto_update().await)
}

async fn put_auto_update(Json(body): Json<AutoUpdateConfig>) -> StatusCode {
    if let Err(e) = system::set_auto_update(body).await {
        tracing::error!("put_auto_update: {e}");
        return StatusCode::INTERNAL_SERVER_ERROR;
    }
    StatusCode::OK
}

async fn post_update() -> StatusCode {
    // A concurrent install (e.g. a reload + a second "Install") would make RAUC
    // error out — surface a clean 409 instead of a 500 so the UI can say "already
    // installing" rather than "update failed".
    if matches!(system::rauc_operation().await.as_deref(), Ok("installing")) {
        return StatusCode::CONFLICT;
    }
    // Install from the channel's bundle URL
    let config = system::get_auto_update().await;
    let url = system::bundle_url(&config.channel).await;
    if let Err(e) = system::rauc_install(&url).await {
        tracing::error!("post_update: {e}");
        return StatusCode::INTERNAL_SERVER_ERROR;
    }
    StatusCode::ACCEPTED
}

/// Where the uploaded RAUC bundle is staged. Must live on a mount shared with the
/// rauc D-Bus service — NOT /tmp: snapdog-ctrl runs with `PrivateTmp=yes`, so a
/// bundle written to its private /tmp is invisible to rauc (install fails with
/// "No such file or directory"). The writable /data partition is shared by all
/// services and has ample space.
const UPDATE_BUNDLE_PATH: &str = "/data/update.raucb";

async fn post_update_upload(
    mut multipart: axum::extract::Multipart,
) -> Result<StatusCode, StatusCode> {
    use tokio::io::AsyncWriteExt;

    let dest = UPDATE_BUNDLE_PATH;
    let _ = tokio::fs::remove_file(dest).await;

    let mut file = match tokio::fs::File::create(dest).await {
        Ok(f) => f,
        Err(e) => {
            tracing::error!("Failed to create {dest}: {e}");
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    while let Some(mut field) = multipart.next_field().await.map_err(|e| {
        tracing::error!("upload: reading multipart failed: {e}");
        StatusCode::BAD_REQUEST
    })? {
        // Drain the field explicitly so a stream error (e.g. a dropped
        // connection) fails the request instead of silently truncating the file
        // and returning success — a partial bundle makes rauc reject the install.
        loop {
            match field.chunk().await {
                Ok(Some(chunk)) => {
                    if let Err(e) = file.write_all(&chunk).await {
                        tracing::error!("upload: write chunk failed: {e}");
                        return Err(StatusCode::INTERNAL_SERVER_ERROR);
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    tracing::error!("upload: reading chunk failed: {e}");
                    return Err(StatusCode::BAD_REQUEST);
                }
            }
        }
    }

    if let Err(e) = file.flush().await {
        tracing::error!("upload: flush failed: {e}");
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    Ok(StatusCode::OK)
}

async fn post_update_install() -> StatusCode {
    // Refuse a concurrent install with 409 rather than a RAUC 500 (see post_update).
    if matches!(system::rauc_operation().await.as_deref(), Ok("installing")) {
        return StatusCode::CONFLICT;
    }
    // Install the uploaded bundle (staged on shared /data, see UPDATE_BUNDLE_PATH).
    // rauc's D-Bus InstallBundle is ASYNCHRONOUS: install_bundle() returns as soon
    // as the install is triggered, not when it completes. So we must NOT delete the
    // staged bundle here — rauc reads it in the background and removing it mid-install
    // fails with "No such file". The next upload clears the stale bundle before
    // writing a new one.
    if let Err(e) = system::rauc_install(UPDATE_BUNDLE_PATH).await {
        tracing::error!("post_update_install: {e}");
        return StatusCode::INTERNAL_SERVER_ERROR;
    }
    StatusCode::ACCEPTED
}

// --- Raw Flash (Escape Hatch) ---

use std::sync::OnceLock;
use tokio::sync::Mutex;

struct PendingFlash {
    challenge: String,
    expires: std::time::Instant,
}

const RAW_FLASH_CHALLENGE_TTL_SECONDS: u64 = 120;

static PENDING_FLASH: OnceLock<Mutex<Option<PendingFlash>>> = OnceLock::new();

fn flash_lock() -> &'static Mutex<Option<PendingFlash>> {
    PENDING_FLASH.get_or_init(|| Mutex::new(None))
}

#[derive(Serialize)]
struct FlashChallengeResponse {
    challenge: String,
    expires_in_seconds: u64,
}

async fn post_flash_raw_upload(
    mut multipart: axum::extract::Multipart,
) -> Result<Json<FlashChallengeResponse>, StatusCode> {
    let dest = "/tmp/pending-flash.img.gz";
    let _ = tokio::fs::remove_file(dest).await;

    let mut file = tokio::fs::File::create(dest).await.map_err(|e| {
        tracing::error!("flash-raw create: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    while let Ok(Some(mut field)) = multipart.next_field().await {
        while let Ok(Some(chunk)) = field.chunk().await {
            use tokio::io::AsyncWriteExt;
            file.write_all(&chunk).await.map_err(|e| {
                tracing::error!("flash-raw write: {e}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
        }
    }

    // Generate challenge
    let challenge = rand::distr::Alphanumeric
        .sample_string(&mut rand::rng(), 6)
        .to_uppercase();
    let expires =
        std::time::Instant::now() + std::time::Duration::from_secs(RAW_FLASH_CHALLENGE_TTL_SECONDS);

    *flash_lock().lock().await = Some(PendingFlash {
        challenge: challenge.clone(),
        expires,
    });

    Ok(Json(FlashChallengeResponse {
        challenge,
        expires_in_seconds: RAW_FLASH_CHALLENGE_TTL_SECONDS,
    }))
}

#[derive(Deserialize)]
struct FlashConfirmRequest {
    challenge: String,
}

async fn post_flash_raw_confirm(Json(body): Json<FlashConfirmRequest>) -> StatusCode {
    let mut lock = flash_lock().lock().await;
    let valid = lock
        .as_ref()
        .is_some_and(|p| p.challenge == body.challenge && p.expires > std::time::Instant::now());

    if !valid {
        return StatusCode::FORBIDDEN;
    }

    // Consume the challenge
    *lock = None;
    drop(lock);

    // Flash to inactive partition
    tokio::spawn(async {
        if let Err(e) = system::flash_raw_image("/tmp/pending-flash.img.gz").await {
            tracing::error!("flash-raw failed: {e}");
        }
    });

    StatusCode::ACCEPTED
}

async fn post_factory_reset() -> StatusCode {
    if let Err(e) = system::factory_reset().await {
        tracing::error!("post_factory_reset: {e}");
        return StatusCode::INTERNAL_SERVER_ERROR;
    }
    StatusCode::ACCEPTED
}

async fn get_logs(Query(query): Query<LogsQuery>) -> Json<LogsResponse> {
    Json(system::get_logs(query.service).await)
}

async fn get_timezone() -> Json<TimezoneInfo> {
    Json(system::get_timezone().await)
}

async fn put_timezone(Json(body): Json<TimezoneUpdate>) -> StatusCode {
    if let Err(e) = system::set_timezone(&body.timezone).await {
        tracing::error!("put_timezone: {e}");
        return StatusCode::INTERNAL_SERVER_ERROR;
    }
    StatusCode::OK
}

// --- Network ---

#[derive(Serialize)]
pub struct NetworkOverview {
    pub ethernet: EthernetInfo,
    pub wifi: WifiInfo,
}

#[derive(Serialize, Clone)]

pub struct EthernetInfo {
    pub connected: bool,
    pub mode: String,
    pub ip: String,
    pub subnet: String,
    pub gateway: String,
    pub dns: String,
}

#[derive(Serialize)]
pub struct WifiInfo {
    pub connected: bool,
    pub ssid: String,
    pub ip: String,
    pub subnet: String,
    pub gateway: String,
    pub dns: String,
    pub signal: i32,
    pub mode: String,
    /// Connection lifecycle for UI feedback:
    /// `disconnected` | `associating` | `auth_failed` | `acquiring_ip` | `connected`.
    pub state: String,
}

#[derive(Deserialize)]
pub struct EthernetConfig {
    pub mode: String,
    pub ip: Option<String>,
    pub subnet: Option<String>,
    pub gateway: Option<String>,
    pub dns: Option<String>,
}

#[derive(Deserialize)]
pub struct WifiConfig {
    pub ssid: String,
    pub password: String,
    pub mode: Option<String>,
    pub ip: Option<String>,
    pub subnet: Option<String>,
    pub gateway: Option<String>,
    pub dns: Option<String>,
}

#[derive(Serialize)]
pub struct WifiNetwork {
    pub ssid: String,
    pub signal: i32,
    pub security: String,
}

#[derive(Serialize)]
pub struct WifiScanResult {
    pub networks: Vec<WifiNetwork>,
    /// `ok` (scan ran) | `unavailable_ap_mode` (single radio busy as setup AP) |
    /// `error` (scan failed). Lets the UI explain an empty list instead of showing
    /// a blank void.
    pub status: String,
    pub ap_active: bool,
}

async fn get_network() -> Json<NetworkOverview> {
    Json(system::get_network_overview().await)
}

async fn get_ethernet() -> Json<EthernetInfo> {
    Json(system::get_ethernet().await)
}

async fn put_ethernet(Json(body): Json<EthernetConfig>) -> StatusCode {
    if let Err(e) = system::set_ethernet(body).await {
        tracing::error!("put_ethernet: {e}");
        return StatusCode::INTERNAL_SERVER_ERROR;
    }
    StatusCode::OK
}

async fn get_wifi() -> Json<WifiInfo> {
    Json(system::get_wifi().await)
}

async fn put_wifi(Json(body): Json<WifiConfig>) -> StatusCode {
    // A non-empty passphrase must be a valid WPA length (8..=63). Reject early with
    // 400 rather than persist a config wpa_supplicant can't parse — a psk="" config
    // breaks the supplicant for ALL operations, scanning included. An empty
    // passphrase is a valid open network (handled as key_mgmt=NONE downstream).
    let pw_len = body.password.chars().count();
    if pw_len != 0 && !(8..=63).contains(&pw_len) {
        return StatusCode::BAD_REQUEST;
    }
    let static_cfg =
        body.mode
            .as_deref()
            .filter(|m| *m == "static")
            .map(|_| crate::network::StaticConfig {
                ip: body.ip.unwrap_or_default(),
                subnet: body.subnet.unwrap_or_else(|| "255.255.255.0".into()),
                gateway: body.gateway.unwrap_or_default(),
                dns: body.dns.unwrap_or_default(),
            });
    if let Err(e) = system::set_wifi(&body.ssid, &body.password, static_cfg.as_ref()).await {
        tracing::error!("put_wifi: {e}");
        return StatusCode::INTERNAL_SERVER_ERROR;
    }
    // 202: config accepted; association happens asynchronously and the setup AP
    // is torn down on a short delay. The client polls GET /network/wifi (state).
    StatusCode::ACCEPTED
}

async fn delete_wifi() -> StatusCode {
    if let Err(e) = system::delete_wifi().await {
        tracing::error!("delete_wifi: {e}");
        return StatusCode::INTERNAL_SERVER_ERROR;
    }
    StatusCode::OK
}

async fn post_wifi_scan() -> Json<WifiScanResult> {
    Json(system::wifi_scan().await)
}

/// Returns a masked `SoftApView`: the passphrase is included only while the setup
/// AP is actually running (so it can be shown for first-join) and withheld
/// otherwise, plus the SSID + country for display.
async fn get_softap() -> Json<system::SoftApView> {
    Json(system::get_softap_view().await)
}

async fn put_softap(Json(body): Json<system::SoftApConfig>) -> impl axum::response::IntoResponse {
    if body.password.len() < 8 || body.password.contains('\n') || body.password.contains('\r') {
        return (
            StatusCode::BAD_REQUEST,
            "password must be at least 8 characters",
        )
            .into_response();
    }
    // Lockout guard: refuse to disable the setup AP unless the device currently
    // has a working way in (wifi associated or ethernet with an IP), otherwise it
    // becomes permanently unreachable.
    if !body.enabled && !system::has_connectivity().await {
        return (
            StatusCode::CONFLICT,
            "cannot disable the setup access point: the device has no other working \
             connection (connect WiFi or plug in ethernet first)",
        )
            .into_response();
    }
    if let Err(e) = system::set_softap_config(body).await {
        tracing::error!("put_softap: {e}");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    StatusCode::OK.into_response()
}

// --- Audio ---

#[derive(Serialize)]
pub struct AudioInfo {
    pub overlay: String,
    pub detected_card: String,
    pub detected_hat: String,
    pub soundcard: String,
    pub available_overlays: Vec<DacOverlay>,
}

#[derive(Serialize)]
pub struct DacOverlay {
    pub id: String,
    pub name: String,
}

#[derive(Deserialize)]
pub struct AudioConfig {
    pub overlay: String,
}

async fn get_audio() -> Json<AudioInfo> {
    Json(system::get_audio().await)
}

async fn put_audio(
    Extension(crate::ws::WsSender(tx)): Extension<crate::ws::WsSender>,
    Json(body): Json<AudioConfig>,
) -> StatusCode {
    if let Err(e) = system::set_audio_overlay(&body.overlay).await {
        tracing::error!("put_audio: {e}");
        return StatusCode::INTERNAL_SERVER_ERROR;
    }
    let _ = tx.send("audio_changed".to_string());
    StatusCode::OK
}

// --- Client ---

/// An ALSA playback device offered in the client soundcard dropdown.
#[derive(Serialize, Clone)]
pub struct Soundcard {
    /// ALSA device string passed to `--soundcard` (e.g. `hw:0`).
    pub device: String,
    /// Human-readable card name for the dropdown label.
    pub name: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ClientConfig {
    pub server_url: String,
    pub host_id: String,
    pub soundcard: String,
    pub mixer: String,
    pub latency: i32,
    #[serde(skip_deserializing)]
    pub mdns_name: String,
    #[serde(skip_deserializing)]
    pub running: bool,
    #[serde(skip_deserializing)]
    pub available_soundcards: Vec<Soundcard>,
}

async fn get_client() -> Json<ClientConfig> {
    Json(system::get_client().await)
}

async fn put_client(
    Extension(crate::ws::WsSender(tx)): Extension<crate::ws::WsSender>,
    Json(body): Json<ClientConfig>,
) -> StatusCode {
    if let Err(e) = system::set_client(body).await {
        tracing::error!("put_client: {e}");
        return StatusCode::INTERNAL_SERVER_ERROR;
    }
    let _ = tx.send("client_changed".to_string());
    StatusCode::OK
}

#[derive(Serialize)]
pub struct DiscoveredServer {
    pub name: String,
    pub host: String,
    pub port: u16,
}

#[derive(Serialize)]
pub struct ScanServersResponse {
    pub servers: Vec<DiscoveredServer>,
}

async fn post_scan_servers() -> Json<ScanServersResponse> {
    Json(system::scan_servers().await)
}

#[derive(Deserialize)]
pub struct TestServerRequest {
    pub host: String,
    pub port: u16,
}

#[derive(Serialize)]
pub struct TestServerResponse {
    pub reachable: bool,
}

async fn post_test_server(Json(body): Json<TestServerRequest>) -> Json<TestServerResponse> {
    let reachable = system::test_server(&body.host, body.port).await;
    Json(TestServerResponse { reachable })
}

// --- SSH ---

#[derive(Serialize, Deserialize, Clone)]
pub struct SshConfig {
    pub enabled: bool,
    pub pubkeys: Vec<String>,
}

async fn get_ssh() -> Json<SshConfig> {
    Json(system::get_ssh().await)
}

async fn put_ssh(Json(body): Json<SshConfig>) -> StatusCode {
    if let Err(e) = system::set_ssh(body).await {
        tracing::error!("put_ssh: {e}");
        return StatusCode::INTERNAL_SERVER_ERROR;
    }
    StatusCode::OK
}

// --- Server ---

#[derive(Serialize)]
pub struct ServerStatus {
    pub enabled: bool,
    pub running: bool,
}

async fn get_server() -> Result<Json<ServerConfig>, StatusCode> {
    server_config::read_config().await.map(Json).map_err(|e| {
        tracing::error!("get_server: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })
}

async fn put_server(
    Extension(crate::ws::WsSender(tx)): Extension<crate::ws::WsSender>,
    Json(body): Json<ServerConfig>,
) -> StatusCode {
    if let Err(e) = server_config::validate(&body) {
        tracing::error!("put_server validate: {e}");
        return StatusCode::BAD_REQUEST;
    }
    if let Err(e) = server_config::write_config(&body).await {
        tracing::error!("put_server write: {e}");
        return StatusCode::INTERNAL_SERVER_ERROR;
    }
    if let Err(e) = run_systemctl(&["restart", "snapdog"]).await {
        tracing::error!("put_server restart: {e}");
        return StatusCode::INTERNAL_SERVER_ERROR;
    }
    let _ = tx.send("server_changed".to_string());
    StatusCode::OK
}

async fn get_server_status() -> Json<ServerStatus> {
    let config = system::get_service_config().await;
    let enabled = config.get("server").copied().unwrap_or(false);
    let running = run_systemctl(&["is-active", "snapdog"]).await.is_ok();
    Json(ServerStatus { enabled, running })
}

async fn post_server_enable(
    Extension(crate::ws::WsSender(tx)): Extension<crate::ws::WsSender>,
) -> StatusCode {
    // Write default config if none exists
    if tokio::fs::metadata("/etc/snapdog/snapdog.toml")
        .await
        .is_err()
    {
        let default = server_config::default_config_toml();
        let _ = tokio::fs::create_dir_all("/etc/snapdog").await;
        if let Err(e) = tokio::fs::write("/etc/snapdog/snapdog.toml", default).await {
            tracing::error!("post_server_enable write default: {e}");
            return StatusCode::INTERNAL_SERVER_ERROR;
        }
    }
    if let Err(e) = system::set_service("server", true).await {
        tracing::error!("post_server_enable: {e}");
        return StatusCode::INTERNAL_SERVER_ERROR;
    }
    let _ = tx.send("server_changed".to_string());
    StatusCode::ACCEPTED
}

async fn post_server_disable(
    Extension(crate::ws::WsSender(tx)): Extension<crate::ws::WsSender>,
) -> StatusCode {
    if let Err(e) = system::set_service("server", false).await {
        tracing::error!("post_server_disable: {e}");
        return StatusCode::INTERNAL_SERVER_ERROR;
    }
    let _ = tx.send("server_changed".to_string());
    StatusCode::ACCEPTED
}

async fn run_systemctl(args: &[&str]) -> anyhow::Result<()> {
    let output = tokio::process::Command::new("systemctl")
        .args(args)
        .output()
        .await?;
    if output.status.success() {
        Ok(())
    } else {
        anyhow::bail!(
            "systemctl {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        )
    }
}

// ── Settings export/import ────────────────────────────────────

async fn get_settings_export() -> impl IntoResponse {
    match crate::settings::export_settings() {
        Ok(data) => (
            StatusCode::OK,
            [
                (
                    axum::http::header::CONTENT_TYPE,
                    "application/gzip".to_string(),
                ),
                (
                    axum::http::header::CONTENT_DISPOSITION,
                    "attachment; filename=\"snapdog-settings.tar.gz\"".to_string(),
                ),
            ],
            data,
        )
            .into_response(),
        Err(e) => {
            tracing::error!("settings export failed: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

async fn post_settings_preview(body: axum::body::Bytes) -> impl IntoResponse {
    match crate::settings::preview_settings(&body) {
        Ok(preview) => Json(preview).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn post_settings_import(body: axum::body::Bytes) -> impl IntoResponse {
    if let Err(e) = crate::settings::import_settings(&body) {
        tracing::error!("settings import failed: {e}");
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response();
    }

    tracing::info!("Settings imported, rebooting in 1s");
    tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        let _ = tokio::process::Command::new("reboot").output().await;
    });

    Json(serde_json::json!({"status": "ok", "rebooting": true})).into_response()
}

// ── Now Playing ───────────────────────────────────────────────

#[cfg(not(debug_assertions))]
async fn get_now_playing(
    Extension(state): Extension<crate::mpris_client::SharedNowPlaying>,
) -> impl IntoResponse {
    let np = state.lock().await;
    Json(serde_json::to_value(&*np).unwrap_or_default())
}

#[cfg(debug_assertions)]
async fn get_now_playing() -> impl IntoResponse {
    Json(serde_json::json!({
        "playing": false,
        "title": "",
        "artist": "",
        "album": "",
        "cover_url": null,
        "duration_ms": 0,
        "position_ms": 0,
        "seekable": false,
        "can_next": false,
        "can_prev": false,
        "volume": 100,
        "muted": false
    }))
}

#[cfg(not(debug_assertions))]
async fn post_now_playing_command(Json(body): Json<serde_json::Value>) -> StatusCode {
    let cmd = body.get("command").and_then(|v| v.as_str()).unwrap_or("");
    match crate::mpris_client::send_command(cmd).await {
        Ok(()) => StatusCode::OK,
        Err(e) => {
            tracing::error!("now-playing command failed: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

#[cfg(debug_assertions)]
async fn post_now_playing_command() -> StatusCode {
    StatusCode::OK
}

#[cfg(not(debug_assertions))]
async fn put_now_playing_volume(Json(body): Json<serde_json::Value>) -> StatusCode {
    let vol = body
        .get("volume")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(1.0);
    match crate::mpris_client::set_volume(vol / 100.0).await {
        Ok(()) => StatusCode::OK,
        Err(e) => {
            tracing::error!("now-playing volume failed: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

#[cfg(debug_assertions)]
async fn put_now_playing_volume() -> StatusCode {
    StatusCode::OK
}

#[cfg(not(debug_assertions))]
async fn post_now_playing_seek(Json(body): Json<serde_json::Value>) -> StatusCode {
    let offset_ms = body
        .get("offset_ms")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0);
    match crate::mpris_client::seek(offset_ms * 1000).await {
        Ok(()) => StatusCode::OK,
        Err(e) => {
            tracing::error!("now-playing seek failed: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

#[cfg(debug_assertions)]
async fn post_now_playing_seek() -> StatusCode {
    StatusCode::OK
}
