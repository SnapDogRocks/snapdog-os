// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

use axum::{
    Extension, Json, Router,
    extract::{Query, Request},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use rust_embed::Embed;
use serde::{Deserialize, Serialize};

use crate::server_config::{self, ServerConfig};
use crate::system;

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
        .route("/system/reboot", post(post_reboot))
        .route("/system/update", post(post_update))
        .route("/system/update/check", get(get_update_check))
        .route("/system/update/status", get(get_update_status))
        .route("/system/update/upload", post(post_update_upload))
        .route("/system/update/install", post(post_update_install))
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
        req.headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .is_some_and(|t| {
                // Can't await inside is_some_and, use blocking check
                auth.0.tokens.try_read().is_ok_and(|set| set.contains(t))
            })
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
) -> Result<Json<LoginResponse>, StatusCode> {
    if !auth.is_enabled().await {
        return Err(StatusCode::BAD_REQUEST);
    }
    if auth.verify_password(&body.password).await {
        let token = auth.create_token().await;
        Ok(Json(LoginResponse { token }))
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
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
            current_version: "0.1.0".into(),
            channel: "stable".into(),
            bundle_url: "https://update.snapdog.cc/os/bundles/pi4.raucb".into(),
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
    pub async fn update_install(State(_m): State<crate::mock::MockState>) -> StatusCode {
        tracing::info!("[mock] OTA manual install triggered (extracting & rebooting)");
        StatusCode::ACCEPTED
    }
    pub fn m_get_auto_update() -> Json<AutoUpdateConfig> {
        Json(AutoUpdateConfig {
            enabled: true,
            channel: "stable".into(),
            interval: "daily".into(),
            time: "04:00".into(),
        })
    }
    pub fn m_put_auto_update(Json(_body): Json<AutoUpdateConfig>) -> StatusCode {
        tracing::info!("[mock] set auto-update");
        StatusCode::OK
    }
    pub async fn get_update_status() -> Json<UpdateStatus> {
        Json(UpdateStatus {
            operation: "idle".into(),
            progress: None,
            last_error: String::new(),
            slots: vec![],
        })
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
}

#[cfg(debug_assertions)]
pub fn api_mock(state: crate::mock::MockState) -> Router {
    use mock_handlers as h;

    Router::new()
        .route("/ws", get(crate::ws::ws_handler))
        .route("/system", get(h::get_system).put(h::put_system))
        .route("/system/reboot", post(h::reboot))
        .route("/system/update", post(h::update))
        .route("/system/update/check", get(h::get_update_check))
        .route("/system/update/status", get(h::get_update_status))
        .route("/system/update/upload", post(h::update_upload))
        .route("/system/update/install", post(h::update_install))
        .route("/system/factory-reset", post(h::factory_reset))
        .route("/system/logs", get(h::get_logs))
        .route(
            "/system/timezone",
            get(h::get_timezone).put(h::put_timezone),
        )
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
        .route("/audio", get(h::get_audio).put(h::put_audio))
        .route("/client", get(h::get_client).put(h::put_client))
        .route("/ssh", get(h::get_ssh).put(h::put_ssh))
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
    pub pi_version: u8,
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

async fn post_reboot() -> StatusCode {
    system::reboot().await;
    StatusCode::ACCEPTED
}

#[derive(Serialize)]
pub struct UpdateCheckResponse {
    pub available: bool,
    pub current_version: String,
    pub channel: String,
    pub bundle_url: String,
}

#[derive(Serialize)]
pub struct UpdateStatus {
    pub operation: String,
    pub progress: Option<crate::rauc::InstallProgress>,
    pub last_error: String,
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
    let last_error = crate::rauc::Rauc::connect()
        .await
        .ok()
        .and_then(|r| futures_util::FutureExt::now_or_never(r.last_error()))
        .and_then(Result::ok)
        .unwrap_or_default();
    let slots = system::rauc_slot_status().await.unwrap_or_default();
    Ok(Json(UpdateStatus {
        operation,
        progress,
        last_error,
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
    // Install from the channel's bundle URL
    let config = system::get_auto_update().await;
    let url = system::bundle_url(&config.channel);
    if let Err(e) = system::rauc_install(&url).await {
        tracing::error!("post_update: {e}");
        return StatusCode::INTERNAL_SERVER_ERROR;
    }
    StatusCode::ACCEPTED
}

async fn post_update_upload(
    mut multipart: axum::extract::Multipart,
) -> Result<StatusCode, StatusCode> {
    let dest = "/tmp/update.raucb";
    let _ = tokio::fs::remove_file(dest).await;

    let mut file = match tokio::fs::File::create(dest).await {
        Ok(f) => f,
        Err(e) => {
            tracing::error!("Failed to create {dest}: {e}");
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    while let Ok(Some(mut field)) = multipart.next_field().await {
        while let Ok(Some(chunk)) = field.chunk().await {
            use tokio::io::AsyncWriteExt;
            if let Err(e) = file.write_all(&chunk).await {
                tracing::error!("Failed to write chunk: {e}");
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
        }
    }

    Ok(StatusCode::OK)
}

async fn post_update_install() -> StatusCode {
    // Install the uploaded bundle
    if let Err(e) = system::rauc_install("/tmp/update.raucb").await {
        tracing::error!("post_update_install: {e}");
        return StatusCode::INTERNAL_SERVER_ERROR;
    }
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
    StatusCode::OK
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
    pub available_soundcards: Vec<String>,
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
