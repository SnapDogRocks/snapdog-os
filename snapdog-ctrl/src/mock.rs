// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! Mock system backend for local development. Only available in debug builds.

use std::sync::Arc;

use anyhow::Result;
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
