// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! Mock system backend for local development. Only available in debug builds.

use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Mutex;

use crate::routes::{
    AudioInfo, ClientConfig, DacOverlay, EthernetConfig, EthernetInfo, NetworkOverview, SshConfig,
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
}

impl MockState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(State {
                hostname: "snapdog-dev".into(),
                channel: "stable".into(),
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
                    available_soundcards: vec!["card 0: sndrpiallo [snd_rpi_allo_boss]".into()],
                },
                ssh: SshConfig {
                    enabled: false,
                    pubkeys: vec![],
                },
            })),
        }
    }

    pub async fn get_system_info(&self) -> SystemInfo {
        let s = self.inner.lock().await;
        SystemInfo {
            hostname: s.hostname.clone(),
            version: "0.1.0".into(),
            channel: s.channel.clone(),
            uptime_seconds: 86400,
            pi_version: 4,
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
        let s = self.inner.lock().await;
        let channel = s.channel.clone();
        drop(s);
        tracing::info!("[mock] OTA update triggered for {channel} (no-op)");
        Ok(())
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
        }
    }

    pub async fn get_audio(&self) -> AudioInfo {
        let s = self.inner.lock().await;
        AudioInfo {
            overlay: s.overlay.clone(),
            detected_card: "Mock Allo Boss DAC".into(),
            soundcard: "hw:0".into(),
            available_overlays: AVAILABLE_OVERLAYS
                .iter()
                .map(|(id, name)| DacOverlay {
                    id: (*id).into(),
                    name: (*name).into(),
                })
                .collect(),
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
        s.client.available_soundcards = vec!["card 0: sndrpiallo [snd_rpi_allo_boss]".into()];
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

const AVAILABLE_OVERLAYS: &[(&str, &str)] = &[
    ("allo-boss-dac-pcm512x-audio", "Allo Boss DAC"),
    ("iqaudio-dacplus", "IQAudio DAC+"),
    ("justboom-dac", "JustBoom DAC"),
    ("max98357a", "MAX98357A (Adafruit, Google AIY)"),
    ("", "Auto-detect (HAT EEPROM)"),
];
