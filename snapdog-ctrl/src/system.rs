// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! System operations — reads/writes config files, calls systemctl, etc.

use anyhow::{Context, Result};

use crate::routes::{
    AudioInfo, AutoUpdateConfig, ClientConfig, ComponentVersions, DacOverlay, EthernetConfig,
    EthernetInfo, LogsResponse, NetworkOverview, ScanServersResponse, SshConfig, SystemInfo,
    TimezoneInfo, UpdateCheckResponse, WifiInfo, WifiNetwork, WifiScanResult,
};

// --- System ---

pub async fn get_system_info() -> SystemInfo {
    let hostname = read_file("/etc/hostname").await.unwrap_or_default();
    let version = read_file("/etc/snapdog-os.version")
        .await
        .unwrap_or_default();
    let channel = read_file("/etc/snapdog-os.channel")
        .await
        .unwrap_or_else(|_| "stable".into());
    let uptime = get_uptime().await;

    let kernel = tokio::process::Command::new("uname")
        .arg("-r")
        .output()
        .await
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    let client = tokio::process::Command::new("snapdog-client")
        .arg("--version")
        .output()
        .await
        .ok()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .split_whitespace()
                .last()
                .unwrap_or("")
                .to_string()
        })
        .unwrap_or_default();

    SystemInfo {
        hostname: hostname.trim().to_string(),
        version: version.trim().to_string(),
        channel: channel.trim().to_string(),
        uptime_seconds: uptime,
        pi_version: read_file("/etc/raspberrypi.version")
            .await
            .unwrap_or_default()
            .trim()
            .parse()
            .unwrap_or(4),
        components: ComponentVersions {
            server: client.clone(),
            client,
            ctrl: env!("SNAPDOG_CTRL_VERSION").to_string(),
            kernel,
        },
    }
}

pub async fn set_system(hostname: Option<String>, channel: Option<String>) -> Result<()> {
    if let Some(h) = hostname {
        run_cmd("hostnamectl", &["set-hostname", &h]).await?;
    }
    if let Some(c) = channel {
        anyhow::ensure!(
            matches!(c.as_str(), "stable" | "beta"),
            "invalid update channel"
        );
        tokio::fs::write("/etc/snapdog-os.channel", format!("{c}\n"))
            .await
            .context("failed to write snapdog-os.channel")?;
    }
    Ok(())
}

pub async fn reboot() {
    let _ = run_cmd("systemctl", &["reboot"]).await;
}

/// Install a RAUC bundle from a local path or URL.
pub async fn rauc_install(source: &str) -> Result<()> {
    let rauc = crate::rauc::Rauc::connect().await?;
    rauc.install(source).await?;
    Ok(())
}

/// Get RAUC installation progress.
pub async fn rauc_progress() -> Result<crate::rauc::InstallProgress> {
    crate::rauc::Rauc::connect().await?.progress().await
}

/// Get RAUC operation state (idle/installing).
pub async fn rauc_operation() -> Result<String> {
    crate::rauc::Rauc::connect().await?.operation().await
}

/// Get RAUC slot status.
pub async fn rauc_slot_status() -> Result<Vec<crate::rauc::SlotStatus>> {
    crate::rauc::Rauc::connect().await?.slot_status().await
}

// --- Network ---

const ETHERNET_INTERFACES: &[&str] = &["eth0", "end0"];
const WIFI_INTERFACE: &str = "wlan0";
const ETH_NETWORK: &str = "/etc/systemd/network/10-ethernet.network";
const WIFI_NETWORK: &str = "/etc/systemd/network/20-wifi.network";

pub async fn get_network_overview() -> NetworkOverview {
    let (ethernet, wifi) = tokio::join!(get_ethernet(), get_wifi());
    NetworkOverview { ethernet, wifi }
}

pub async fn get_ethernet() -> EthernetInfo {
    let iface = first_existing_interface(ETHERNET_INTERFACES)
        .await
        .unwrap_or_else(|| ETHERNET_INTERFACES[0].to_string());
    let status = interface_status(&iface).await;

    EthernetInfo {
        connected: status.connected,
        mode: read_network_mode(ETH_NETWORK).await,
        ip: status.ip,
        subnet: status.subnet,
        gateway: status.gateway,
        dns: status.dns,
    }
}

pub async fn set_ethernet(config: EthernetConfig) -> Result<()> {
    let static_cfg = if config.mode == "static" {
        Some(crate::network::StaticConfig {
            ip: config.ip.unwrap_or_default(),
            subnet: config.subnet.unwrap_or_else(|| "255.255.255.0".into()),
            gateway: config.gateway.unwrap_or_default(),
            dns: config.dns.unwrap_or_default(),
        })
    } else {
        None
    };
    crate::network::configure_ethernet(static_cfg.as_ref()).await
}

pub async fn get_wifi() -> WifiInfo {
    let status = interface_status(WIFI_INTERFACE).await;
    let wpa = wpa_status(WIFI_INTERFACE).await;
    let signal = wifi_signal(WIFI_INTERFACE).await.unwrap_or_default();
    let connected = wpa.state == "COMPLETED" || status.connected;

    WifiInfo {
        connected,
        ssid: wpa.ssid,
        ip: if status.ip.is_empty() {
            wpa.ip
        } else {
            status.ip
        },
        subnet: status.subnet,
        gateway: status.gateway,
        dns: status.dns,
        signal,
        mode: read_network_mode(WIFI_NETWORK).await,
    }
}

pub async fn set_wifi(
    ssid: &str,
    password: &str,
    static_cfg: Option<&crate::network::StaticConfig>,
) -> Result<()> {
    crate::network::connect_wifi(ssid, password, static_cfg).await
}

pub async fn delete_wifi() -> Result<()> {
    crate::network::disconnect_wifi().await
}

pub async fn wifi_scan() -> WifiScanResult {
    let networks = crate::network::scan_networks().await.unwrap_or_default();
    WifiScanResult {
        networks: networks
            .into_iter()
            .map(|n| WifiNetwork {
                ssid: n.ssid,
                signal: n.signal,
                security: n.security,
            })
            .collect(),
    }
}

#[derive(Default)]
struct InterfaceStatus {
    connected: bool,
    ip: String,
    subnet: String,
    gateway: String,
    dns: String,
}

#[derive(Default)]
struct WpaStatus {
    state: String,
    ssid: String,
    ip: String,
}

async fn first_existing_interface(candidates: &[&str]) -> Option<String> {
    for iface in candidates {
        if tokio::fs::metadata(format!("/sys/class/net/{iface}"))
            .await
            .is_ok()
        {
            return Some((*iface).to_string());
        }
    }
    None
}

async fn interface_status(iface: &str) -> InterfaceStatus {
    let (ip, subnet) = ipv4_address(iface).await.unwrap_or_default();
    let gateway = default_gateway(iface).await.unwrap_or_default();
    let dns = dns_servers(iface).await.unwrap_or_default();
    let connected = interface_is_up(iface).await || !ip.is_empty();

    InterfaceStatus {
        connected,
        ip,
        subnet,
        gateway,
        dns,
    }
}

async fn interface_is_up(iface: &str) -> bool {
    read_file(&format!("/sys/class/net/{iface}/operstate"))
        .await
        .is_ok_and(|state| state.trim() == "up")
}

async fn ipv4_address(iface: &str) -> Result<(String, String)> {
    let output = command_stdout("ip", &["-o", "-4", "addr", "show", "dev", iface]).await?;
    Ok(parse_ipv4_address(&output).unwrap_or_default())
}

fn parse_ipv4_address(output: &str) -> Option<(String, String)> {
    output.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        while let Some(field) = fields.next() {
            if field == "inet" {
                let cidr = fields.next()?;
                let (ip, prefix) = cidr.split_once('/')?;
                let prefix = prefix.parse::<u8>().ok()?;
                return Some((ip.to_string(), prefix_to_subnet(prefix)));
            }
        }
        None
    })
}

async fn default_gateway(iface: &str) -> Result<String> {
    let output = command_stdout("ip", &["-4", "route", "show", "default", "dev", iface]).await?;
    Ok(parse_default_gateway(&output).unwrap_or_default())
}

fn parse_default_gateway(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        while let Some(field) = fields.next() {
            if field == "via" {
                return fields.next().map(ToString::to_string);
            }
        }
        None
    })
}

async fn dns_servers(iface: &str) -> Result<String> {
    if let Ok(output) = command_stdout("resolvectl", &["dns", iface]).await {
        if let Some(servers) = parse_resolvectl_dns(&output) {
            return Ok(servers);
        }
    }

    let resolv_conf = read_file("/etc/resolv.conf").await.unwrap_or_default();
    Ok(parse_resolv_conf_dns(&resolv_conf))
}

fn parse_resolvectl_dns(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let (_label, servers) = line.split_once(':')?;
        let servers = servers.trim();
        if servers.is_empty() {
            None
        } else {
            Some(servers.to_string())
        }
    })
}

fn parse_resolv_conf_dns(output: &str) -> String {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            if fields.next() == Some("nameserver") {
                fields.next()
            } else {
                None
            }
        })
        .filter(|server| !server.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

async fn read_network_mode(path: &str) -> String {
    let content = read_file(path).await.unwrap_or_default();
    parse_network_mode(&content).unwrap_or_else(|| "dhcp".into())
}

fn parse_network_mode(content: &str) -> Option<String> {
    let mut has_static_address = false;

    for line in content.lines().map(str::trim) {
        if line.starts_with('#') {
            continue;
        }
        if matches!(
            line.split_once('='),
            Some(("DHCP", "yes" | "true" | "ipv4" | "both"))
        ) {
            return Some("dhcp".into());
        }
        if line.starts_with("Address=") {
            has_static_address = true;
        }
    }

    has_static_address.then(|| "static".into())
}

async fn wpa_status(iface: &str) -> WpaStatus {
    command_stdout("wpa_cli", &["-i", iface, "status"])
        .await
        .map(|output| parse_wpa_status(&output))
        .unwrap_or_default()
}

fn parse_wpa_status(output: &str) -> WpaStatus {
    let mut status = WpaStatus::default();
    for line in output.lines() {
        match line.split_once('=') {
            Some(("wpa_state", value)) => status.state = value.to_string(),
            Some(("ssid", value)) => status.ssid = value.to_string(),
            Some(("ip_address", value)) => status.ip = value.to_string(),
            _ => {}
        }
    }
    status
}

async fn wifi_signal(iface: &str) -> Result<i32> {
    let output = command_stdout("wpa_cli", &["-i", iface, "signal_poll"]).await?;
    Ok(parse_wifi_signal(&output).unwrap_or_default())
}

fn parse_wifi_signal(output: &str) -> Option<i32> {
    output.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        if matches!(key, "RSSI" | "AVG_RSSI") {
            value.parse().ok()
        } else {
            None
        }
    })
}

fn prefix_to_subnet(prefix: u8) -> String {
    let prefix = prefix.min(32);
    let mask = if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - u32::from(prefix))
    };
    format!(
        "{}.{}.{}.{}",
        (mask >> 24) & 0xff,
        (mask >> 16) & 0xff,
        (mask >> 8) & 0xff,
        mask & 0xff
    )
}

// --- Audio ---

const AVAILABLE_OVERLAYS: &[(&str, &str)] = &[
    ("", "Auto-detect (HAT EEPROM)"),
    ("hifiberry-dacplus", "HiFiBerry DAC+/Amp2/Amp4"),
    ("hifiberry-dacplushd", "HiFiBerry DAC2 HD"),
    ("hifiberry-dacplusdsp", "HiFiBerry DAC+ DSP"),
    ("hifiberry-digi", "HiFiBerry Digi+"),
    ("iqaudio-dacplus", "Raspberry Pi DAC+ / DAC Pro / DigiAMP+"),
    ("iqaudio-codec", "Raspberry Pi Codec Zero"),
    ("justboom-dac", "JustBoom DAC/Amp HAT"),
    ("justboom-digi", "JustBoom Digi HAT"),
    ("allo-boss-dac-pcm512x-audio", "Allo Boss DAC"),
    ("max98357a", "MAX98357A (Adafruit, Google AIY)"),
    ("googlevoicehat-soundcard", "Google AIY Voice HAT"),
    ("vc4-kms-v3d", "HDMI Audio"),
];

/// Detect DAC from HAT EEPROM product string.
async fn detect_hat_overlay() -> Option<&'static str> {
    let product = read_file("/proc/device-tree/hat/product").await.ok()?;
    let product = product.trim();

    Some(match product {
        p if p.contains("DAC2 HD") => "hifiberry-dacplushd",
        p if p.contains("DAC+ DSP") || p.contains("DAC+DSP") => "hifiberry-dacplusdsp",
        p if p.contains("Digi") && p.contains("HiFiBerry") => "hifiberry-digi",
        p if p.contains("HiFiBerry") => "hifiberry-dacplus",
        p if p.contains("DigiAMP") || p.contains("IQaudIO") || p.contains("IQaudio") => {
            "iqaudio-dacplus"
        }
        p if p.contains("Codec Zero") => "iqaudio-codec",
        p if p.contains("JustBoom") && p.contains("Digi") => "justboom-digi",
        p if p.contains("JustBoom") => "justboom-dac",
        _ => return None,
    })
}

pub async fn get_audio() -> AudioInfo {
    let overlay = crate::config_txt::get_audio_overlay()
        .await
        .unwrap_or_default();
    let detected_card = read_file("/proc/asound/card0/id")
        .await
        .unwrap_or_default()
        .trim()
        .to_string();
    let detected_hat = detect_hat_overlay().await;

    AudioInfo {
        overlay,
        detected_card,
        detected_hat: detected_hat.unwrap_or_default().to_string(),
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

pub async fn set_audio_overlay(overlay: &str) -> Result<()> {
    crate::config_txt::set_audio_overlay(overlay).await
}

// --- Client ---

pub async fn get_client() -> ClientConfig {
    let defaults = read_file("/etc/default/snapdog-client")
        .await
        .unwrap_or_default();
    let args = parse_client_args(&defaults);
    let running = run_cmd("systemctl", &["is-active", "snapdog-client"])
        .await
        .is_ok();
    let available_soundcards = list_soundcards().await;

    ClientConfig {
        server_url: args.server_url,
        host_id: args.host_id,
        soundcard: args.soundcard,
        mixer: args.mixer,
        latency: args.latency,
        mdns_name: "_snapdog._tcp".into(),
        running,
        available_soundcards,
    }
}

pub async fn set_client(config: ClientConfig) -> Result<()> {
    let mut args = Vec::new();
    if !config.server_url.is_empty() {
        validate_client_arg("server_url", &config.server_url)?;
        args.push(config.server_url);
    }
    if !config.host_id.is_empty() {
        validate_client_arg("host_id", &config.host_id)?;
        args.push(format!("--hostID {}", config.host_id));
    }
    if config.soundcard != "default" && !config.soundcard.is_empty() {
        validate_client_arg("soundcard", &config.soundcard)?;
        args.push(format!("--soundcard {}", config.soundcard));
    }
    if config.mixer != "software" && !config.mixer.is_empty() {
        validate_client_arg("mixer", &config.mixer)?;
        args.push(format!("--mixer {}", config.mixer));
    }
    if config.latency != 0 {
        args.push(format!("--latency {}", config.latency));
    }

    let content = format!("SNAPDOG_CLIENT_ARGS=\"{}\"\n", args.join(" "));
    tokio::fs::write("/etc/default/snapdog-client", content)
        .await
        .context("failed to write snapdog-client configuration")?;

    run_cmd("systemctl", &["restart", "snapdog-client"]).await?;

    // Sync hostname with host_id
    if !config.host_id.is_empty() {
        let _ = run_cmd("hostnamectl", &["set-hostname", &config.host_id]).await;
    }

    Ok(())
}

// --- SSH ---

pub async fn get_ssh() -> SshConfig {
    let enabled = run_cmd("systemctl", &["is-active", "sshd"]).await.is_ok();
    let keys = read_file("/root/.ssh/authorized_keys")
        .await
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(String::from)
        .collect();

    SshConfig {
        enabled,
        pubkeys: keys,
    }
}

pub async fn set_ssh(config: SshConfig) -> Result<()> {
    set_service("ssh", config.enabled).await?;

    tokio::fs::create_dir_all("/root/.ssh").await?;
    let keys = config.pubkeys.join("\n") + "\n";
    tokio::fs::write("/root/.ssh/authorized_keys", keys).await?;
    run_cmd("chmod", &["600", "/root/.ssh/authorized_keys"]).await?;
    Ok(())
}

// --- OTA Update (RAUC) ---

const UPDATE_BASE_URL: &str = "https://update.snapdog.cc/os/bundles";

/// Construct the bundle URL for a given channel.
pub fn bundle_url(channel: &str) -> String {
    let board = detect_board();
    let suffix = if channel == "stable" { "" } else { "-beta" };
    format!("{UPDATE_BASE_URL}/{board}{suffix}.raucb")
}

pub fn detect_board() -> &'static str {
    let content = std::fs::read_to_string("/etc/rauc/system.conf").unwrap_or_default();
    if content.contains("pi5") {
        "pi5"
    } else if content.contains("pi3") {
        "pi3"
    } else {
        "pi4"
    }
}

pub async fn check_update() -> UpdateCheckResponse {
    let current = read_file("/etc/snapdog-os.version")
        .await
        .unwrap_or_default()
        .trim()
        .to_string();
    let config = get_auto_update().await;
    let url = bundle_url(&config.channel);

    // Check if bundle URL is reachable (HEAD request)
    let available = tokio::process::Command::new("curl")
        .args(["-sfI", "--max-time", "5", &url])
        .output()
        .await
        .is_ok_and(|o| o.status.success());

    UpdateCheckResponse {
        available,
        current_version: if current.is_empty() {
            "unknown".into()
        } else {
            current
        },
        channel: config.channel,
        bundle_url: url,
    }
}

// --- Factory Reset ---

pub async fn factory_reset() -> Result<()> {
    tracing::warn!("Factory reset initiated");

    // Remove configurations directly from the writeable /data partition to preserve symbolic links
    let _ = tokio::fs::remove_file("/data/wpa_supplicant/wpa_supplicant-wlan0.conf").await;
    let _ = tokio::fs::remove_file("/data/systemd/network/10-ethernet.network").await;
    let _ = tokio::fs::remove_file("/data/systemd/network/20-wifi.network").await;
    let _ = tokio::fs::remove_file("/data/default/snapdog-client").await;
    let _ = tokio::fs::remove_file("/data/hostname").await;
    let _ = tokio::fs::remove_file("/data/snapdog-os.channel").await;
    let _ = tokio::fs::remove_file("/data/snapdog-os.auto-update").await;
    let _ = tokio::fs::remove_file("/data/snapdog/snapdog.toml").await;

    // Disable SSH and remove authorized keys
    let _ = set_service("ssh", false).await;
    let _ = tokio::fs::remove_file("/data/ssh/authorized_keys").await;

    // Reset hostname immediately
    let _ = run_cmd("hostnamectl", &["set-hostname", "snapdog"]).await;

    // Run snapdog-data-init script if on Linux to re-populate standard defaults immediately
    #[cfg(target_os = "linux")]
    {
        let _ = run_cmd("/usr/bin/snapdog-data-init", &[]).await;
    }

    // Reboot
    tracing::info!("Factory reset complete, rebooting");
    run_cmd("systemctl", &["reboot"]).await?;
    Ok(())
}

// --- Logs ---

pub async fn get_logs(service: Option<String>) -> LogsResponse {
    let mut args = vec!["--no-pager", "-n", "200", "--output", "short-iso"];

    match service.as_deref() {
        Some("server") => {
            args.push("-u");
            args.push("snapdog");
        }
        Some("client") => {
            args.push("-u");
            args.push("snapdog-client");
        }
        Some("controller") => {
            args.push("-u");
            args.push("snapdog-ctrl");
        }
        _ => {
            args.push("-u");
            args.push("snapdog");
            args.push("-u");
            args.push("snapdog-client");
            args.push("-u");
            args.push("snapdog-ctrl");
        }
    }

    let output = tokio::process::Command::new("journalctl")
        .args(&args)
        .output()
        .await
        .ok();

    let lines = output
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();

    LogsResponse { lines }
}

// --- Timezone ---

pub async fn get_timezone() -> TimezoneInfo {
    let current = tokio::process::Command::new("timedatectl")
        .args(["show", "--property=Timezone", "--value"])
        .output()
        .await
        .ok()
        .map_or_else(
            || "UTC".into(),
            |o| String::from_utf8_lossy(&o.stdout).trim().to_string(),
        );

    let available = tokio::process::Command::new("timedatectl")
        .args(["list-timezones"])
        .output()
        .await
        .ok()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();

    TimezoneInfo {
        timezone: current,
        available,
    }
}

pub async fn set_timezone(tz: &str) -> Result<()> {
    run_cmd("timedatectl", &["set-timezone", tz]).await
}

// --- Soundcards ---

pub async fn list_soundcards() -> Vec<String> {
    let output = tokio::process::Command::new("aplay")
        .args(["-l"])
        .output()
        .await
        .ok();

    output
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| l.starts_with("card "))
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

// --- Auto-Update Settings ---

const CTRL_CONFIG: &str = "/data/snapdog/ctrl.toml";

pub async fn get_auto_update() -> AutoUpdateConfig {
    let content = read_file(CTRL_CONFIG).await.unwrap_or_default();
    let doc: toml_edit::DocumentMut = content.parse().unwrap_or_default();
    let au = doc.get("auto-update");
    AutoUpdateConfig {
        enabled: au
            .and_then(|t| t.get("enabled"))
            .and_then(toml_edit::Item::as_bool)
            .unwrap_or(true),
        channel: au
            .and_then(|t| t.get("channel"))
            .and_then(|v| v.as_str())
            .unwrap_or("stable")
            .to_string(),
        interval: au
            .and_then(|t| t.get("interval"))
            .and_then(|v| v.as_str())
            .unwrap_or("daily")
            .to_string(),
        time: au
            .and_then(|t| t.get("time"))
            .and_then(|v| v.as_str())
            .unwrap_or("04:00")
            .to_string(),
    }
}

pub async fn set_auto_update(config: AutoUpdateConfig) -> Result<()> {
    let content = read_file(CTRL_CONFIG).await.unwrap_or_default();
    let mut doc: toml_edit::DocumentMut = content.parse().unwrap_or_default();

    let au = doc
        .entry("auto-update")
        .or_insert_with(|| toml_edit::Item::Table(toml_edit::Table::new()));
    au["enabled"] = toml_edit::value(config.enabled);
    au["channel"] = toml_edit::value(&config.channel);
    au["interval"] = toml_edit::value(&config.interval);
    au["time"] = toml_edit::value(&config.time);

    if let Some(parent) = std::path::Path::new(CTRL_CONFIG).parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(CTRL_CONFIG, doc.to_string()).await?;
    Ok(())
}

// --- SoftAP Settings ---

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct SoftApConfig {
    pub enabled: bool,
    pub password: String,
}

pub async fn get_softap_config() -> SoftApConfig {
    let content = read_file(CTRL_CONFIG).await.unwrap_or_default();
    let doc: toml_edit::DocumentMut = content.parse().unwrap_or_default();
    let ap = doc.get("softap");
    SoftApConfig {
        enabled: ap
            .and_then(|t| t.get("enabled"))
            .and_then(toml_edit::Item::as_bool)
            .unwrap_or(true),
        password: ap
            .and_then(|t| t.get("password"))
            .and_then(|v| v.as_str())
            .unwrap_or("snapdog123")
            .to_string(),
    }
}

pub async fn set_softap_config(config: SoftApConfig) -> Result<()> {
    let content = read_file(CTRL_CONFIG).await.unwrap_or_default();
    let mut doc: toml_edit::DocumentMut = content.parse().unwrap_or_default();

    let ap = doc
        .entry("softap")
        .or_insert_with(|| toml_edit::Item::Table(toml_edit::Table::new()));
    ap["enabled"] = toml_edit::value(config.enabled);
    ap["password"] = toml_edit::value(&config.password);

    if let Some(parent) = std::path::Path::new(CTRL_CONFIG).parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(CTRL_CONFIG, doc.to_string()).await?;
    Ok(())
}

// --- Service Management ---
// snapdog-ctrl is the sole manager of optional services.
// Services are NOT enabled in systemd — snapdog-ctrl starts them at boot based on config.

const SERVICE_MAP: &[(&str, &str)] = &[
    ("ssh", "sshd.service"),
    ("client", "snapdog-client.service"),
    ("server", "snapdog.service"),
];

/// Read service states from ctrl.toml, apply defaults if missing.
pub async fn get_service_config() -> std::collections::HashMap<String, bool> {
    let content = read_file(CTRL_CONFIG).await.unwrap_or_default();
    let doc: toml_edit::DocumentMut = content.parse().unwrap_or_default();
    let svc = doc.get("services");

    let mut map = std::collections::HashMap::new();
    map.insert(
        "ssh".into(),
        svc.and_then(|t| t.get("ssh"))
            .and_then(toml_edit::Item::as_bool)
            .unwrap_or(false),
    );
    map.insert(
        "client".into(),
        svc.and_then(|t| t.get("client"))
            .and_then(toml_edit::Item::as_bool)
            .unwrap_or(true),
    );
    map.insert(
        "server".into(),
        svc.and_then(|t| t.get("server"))
            .and_then(toml_edit::Item::as_bool)
            .unwrap_or(false),
    );
    map
}

/// Apply service states: start enabled services, stop disabled ones.
pub async fn apply_service_config() {
    let config = get_service_config().await;
    for (key, unit) in SERVICE_MAP {
        let enabled = config.get(*key).copied().unwrap_or(false);
        if enabled {
            let _ = run_cmd("systemctl", &["unmask", unit]).await;
            let _ = run_cmd("systemctl", &["start", unit]).await;
        } else {
            let _ = run_cmd("systemctl", &["stop", unit]).await;
        }
    }
}

/// Set a service enabled/disabled and start/stop it.
pub async fn set_service(name: &str, enabled: bool) -> Result<()> {
    let unit = SERVICE_MAP
        .iter()
        .find(|(k, _)| *k == name)
        .map(|(_, v)| *v)
        .ok_or_else(|| anyhow::anyhow!("unknown service: {name}"))?;

    let content = read_file(CTRL_CONFIG).await.unwrap_or_default();
    let mut doc: toml_edit::DocumentMut = content.parse().unwrap_or_default();
    let svc = doc
        .entry("services")
        .or_insert_with(|| toml_edit::Item::Table(toml_edit::Table::new()));
    svc[name] = toml_edit::value(enabled);

    if let Some(parent) = std::path::Path::new(CTRL_CONFIG).parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(CTRL_CONFIG, doc.to_string()).await?;

    if enabled {
        run_cmd("systemctl", &["unmask", unit]).await?;
        run_cmd("systemctl", &["start", unit]).await?;
    } else {
        run_cmd("systemctl", &["stop", unit]).await?;
    }
    Ok(())
}

// --- Server Connectivity Test ---

pub async fn test_server(host: &str, port: u16) -> bool {
    let addr = format!("{host}:{port}");
    tokio::time::timeout(
        std::time::Duration::from_secs(3),
        tokio::net::TcpStream::connect(&addr),
    )
    .await
    .is_ok_and(|r| r.is_ok())
}

// --- mDNS Server Discovery ---

pub async fn scan_servers() -> ScanServersResponse {
    ScanServersResponse {
        servers: crate::mdns::browse_servers().await,
    }
}

// --- Helpers ---

async fn read_file(path: &str) -> Result<String> {
    Ok(tokio::fs::read_to_string(path).await?)
}

async fn command_stdout(cmd: &str, args: &[&str]) -> Result<String> {
    let output = tokio::process::Command::new(cmd)
        .args(args)
        .output()
        .await?;
    if !output.status.success() {
        anyhow::bail!(
            "{} {} failed: {}",
            cmd,
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

async fn run_cmd(cmd: &str, args: &[&str]) -> Result<()> {
    let output = tokio::process::Command::new(cmd)
        .args(args)
        .output()
        .await?;
    if !output.status.success() {
        anyhow::bail!(
            "{} {} failed: {}",
            cmd,
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

async fn get_uptime() -> u64 {
    read_file("/proc/uptime")
        .await
        .unwrap_or_default()
        .split_whitespace()
        .next()
        .unwrap_or("")
        .split('.')
        .next()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0)
}

struct ParsedClientArgs {
    server_url: String,
    host_id: String,
    soundcard: String,
    mixer: String,
    latency: i32,
}

fn parse_client_args(defaults_file: &str) -> ParsedClientArgs {
    let args_line = defaults_file
        .lines()
        .find(|l| l.starts_with("SNAPDOG_CLIENT_ARGS="))
        .unwrap_or("")
        .trim_start_matches("SNAPDOG_CLIENT_ARGS=")
        .trim_matches('"');

    let parts: Vec<&str> = args_line.split_whitespace().collect();
    let mut result = ParsedClientArgs {
        server_url: String::new(),
        host_id: String::new(),
        soundcard: "default".into(),
        mixer: "software".into(),
        latency: 0,
    };

    let mut i = 0;
    while i < parts.len() {
        match parts[i] {
            "--hostID" if i + 1 < parts.len() => {
                result.host_id = parts[i + 1].to_string();
                i += 2;
            }
            "--soundcard" if i + 1 < parts.len() => {
                result.soundcard = parts[i + 1].to_string();
                i += 2;
            }
            "--mixer" if i + 1 < parts.len() => {
                result.mixer = parts[i + 1].to_string();
                i += 2;
            }
            "--latency" if i + 1 < parts.len() => {
                result.latency = parts[i + 1].parse().unwrap_or(0);
                i += 2;
            }
            s if !s.starts_with('-') && result.server_url.is_empty() => {
                result.server_url = s.to_string();
                i += 1;
            }
            _ => i += 1,
        }
    }

    result
}

fn validate_client_arg(field: &str, value: &str) -> Result<()> {
    anyhow::ensure!(
        value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':' | '/')),
        "{field} contains unsupported characters"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_client_arg_rejects_environment_file_breakout() {
        assert!(validate_client_arg("host_id", "living room").is_err());
        assert!(validate_client_arg("host_id", "living\"room").is_err());
        assert!(validate_client_arg("host_id", "living\nroom").is_err());
    }

    #[test]
    fn validate_client_arg_accepts_common_values() {
        assert!(validate_client_arg("server_url", "tcp://192.168.1.10:1704").is_ok());
        assert!(validate_client_arg("host_id", "kitchen").is_ok());
        assert!(validate_client_arg("soundcard", "hw:0").is_ok());
    }

    #[test]
    fn parses_ipv4_address_and_prefix() {
        let output = "2: eth0    inet 192.168.1.42/24 brd 192.168.1.255 scope global eth0";
        assert_eq!(
            parse_ipv4_address(output),
            Some(("192.168.1.42".into(), "255.255.255.0".into()))
        );
    }

    #[test]
    fn parses_network_mode() {
        assert_eq!(
            parse_network_mode("[Network]\nDHCP=yes\n"),
            Some("dhcp".into())
        );
        assert_eq!(
            parse_network_mode("[Network]\nAddress=10.0.0.2/24\n"),
            Some("static".into())
        );
    }

    #[test]
    fn parses_wifi_status_and_signal() {
        let status = parse_wpa_status("wpa_state=COMPLETED\nssid=Studio\nip_address=10.0.0.3\n");
        assert_eq!(status.state, "COMPLETED");
        assert_eq!(status.ssid, "Studio");
        assert_eq!(status.ip, "10.0.0.3");
        assert_eq!(parse_wifi_signal("RSSI=-51\nLINKSPEED=72\n"), Some(-51));
    }

    #[test]
    fn parses_dns_and_gateway() {
        assert_eq!(
            parse_default_gateway("default via 192.168.1.1 dev eth0 proto dhcp"),
            Some("192.168.1.1".into())
        );
        assert_eq!(
            parse_resolvectl_dns("Link 2 (eth0): 1.1.1.1 8.8.8.8"),
            Some("1.1.1.1 8.8.8.8".into())
        );
        assert_eq!(
            parse_resolv_conf_dns("nameserver 9.9.9.9\nnameserver 149.112.112.112\n"),
            "9.9.9.9 149.112.112.112"
        );
    }
}
