// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! Mock system backend for local development. Only available in debug builds.

use std::sync::Arc;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::routes::{
    AudioInfo, ClientConfig, EthernetConfig, EthernetInfo, NetworkOverview, Soundcard, SshConfig,
    SystemInfo, WifiInfo, WifiNetwork, WifiScanResult,
};

#[derive(Clone)]
pub struct MockState {
    inner: Arc<Mutex<State>>,
}

struct State {
    hostname: String,
    channel: String,
    ethernet: EthernetInfo,
    wifi_ssid: String,
    wifi_connected: bool,
    overlay: String,
    client: ClientConfig,
    ssh: SshConfig,
    tuning: crate::tuning::TuningConfig,
    /// When a mock install was last triggered — drives the scripted install
    /// lifecycle in `update_status()` so the dev UI exercises the real polling path.
    install_started: Option<std::time::Instant>,
    server_scenario: MockServerScenario,
    server_config: Option<crate::server_config::ServerConfig>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MockServerScenario {
    Unconfigured,
    Stopped,
    Starting,
    Healthy,
    InvalidConfig,
    Failed,
    RollbackSucceeded,
    RollbackFailed,
    Conflict,
}

impl MockState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(State {
                hostname: "snapdog-dev".into(),
                channel: "release".into(),
                ethernet: EthernetInfo {
                    connected: true,
                    mode: "dhcp".into(),
                    ip: "192.168.1.42".into(),
                    subnet: "255.255.255.0".into(),
                    gateway: "192.168.1.1".into(),
                    dns: "1.1.1.1".into(),
                },
                wifi_ssid: "DevNetwork".into(),
                wifi_connected: true,
                overlay: "allo-boss-dac-pcm512x-audio".into(),
                client: ClientConfig {
                    server_url: "tcp://192.168.1.10:1704".into(),
                    host_id: "kitchen".into(),
                    soundcard: "hw:0".into(),
                    mixer: "software".into(),
                    latency: 0,
                    mdns_name: "_snapdog._tcp".into(),
                    running: true,
                    available_soundcards: vec![Soundcard {
                        device: "hw:0".into(),
                        name: "Allo Boss DAC".into(),
                    }],
                },
                ssh: SshConfig {
                    enabled: false,
                    pubkeys: vec![],
                },
                tuning: crate::tuning::TuningConfig {
                    rf_kill_wifi: false,
                    rf_kill_bluetooth: false,
                    disable_onboard_audio: false,
                    exclusive_audio_core: false,
                },
                install_started: None,
                server_scenario: MockServerScenario::Unconfigured,
                server_config: None,
            })),
        }
    }

    pub async fn get_tuning(&self) -> crate::tuning::TuningConfig {
        self.inner.lock().await.tuning.clone()
    }

    pub async fn set_tuning(&self, config: crate::tuning::TuningConfig) {
        let mut s = self.inner.lock().await;
        s.tuning = config;
    }

    pub async fn get_system_info(&self) -> SystemInfo {
        let s = self.inner.lock().await;
        SystemInfo {
            hostname: s.hostname.clone(),
            version: env!("CARGO_PKG_VERSION").into(),
            channel: s.channel.clone(),
            uptime_seconds: 86400,
            board_model: "Mock SnapDog Board".into(),
            components: crate::routes::ComponentVersions {
                server: "0.11.3".into(),
                client: "0.11.3".into(),
                ctrl: env!("SNAPDOG_CTRL_VERSION").to_string(),
                kernel: "6.6.78-v8+".into(),
            },
        }
    }

    pub async fn set_system(
        &self,
        hostname: Option<String>,
        channel: Option<String>,
    ) -> Result<()> {
        let mut s = self.inner.lock().await;
        if let Some(h) = hostname {
            tracing::info!("[mock] set hostname: {h}");
            s.hostname = h;
        }
        if let Some(c) = channel {
            tracing::info!("[mock] set channel: {c}");
            s.channel = c;
        }
        drop(s);
        Ok(())
    }

    pub async fn reboot(&self) {
        let s = self.inner.lock().await;
        let hostname = s.hostname.clone();
        drop(s);
        tracing::info!("[mock] reboot requested for {hostname} (no-op)");
    }

    pub async fn trigger_update(&self) -> Result<()> {
        let mut s = self.inner.lock().await;
        let channel = s.channel.clone();
        s.install_started = Some(std::time::Instant::now());
        drop(s);
        tracing::info!("[mock] OTA update triggered for {channel} (scripted install)");
        Ok(())
    }

    /// Arm the scripted install lifecycle (manual upload → install path).
    pub async fn mock_install(&self) {
        self.inner.lock().await.install_started = Some(std::time::Instant::now());
        tracing::info!("[mock] OTA manual install triggered (scripted)");
    }

    /// Scripted phased status so the dev UI exercises the same truthful lifecycle
    /// as a device: byte-based download, indeterminate verification, image write,
    /// indeterminate finalization, then a retained ready-to-reboot state.
    pub async fn update_status(&self) -> crate::update::UpdateProgress {
        let s = self.inner.lock().await;
        let elapsed = s.install_started.map(|started| started.elapsed());
        drop(s);
        mock_update_progress(elapsed)
    }

    pub async fn get_network_overview(&self) -> NetworkOverview {
        NetworkOverview {
            ethernet: self.get_ethernet().await,
            wifi: self.get_wifi().await,
        }
    }

    pub async fn get_ethernet(&self) -> EthernetInfo {
        self.inner.lock().await.ethernet.clone()
    }

    pub async fn set_ethernet(&self, config: EthernetConfig) -> Result<()> {
        let mut s = self.inner.lock().await;
        tracing::info!("[mock] set ethernet: mode={}", config.mode);
        s.ethernet.mode = config.mode;
        s.ethernet.ip = config.ip.unwrap_or_default();
        s.ethernet.gateway = config.gateway.unwrap_or_default();
        s.ethernet.subnet = config.subnet.unwrap_or_default();
        s.ethernet.dns = config.dns.unwrap_or_default();
        drop(s);
        Ok(())
    }

    pub async fn get_wifi(&self) -> WifiInfo {
        let s = self.inner.lock().await;
        WifiInfo {
            connected: s.wifi_connected,
            ssid: s.wifi_ssid.clone(),
            ip: "192.168.1.43".into(),
            subnet: "255.255.255.0".into(),
            gateway: "192.168.1.1".into(),
            dns: "1.1.1.1".into(),
            signal: -52,
            mode: "dhcp".into(),
            state: if s.wifi_connected {
                "connected"
            } else {
                "disconnected"
            }
            .into(),
        }
    }

    pub async fn set_wifi(
        &self,
        ssid: &str,
        _password: &str,
        _static_cfg: Option<&crate::network::StaticConfig>,
    ) -> Result<()> {
        let mut s = self.inner.lock().await;
        tracing::info!("[mock] connect wifi: {ssid}");
        s.wifi_ssid = ssid.to_string();
        s.wifi_connected = true;
        drop(s);
        Ok(())
    }

    pub async fn delete_wifi(&self) -> Result<()> {
        let mut s = self.inner.lock().await;
        tracing::info!("[mock] disconnect wifi");
        s.wifi_ssid.clear();
        s.wifi_connected = false;
        drop(s);
        Ok(())
    }

    pub async fn wifi_scan(&self) -> WifiScanResult {
        let s = self.inner.lock().await;
        let connected = s.wifi_connected;
        drop(s);
        tracing::info!("[mock] wifi scan (connected={connected})");
        WifiScanResult {
            networks: vec![
                WifiNetwork {
                    ssid: "DevNetwork".into(),
                    signal: -45,
                    security: "wpa2".into(),
                },
                WifiNetwork {
                    ssid: "Neighbor-5G".into(),
                    signal: -72,
                    security: "wpa2".into(),
                },
                WifiNetwork {
                    ssid: "IoT-Guest".into(),
                    signal: -80,
                    security: "open".into(),
                },
            ],
            status: "ok".into(),
            ap_active: false,
        }
    }

    pub async fn get_audio(&self) -> AudioInfo {
        let s = self.inner.lock().await;
        AudioInfo {
            overlay: s.overlay.clone(),
            detected_card: "Mock Allo Boss DAC".into(),
            detected_hat: "hifiberry-dacplus".into(),
            soundcard: "hw:0".into(),
            available_overlays: crate::system::overlay_catalog(),
        }
    }

    pub async fn set_audio_overlay(&self, overlay: &str) -> Result<()> {
        let mut s = self.inner.lock().await;
        tracing::info!("[mock] set DAC overlay: {overlay}");
        s.overlay = overlay.to_string();
        drop(s);
        Ok(())
    }

    pub async fn get_client(&self) -> ClientConfig {
        self.inner.lock().await.client.clone()
    }

    pub async fn set_client(&self, config: ClientConfig) -> Result<()> {
        let mut s = self.inner.lock().await;
        tracing::info!(
            "[mock] set client: server={}, hostID={}",
            config.server_url,
            config.host_id
        );
        s.client = config;
        s.client.mdns_name = "_snapdog._tcp".into();
        s.client.running = true;
        s.client.available_soundcards = vec![Soundcard {
            device: "hw:0".into(),
            name: "Allo Boss DAC".into(),
        }];
        drop(s);
        Ok(())
    }

    pub async fn get_ssh(&self) -> SshConfig {
        self.inner.lock().await.ssh.clone()
    }

    pub async fn set_ssh(&self, config: SshConfig) -> Result<()> {
        let mut s = self.inner.lock().await;
        tracing::info!("[mock] set ssh: enabled={}", config.enabled);
        s.ssh = config;
        drop(s);
        Ok(())
    }

    pub async fn set_server_scenario(&self, scenario: MockServerScenario) {
        let mut state = self.inner.lock().await;
        state.server_config = if matches!(scenario, MockServerScenario::Unconfigured) {
            None
        } else {
            Some(
                state
                    .server_config
                    .take()
                    .unwrap_or_else(mock_server_config),
            )
        };
        if matches!(scenario, MockServerScenario::Conflict) {
            let current = state
                .server_config
                .take()
                .unwrap_or_else(mock_server_config);
            state.server_config = Some(mock_external_config_change(&current));
            // Conflict is represented by the changed revision, not by a broken
            // runtime. A fresh GET followed by rebase/apply must succeed.
            state.server_scenario = MockServerScenario::Healthy;
        } else {
            state.server_scenario = scenario;
        }
    }

    pub async fn get_server_legacy(&self) -> crate::server_config::ServerConfig {
        self.inner
            .lock()
            .await
            .server_config
            .clone()
            .unwrap_or_else(crate::server_manager::initial_config)
    }

    pub async fn server_state(&self) -> crate::server_manager::ServerState {
        self.server_state_with_scenario().await.0
    }

    async fn server_state_with_scenario(
        &self,
    ) -> (crate::server_manager::ServerState, MockServerScenario) {
        use crate::server_manager::{
            ConfigState, DesiredState, HealthState, OperationKind, OperationPhase, RuntimeState,
            ServerOperation, ServerState, SetupState,
        };

        let (scenario, revision) = {
            let state = self.inner.lock().await;
            (
                state.server_scenario,
                state
                    .server_config
                    .as_ref()
                    .map(|config| config.revision.clone()),
            )
        };
        let desired_running = !matches!(
            scenario,
            MockServerScenario::Unconfigured | MockServerScenario::Stopped
        );
        let running = matches!(
            scenario,
            MockServerScenario::Healthy | MockServerScenario::RollbackSucceeded
        );
        let healthy = running;
        let config_state = mock_config_state(scenario);
        let operation = matches!(scenario, MockServerScenario::Starting).then(|| ServerOperation {
            id: "mock-start".to_string(),
            kind: OperationKind::Start,
            phase: OperationPhase::Verifying,
            started_at: chrono::Utc::now().to_rfc3339(),
        });
        let issue = mock_server_issue(scenario);
        let state = ServerState {
            setup_state: match config_state {
                ConfigState::Missing => SetupState::NeedsSetup,
                ConfigState::Invalid | ConfigState::Unreadable => SetupState::NeedsRepair,
                ConfigState::Valid | ConfigState::ValidUnverified => SetupState::Configured,
            },
            desired_state: if desired_running {
                DesiredState::Running
            } else {
                DesiredState::Stopped
            },
            runtime_state: match scenario {
                MockServerScenario::Unconfigured | MockServerScenario::Stopped => {
                    RuntimeState::Stopped
                }
                MockServerScenario::Starting => RuntimeState::Starting,
                MockServerScenario::Healthy | MockServerScenario::RollbackSucceeded => {
                    RuntimeState::Running
                }
                _ => RuntimeState::Failed,
            },
            health_state: if healthy {
                HealthState::Healthy
            } else if matches!(scenario, MockServerScenario::Starting) {
                HealthState::Checking
            } else if desired_running {
                HealthState::Unhealthy
            } else {
                HealthState::Unknown
            },
            config_state,
            active_revision: healthy.then(|| revision.clone()).flatten(),
            last_good_revision: matches!(
                scenario,
                MockServerScenario::Healthy | MockServerScenario::RollbackSucceeded
            )
            .then(|| revision.clone())
            .flatten(),
            endpoint: healthy.then_some("http://localhost:5555".to_string()),
            operation,
            issue,
            enabled: desired_running,
            running,
        };
        (state, scenario)
    }

    pub async fn server_config_envelope(&self) -> crate::server_manager::ServerConfigEnvelope {
        use crate::server_manager::{ConfigState, ServerConfigEnvelope};

        let (scenario, config) = {
            let state = self.inner.lock().await;
            (state.server_scenario, state.server_config.clone())
        };
        let config_state = mock_config_state(scenario);
        // `GET /server/config` inspects the configuration file only. Runtime
        // failures belong to server state and diagnostics, not to this envelope.
        let issues = if matches!(scenario, MockServerScenario::InvalidConfig) {
            mock_server_issue(scenario).into_iter().collect()
        } else {
            Vec::new()
        };
        if let Some(config) = config {
            ServerConfigEnvelope {
                state: config_state,
                revision: config.revision.clone(),
                raw_toml: config.raw_toml.clone(),
                config: Some(config),
                issues,
            }
        } else {
            let initial = crate::server_manager::initial_config();
            ServerConfigEnvelope {
                state: ConfigState::Missing,
                revision: initial.revision.clone(),
                raw_toml: String::new(),
                config: Some(initial),
                issues: Vec::new(),
            }
        }
    }

    #[allow(clippy::result_large_err, clippy::unused_async)]
    pub async fn validate_server(
        &self,
        payload: crate::server_manager::ConfigPayload,
    ) -> std::result::Result<
        crate::server_manager::ValidationResponse,
        crate::server_manager::ManagerError,
    > {
        let config = payload.into_config();
        Ok(match validate_mock_server_config(&config) {
            Ok(()) => crate::server_manager::ValidationResponse {
                valid: true,
                issues: Vec::new(),
                config: None,
            },
            Err(issue) => crate::server_manager::ValidationResponse {
                valid: false,
                issues: vec![issue],
                config: None,
            },
        })
    }

    pub async fn apply_server(
        &self,
        payload: crate::server_manager::ConfigPayload,
    ) -> std::result::Result<crate::server_manager::ServerState, crate::server_manager::ManagerError>
    {
        let config = payload.into_config();
        let mut state = self.inner.lock().await;
        let baseline = state.server_config.as_ref().map_or_else(
            || crate::server_config::config_revision(""),
            |current| current.revision.clone(),
        );
        if config.revision != baseline {
            return Err(mock_manager_error(
                crate::server_manager::ManagerErrorKind::Conflict,
                "config_revision_conflict",
                "The configuration changed while it was being edited",
                "Reload the mock configuration and try again",
                None,
            ));
        }
        let parsed = build_mock_candidate(state.server_config.as_ref(), &config)?;
        let desired_running = !matches!(
            state.server_scenario,
            MockServerScenario::Unconfigured | MockServerScenario::Stopped
        );
        state.server_config = Some(parsed);
        state.server_scenario = if desired_running {
            MockServerScenario::Healthy
        } else {
            MockServerScenario::Stopped
        };
        drop(state);
        Ok(self.server_state().await)
    }

    pub async fn setup_server(
        &self,
        payload: crate::server_manager::SetupPayload,
    ) -> std::result::Result<crate::server_manager::ServerState, crate::server_manager::ManagerError>
    {
        let (config, start) = payload.into_parts();
        let mut state = self.inner.lock().await;
        let parsed = build_mock_candidate(state.server_config.as_ref(), &config)?;
        state.server_config = Some(parsed);
        state.server_scenario = if start {
            MockServerScenario::Healthy
        } else {
            MockServerScenario::Stopped
        };
        drop(state);
        Ok(self.server_state().await)
    }

    pub async fn server_action(
        &self,
        action: &str,
    ) -> std::result::Result<crate::server_manager::ServerState, crate::server_manager::ManagerError>
    {
        let mut state = self.inner.lock().await;
        if action == "stop" {
            state.server_scenario = if state.server_config.is_some() {
                MockServerScenario::Stopped
            } else {
                MockServerScenario::Unconfigured
            };
        } else if state.server_config.is_none() {
            return Err(crate::server_manager::setup_required_error());
        } else if matches!(state.server_scenario, MockServerScenario::InvalidConfig) {
            return Err(mock_manager_error(
                crate::server_manager::ManagerErrorKind::Invalid,
                "config_invalid",
                "The mock configuration is invalid",
                "Repair the highlighted field before starting",
                Some("audio.sample_rate"),
            ));
        } else {
            state.server_scenario = MockServerScenario::Healthy;
        }
        drop(state);
        Ok(self.server_state().await)
    }

    pub async fn server_diagnostics(&self) -> crate::server_manager::ServerDiagnostics {
        let (state, scenario) = self.server_state_with_scenario().await;
        let issue_line = state.issue.as_ref().map_or_else(
            || "Mock SnapDog server is healthy".to_string(),
            |issue| issue.detail.clone(),
        );
        crate::server_manager::ServerDiagnostics {
            generated_at: chrono::Utc::now().to_rfc3339(),
            state,
            systemd: mock_service_snapshot(scenario),
            journal: vec![format!("[mock] {issue_line}")],
        }
    }
}

const fn mock_config_state(scenario: MockServerScenario) -> crate::server_manager::ConfigState {
    use crate::server_manager::ConfigState;

    match scenario {
        MockServerScenario::Unconfigured => ConfigState::Missing,
        MockServerScenario::InvalidConfig => ConfigState::Invalid,
        MockServerScenario::Stopped | MockServerScenario::Starting => ConfigState::ValidUnverified,
        // Runtime and rollback failures do not make a parseable, validated
        // configuration invalid.
        _ => ConfigState::Valid,
    }
}

fn mock_service_snapshot(scenario: MockServerScenario) -> crate::server_manager::ServiceSnapshot {
    let (active_state, sub_state, result, exec_main_code, exec_main_status) = match scenario {
        MockServerScenario::Unconfigured | MockServerScenario::Stopped => {
            ("inactive", "dead", "success", Some(0), Some(0))
        }
        MockServerScenario::Starting => ("activating", "start", "", None, None),
        MockServerScenario::Healthy
        | MockServerScenario::RollbackSucceeded
        | MockServerScenario::Conflict => ("active", "running", "success", Some(0), Some(0)),
        MockServerScenario::InvalidConfig | MockServerScenario::Failed => {
            ("failed", "failed", "exit-code", Some(1), Some(78))
        }
        MockServerScenario::RollbackFailed => ("failed", "failed", "exit-code", Some(1), Some(1)),
    };

    crate::server_manager::ServiceSnapshot {
        load_state: "loaded".into(),
        active_state: active_state.into(),
        sub_state: sub_state.into(),
        result: result.into(),
        exec_main_code,
        exec_main_status,
        restart_count: Some(0),
        invocation_id: "mock-invocation".into(),
    }
}

fn mock_server_config() -> crate::server_config::ServerConfig {
    crate::server_config::default_server_config()
}

fn mock_external_config_change(
    current: &crate::server_config::ServerConfig,
) -> crate::server_config::ServerConfig {
    use std::sync::atomic::{AtomicU64, Ordering};

    static EXTERNAL_EDIT_SEQUENCE: AtomicU64 = AtomicU64::new(1);
    let sequence = EXTERNAL_EDIT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let source = format!(
        "{}\n# mock external edit {sequence}\n",
        current.raw_toml.trim_end()
    );
    crate::server_config::parse_config_toml(&source)
        .unwrap_or_else(|_| crate::server_config::default_server_config())
}

#[allow(clippy::result_large_err)]
fn build_mock_candidate(
    current: Option<&crate::server_config::ServerConfig>,
    config: &crate::server_config::ServerConfig,
) -> std::result::Result<crate::server_config::ServerConfig, crate::server_manager::ManagerError> {
    validate_mock_server_config(config).map_err(|issue| crate::server_manager::ManagerError {
        kind: crate::server_manager::ManagerErrorKind::Invalid,
        issue,
    })?;
    let source = current.map_or("", |current| current.raw_toml.as_str());
    let candidate = crate::server_config::render_candidate(source, config).map_err(|error| {
        mock_manager_error(
            crate::server_manager::ManagerErrorKind::Invalid,
            "config_invalid",
            "The mock configuration is invalid",
            &error.to_string(),
            None,
        )
    })?;
    crate::server_config::parse_config_toml(&candidate).map_err(|error| {
        mock_manager_error(
            crate::server_manager::ManagerErrorKind::Invalid,
            "config_invalid",
            "The mock configuration is invalid",
            &error.to_string(),
            None,
        )
    })
}

#[allow(clippy::result_large_err)]
fn validate_mock_server_config(
    config: &crate::server_config::ServerConfig,
) -> std::result::Result<(), crate::server_manager::ServerIssue> {
    crate::server_config::validate(config).map_err(|error| {
        let detail = error.to_string();
        crate::server_manager::ServerIssue {
            code: "config_invalid".into(),
            stage: "validation".into(),
            summary: "The mock configuration is invalid".into(),
            detail,
            field_path: None,
            line: None,
            column: None,
            exit_code: None,
            systemd_result: None,
            rollback_succeeded: None,
        }
    })
}

fn mock_manager_error(
    kind: crate::server_manager::ManagerErrorKind,
    code: &str,
    summary: &str,
    detail: &str,
    field_path: Option<&str>,
) -> crate::server_manager::ManagerError {
    crate::server_manager::ManagerError {
        kind,
        issue: crate::server_manager::ServerIssue {
            code: code.into(),
            stage: "mock".into(),
            summary: summary.into(),
            detail: detail.into(),
            field_path: field_path.map(str::to_string),
            line: None,
            column: None,
            exit_code: None,
            systemd_result: None,
            rollback_succeeded: None,
        },
    }
}

fn mock_server_issue(scenario: MockServerScenario) -> Option<crate::server_manager::ServerIssue> {
    let (code, summary, detail, rollback_succeeded, exit_code) = match scenario {
        MockServerScenario::InvalidConfig => (
            "config_invalid",
            "The mock configuration is invalid",
            "audio.sample_rate must be one of the supported values",
            None,
            None,
        ),
        MockServerScenario::Failed => (
            "service_failed",
            "SnapDog could not start",
            "mock systemd result exit-code",
            None,
            Some(78),
        ),
        MockServerScenario::RollbackSucceeded => (
            "readiness_timeout",
            "The new configuration failed and was rolled back",
            "mock readiness endpoint did not become healthy",
            Some(true),
            None,
        ),
        MockServerScenario::RollbackFailed => (
            "rollback_failed",
            "SnapDog and its recovery configuration both failed",
            "mock rollback verification failed",
            Some(false),
            Some(1),
        ),
        _ => return None,
    };
    Some(crate::server_manager::ServerIssue {
        code: code.into(),
        stage: "mock".into(),
        summary: summary.into(),
        detail: detail.into(),
        field_path: matches!(scenario, MockServerScenario::InvalidConfig)
            .then_some("audio.sample_rate".into()),
        line: None,
        column: None,
        exit_code,
        systemd_result: exit_code.map(|_| "exit-code".into()),
        rollback_succeeded,
    })
}

const MOCK_DOWNLOAD_END: std::time::Duration = std::time::Duration::from_secs(4);
const MOCK_VERIFY_END: std::time::Duration = std::time::Duration::from_secs(6);
const MOCK_WRITE_END: std::time::Duration = std::time::Duration::from_secs(10);
const MOCK_FINALIZE_END: std::time::Duration = std::time::Duration::from_secs(13);
const MOCK_BUNDLE_BYTES: u64 = 80 * 1024 * 1024;

fn scaled_value(elapsed: std::time::Duration, start_ms: u128, end_ms: u128, max: u64) -> u64 {
    let elapsed_ms = elapsed.as_millis().clamp(start_ms, end_ms) - start_ms;
    let duration_ms = end_ms - start_ms;
    let scaled = elapsed_ms * u128::from(max) / duration_ms;
    u64::try_from(scaled).unwrap_or(max)
}

fn mock_update_progress(elapsed: Option<std::time::Duration>) -> crate::update::UpdateProgress {
    use crate::update::{UpdatePhase, UpdateProgress};

    let Some(elapsed) = elapsed else {
        return UpdateProgress::default();
    };

    if elapsed <= MOCK_DOWNLOAD_END {
        let bytes_done = scaled_value(elapsed, 0, MOCK_DOWNLOAD_END.as_millis(), MOCK_BUNDLE_BYTES);
        return UpdateProgress {
            phase: UpdatePhase::Downloading,
            phase_progress: Some(
                u8::try_from(scaled_value(elapsed, 0, MOCK_DOWNLOAD_END.as_millis(), 100))
                    .unwrap_or(100),
            ),
            bytes_done: Some(bytes_done),
            bytes_total: Some(MOCK_BUNDLE_BYTES),
            detail: "Downloading firmware bundle".into(),
            ..UpdateProgress::default()
        };
    }

    if elapsed < MOCK_VERIFY_END {
        return UpdateProgress {
            phase: UpdatePhase::Verifying,
            detail: "Checking firmware bundle".into(),
            ..UpdateProgress::default()
        };
    }

    if elapsed <= MOCK_WRITE_END {
        return UpdateProgress {
            phase: UpdatePhase::Writing,
            overall_progress: Some(
                40 + u8::try_from(scaled_value(
                    elapsed,
                    MOCK_VERIFY_END.as_millis(),
                    MOCK_WRITE_END.as_millis(),
                    58,
                ))
                .unwrap_or(58),
            ),
            detail: "Copying image to rootfs.1".into(),
            signature_verified: true,
            ..UpdateProgress::default()
        };
    }

    if elapsed < MOCK_FINALIZE_END {
        return UpdateProgress {
            phase: UpdatePhase::Finalizing,
            detail: "Synchronizing installed system".into(),
            signature_verified: true,
            ..UpdateProgress::default()
        };
    }

    UpdateProgress {
        phase: UpdatePhase::ReadyToReboot,
        detail: "Firmware installed and verified".into(),
        signature_verified: true,
        ..UpdateProgress::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::update::UpdatePhase;

    #[tokio::test]
    async fn server_config_envelope_reports_only_configuration_issues() {
        use crate::server_manager::ConfigState;

        let cases = [
            (MockServerScenario::Unconfigured, ConfigState::Missing, None),
            (
                MockServerScenario::Stopped,
                ConfigState::ValidUnverified,
                None,
            ),
            (
                MockServerScenario::Starting,
                ConfigState::ValidUnverified,
                None,
            ),
            (MockServerScenario::Healthy, ConfigState::Valid, None),
            (
                MockServerScenario::InvalidConfig,
                ConfigState::Invalid,
                Some("config_invalid"),
            ),
            (MockServerScenario::Failed, ConfigState::Valid, None),
            (
                MockServerScenario::RollbackSucceeded,
                ConfigState::Valid,
                None,
            ),
            (MockServerScenario::RollbackFailed, ConfigState::Valid, None),
            (MockServerScenario::Conflict, ConfigState::Valid, None),
        ];

        let mock = MockState::new();
        for (scenario, expected_state, expected_issue) in cases {
            mock.set_server_scenario(scenario).await;
            let envelope = mock.server_config_envelope().await;

            assert_eq!(envelope.state, expected_state, "scenario {scenario:?}");
            assert_eq!(
                envelope.issues.first().map(|issue| issue.code.as_str()),
                expected_issue,
                "scenario {scenario:?}"
            );
            assert_eq!(
                envelope.issues.len(),
                usize::from(expected_issue.is_some()),
                "scenario {scenario:?}"
            );
        }
    }

    #[tokio::test]
    async fn server_diagnostics_projects_truthful_systemd_outcomes() {
        let cases = [
            (
                MockServerScenario::Unconfigured,
                "inactive",
                "dead",
                "success",
                Some(0),
                Some(0),
            ),
            (
                MockServerScenario::Stopped,
                "inactive",
                "dead",
                "success",
                Some(0),
                Some(0),
            ),
            (
                MockServerScenario::Starting,
                "activating",
                "start",
                "",
                None,
                None,
            ),
            (
                MockServerScenario::Healthy,
                "active",
                "running",
                "success",
                Some(0),
                Some(0),
            ),
            (
                MockServerScenario::InvalidConfig,
                "failed",
                "failed",
                "exit-code",
                Some(1),
                Some(78),
            ),
            (
                MockServerScenario::Failed,
                "failed",
                "failed",
                "exit-code",
                Some(1),
                Some(78),
            ),
            (
                MockServerScenario::RollbackSucceeded,
                "active",
                "running",
                "success",
                Some(0),
                Some(0),
            ),
            (
                MockServerScenario::RollbackFailed,
                "failed",
                "failed",
                "exit-code",
                Some(1),
                Some(1),
            ),
            (
                MockServerScenario::Conflict,
                "active",
                "running",
                "success",
                Some(0),
                Some(0),
            ),
        ];

        let mock = MockState::new();
        for (scenario, active, sub, result, code, status) in cases {
            mock.set_server_scenario(scenario).await;
            let diagnostics = mock.server_diagnostics().await;

            assert_eq!(diagnostics.systemd.active_state, active, "{scenario:?}");
            assert_eq!(diagnostics.systemd.sub_state, sub, "{scenario:?}");
            assert_eq!(diagnostics.systemd.result, result, "{scenario:?}");
            assert_eq!(diagnostics.systemd.exec_main_code, code, "{scenario:?}");
            assert_eq!(diagnostics.systemd.exec_main_status, status, "{scenario:?}");
        }
    }

    #[tokio::test]
    async fn conflict_scenario_is_a_rebasable_external_edit() {
        let mock = MockState::new();
        mock.set_server_scenario(MockServerScenario::Healthy).await;
        let baseline_draft = mock.get_server_legacy().await;

        mock.set_server_scenario(MockServerScenario::Conflict).await;
        let fresh = mock.get_server_legacy().await;
        let runtime = mock.server_state().await;
        assert_ne!(baseline_draft.revision, fresh.revision);
        assert_eq!(
            runtime.runtime_state,
            crate::server_manager::RuntimeState::Running
        );
        assert!(runtime.issue.is_none());

        let Err(conflict) = mock
            .apply_server(crate::server_manager::ConfigPayload::Direct(baseline_draft))
            .await
        else {
            panic!("stale mock draft unexpectedly applied");
        };
        assert_eq!(
            conflict.kind,
            crate::server_manager::ManagerErrorKind::Conflict
        );
        let applied = mock
            .apply_server(crate::server_manager::ConfigPayload::Direct(fresh))
            .await
            .unwrap();
        assert_eq!(
            applied.runtime_state,
            crate::server_manager::RuntimeState::Running
        );
    }

    #[test]
    fn update_mock_is_idle_before_an_install_starts() {
        let status = mock_update_progress(None);

        assert_eq!(status.phase, UpdatePhase::Idle);
        assert_eq!(status.phase_progress, None);
        assert_eq!(status.overall_progress, None);
        assert_eq!(status.bytes_done, None);
        assert_eq!(status.bytes_total, None);
        assert!(!status.signature_verified);
    }

    #[test]
    fn update_mock_download_reports_monotonic_real_bytes() {
        let start = mock_update_progress(Some(std::time::Duration::ZERO));
        let middle = mock_update_progress(Some(std::time::Duration::from_secs(2)));
        let complete = mock_update_progress(Some(MOCK_DOWNLOAD_END));

        assert_eq!(start.phase, UpdatePhase::Downloading);
        assert_eq!(start.phase_progress, Some(0));
        assert_eq!(start.bytes_done, Some(0));
        assert_eq!(middle.phase_progress, Some(50));
        assert_eq!(middle.bytes_done, Some(MOCK_BUNDLE_BYTES / 2));
        assert_eq!(complete.phase_progress, Some(100));
        assert_eq!(complete.bytes_done, complete.bytes_total);
        assert!(!complete.signature_verified);
    }

    #[test]
    fn update_mock_preserves_truthful_phase_boundaries() {
        let verifying = mock_update_progress(Some(std::time::Duration::from_millis(4_001)));
        assert_eq!(verifying.phase, UpdatePhase::Verifying);
        assert_eq!(verifying.phase_progress, None);
        assert_eq!(verifying.bytes_done, None);
        assert_eq!(verifying.bytes_total, None);
        assert!(!verifying.signature_verified);

        let write_start = mock_update_progress(Some(MOCK_VERIFY_END));
        let write_middle = mock_update_progress(Some(std::time::Duration::from_secs(8)));
        let write_complete = mock_update_progress(Some(MOCK_WRITE_END));
        assert_eq!(write_start.phase, UpdatePhase::Writing);
        assert_eq!(write_start.phase_progress, None);
        assert_eq!(write_middle.phase_progress, None);
        assert_eq!(write_complete.phase_progress, None);
        assert_eq!(write_start.overall_progress, Some(40));
        assert_eq!(write_middle.overall_progress, Some(69));
        assert_eq!(write_complete.overall_progress, Some(98));
        assert!(write_start.signature_verified);

        let finalizing = mock_update_progress(Some(std::time::Duration::from_millis(10_001)));
        assert_eq!(finalizing.phase, UpdatePhase::Finalizing);
        assert_eq!(finalizing.phase_progress, None);
        assert!(finalizing.signature_verified);

        let ready = mock_update_progress(Some(MOCK_FINALIZE_END));
        assert_eq!(ready.phase, UpdatePhase::ReadyToReboot);
        assert_eq!(ready.phase_progress, None);
        assert_eq!(ready.bytes_done, None);
        assert_eq!(ready.bytes_total, None);
        assert!(ready.last_error.is_empty());
        assert!(ready.signature_verified);
        assert_eq!(
            serde_json::to_value(ready).expect("mock terminal status should serialize"),
            serde_json::json!({
                "phase": "ready_to_reboot",
                "phase_progress": null,
                "overall_progress": null,
                "bytes_done": null,
                "bytes_total": null,
                "detail": "Firmware installed and verified",
                "last_error": "",
                "signature_verified": true,
            })
        );
    }
}
