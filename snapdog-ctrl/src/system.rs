// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! System operations — reads/writes config files, calls systemctl, etc.

use anyhow::{Context, Result};

use crate::routes::{
    AudioInfo, AutoUpdateConfig, ClientConfig, ComponentVersions, DacOverlay, EthernetConfig,
    EthernetInfo, LogsResponse, NetworkOverview, ScanServersResponse, Soundcard, SshConfig,
    SystemInfo, TimezoneInfo, UpdateCheckResponse, WifiInfo, WifiNetwork, WifiScanResult,
};

// --- Health Check ---

#[derive(serde::Serialize, Clone)]
pub struct HealthWarning {
    pub id: &'static str,
    pub severity: &'static str,
}

/// Returns warnings (including critical ones). Never panics.
pub async fn preflight_check() -> Vec<HealthWarning> {
    let mut warnings = Vec::new();

    // Critical: /data must be mounted and writable
    let data_mounted = tokio::fs::metadata("/data").await.is_ok();
    if data_mounted {
        let test_file = "/data/.health-check";
        let writable = tokio::fs::write(test_file, "ok").await.is_ok();
        let _ = tokio::fs::remove_file(test_file).await;
        if !writable {
            tracing::error!("/data is not writable — configuration will not persist");
            warnings.push(HealthWarning {
                id: "data_not_writable",
                severity: "critical",
            });
        }
    } else {
        tracing::error!("/data is not mounted — configuration will not persist");
        warnings.push(HealthWarning {
            id: "data_not_mounted",
            severity: "critical",
        });
    }

    if tokio::fs::metadata("/boot").await.is_err() {
        warnings.push(HealthWarning {
            id: "boot_not_mounted",
            severity: "warn",
        });
    }

    let wifi_iface = crate::network::detect_wifi_interface().await;
    if tokio::fs::metadata(format!("/sys/class/net/{wifi_iface}"))
        .await
        .is_err()
    {
        warnings.push(HealthWarning {
            id: "no_wlan",
            severity: "info",
        });
    }

    // Check inactive partition exists
    if inactive_root_partition_exists().await.is_err() {
        warnings.push(HealthWarning {
            id: "no_inactive_partition",
            severity: "warn",
        });
    }

    for w in &warnings {
        tracing::warn!("health: [{}] {}", w.severity, w.id);
    }

    warnings
}

async fn inactive_root_partition_exists() -> Result<()> {
    let target = inactive_root_partition().await?;
    tokio::fs::metadata(&target).await?;
    Ok(())
}

// --- System ---

pub async fn get_system_info() -> SystemInfo {
    let hostname = read_file("/etc/hostname").await.unwrap_or_default();
    let version = read_file("/etc/snapdog-os.version")
        .await
        .unwrap_or_default();
    let channel = read_file("/etc/snapdog-os.channel")
        .await
        .unwrap_or_else(|_| "release".into());
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
        board_model: detect_board_model().await,
        components: ComponentVersions {
            server: client.clone(),
            client,
            ctrl: env!("SNAPDOG_CTRL_VERSION").to_string(),
            kernel,
        },
    }
}

pub async fn detect_board_model() -> String {
    if let Ok(model) = tokio::fs::read_to_string("/proc/device-tree/model").await {
        let trimmed = model.trim_end_matches('\0').trim().to_string();
        if !trimmed.is_empty() {
            return trimmed;
        }
    }
    // Fallback to RAUC compatible
    detect_board().await
}

pub async fn set_system(hostname: Option<String>, channel: Option<String>) -> Result<()> {
    if let Some(h) = hostname {
        run_cmd("hostnamectl", &["set-hostname", &h]).await?;
    }
    if let Some(c) = channel {
        anyhow::ensure!(
            matches!(c.as_str(), "release" | "beta"),
            "invalid update channel"
        );
        tokio::fs::write("/etc/snapdog-os.channel", format!("{c}\n"))
            .await
            .context("failed to write snapdog-os.channel")?;
    }
    Ok(())
}

pub async fn reboot() {
    // If a RAUC tryboot trial is armed (a bundle was just installed to the
    // inactive slot), boot it via the RPi one-shot tryboot flag so a failed trial
    // auto-reverts to the committed slot on the next normal boot. systemd on this
    // image cannot set the tryboot flag, so use the RESTART2 helper (runs with the
    // ctrl's CAP_SYS_BOOT). Otherwise a plain reboot, which always lands on the
    // committed slot.
    if tokio::fs::metadata("/boot/tryboot.txt").await.is_ok() {
        tracing::info!("Tryboot trial armed — rebooting into it via the one-shot tryboot flag");
        // Record the version we are trialing so the next boot's reconcile can
        // confirm it (or mark it bad on a rollback). This makes WebUI-triggered
        // installs covered by rollback tracking + the known-bad reinstall guard,
        // exactly like the auto-updater — recorded here (only when a trial is
        // actually armed) so it covers both the online and manual-upload paths.
        if let Some(v) = pending_trial_version().await {
            record_pending_update(&v).await;
        }
        // Reboots immediately via RESTART2 (ctrl carries CAP_SYS_BOOT). If it
        // returns (e.g. missing capability), we fall through to a normal reboot.
        let _ = run_cmd("/usr/lib/rauc/tryboot-reboot", &[]).await;
    }
    let _ = run_cmd("systemctl", &["reboot"]).await;
}

/// Bundle version of the inactive rootfs slot — the one a tryboot trial boots
/// next. Used to record the pending-update marker at reboot time.
async fn pending_trial_version() -> Option<String> {
    let slots = crate::rauc::Rauc::connect()
        .await
        .ok()?
        .slot_status()
        .await
        .ok()?;
    slots
        .into_iter()
        .find(|s| !s.booted && s.class == "rootfs" && !s.version.is_empty())
        .map(|s| s.version)
}

/// Install a RAUC bundle from a local path or URL.
pub async fn rauc_install(source: &str) -> Result<()> {
    let rauc = crate::rauc::Rauc::connect().await?;
    rauc.install(source).await?;
    Ok(())
}

/// Flash a raw .img.gz to the inactive root partition (escape hatch, bypasses RAUC).
pub async fn flash_raw_image(image_path: &str) -> Result<()> {
    let target = inactive_root_partition().await?;

    tracing::warn!("Raw flash: writing {image_path} to {target}");

    let status = tokio::process::Command::new("sh")
        .args([
            "-c",
            &format!("gzip -dc '{image_path}' | dd of={target} bs=4M conv=fsync status=none"),
        ])
        .status()
        .await?;

    anyhow::ensure!(status.success(), "dd failed with exit code {status}");

    let _ = tokio::fs::remove_file(image_path).await;
    tracing::info!("Raw flash complete. Reboot required.");
    Ok(())
}

/// Determine the inactive root partition from the active one in /proc/cmdline.
/// Supports mmcblk (SD/eMMC) and nvme devices.
async fn inactive_root_partition() -> Result<String> {
    let cmdline = read_file("/proc/cmdline").await.unwrap_or_default();
    let root = cmdline
        .split_whitespace()
        .find(|s| s.starts_with("root="))
        .map(|s| s.trim_start_matches("root="))
        .ok_or_else(|| anyhow::anyhow!("cannot find root= in /proc/cmdline"))?;

    // Swap partition 2 <-> 3 (A/B)
    if let Some(base) = root.strip_suffix('2') {
        Ok(format!("{base}3"))
    } else if let Some(base) = root.strip_suffix('3') {
        Ok(format!("{base}2"))
    } else {
        anyhow::bail!("unexpected root partition: {root} (expected p2 or p3)")
    }
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

const ETH_NETWORK: &str = "/etc/systemd/network/10-ethernet.network";
const WIFI_NETWORK: &str = "/etc/systemd/network/20-wifi.network";

pub async fn get_network_overview() -> NetworkOverview {
    let (ethernet, wifi) = tokio::join!(get_ethernet(), get_wifi());
    NetworkOverview { ethernet, wifi }
}

pub async fn get_ethernet() -> EthernetInfo {
    let eths = crate::network::detect_ethernet_interfaces().await;
    let iface = eths.first().cloned().unwrap_or_else(|| "eth0".to_string());
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
    let iface = crate::network::detect_wifi_interface().await;
    let status = interface_status(&iface).await;
    let wpa = wpa_status(&iface).await;
    let signal = wifi_signal(&iface).await.unwrap_or_default();
    let connected = wpa.state == "COMPLETED" || status.connected;
    let ip = if status.ip.is_empty() {
        wpa.ip.clone()
    } else {
        status.ip.clone()
    };
    let state = derive_wifi_state(&iface, &wpa, &ip).await;

    WifiInfo {
        connected,
        ssid: wpa.ssid,
        ip,
        subnet: status.subnet,
        gateway: status.gateway,
        dns: status.dns,
        signal,
        mode: read_network_mode(WIFI_NETWORK).await,
        state,
    }
}

/// Pure mapping from `wpa_supplicant`'s `wpa_state` + IP presence + the network's
/// TEMP-DISABLED flag into the UI-facing lifecycle. Extracted so the ordering is
/// unit-testable — in particular that a TEMP-DISABLED (wrong-passphrase) network
/// wins over the in-flight retry states.
fn classify_wifi_state(wpa_state: &str, has_ip: bool, temp_disabled: bool) -> &'static str {
    // A COMPLETED association is authoritative.
    if wpa_state == "COMPLETED" {
        return if has_ip { "connected" } else { "acquiring_ip" };
    }
    // A wrong passphrase durably TEMP-DISABLES the network, but wpa_supplicant then
    // keeps RETRYING — cycling back through SCANNING/ASSOCIATING with backoff. So
    // this must be checked BEFORE the retry states below, otherwise the failure
    // reads as a perpetual "associating" and the UI hangs on "connecting…" until it
    // times out instead of showing "wrong password".
    if temp_disabled {
        return "auth_failed";
    }
    match wpa_state {
        "SCANNING" | "AUTHENTICATING" | "ASSOCIATING" | "ASSOCIATED" | "4WAY_HANDSHAKE"
        | "GROUP_HANDSHAKE" => "associating",
        _ => "disconnected",
    }
}

/// Map `wpa_supplicant`'s `wpa_state` + IP into the UI-facing lifecycle. A
/// TEMP-DISABLED network almost always means a wrong passphrase, which is the
/// single most useful failure to surface.
async fn derive_wifi_state(iface: &str, wpa: &WpaStatus, ip: &str) -> String {
    // Only query the extra TEMP-DISABLED flag when not already connected.
    let temp_disabled = wpa.state != "COMPLETED" && wpa_network_temp_disabled(iface).await;
    classify_wifi_state(&wpa.state, !ip.is_empty(), temp_disabled).to_string()
}

/// True when a configured network is in the `[TEMP-DISABLED]` state — the
/// supplicant's signal for repeated auth/handshake failure (wrong PSK).
async fn wpa_network_temp_disabled(iface: &str) -> bool {
    command_stdout("wpa_cli", &["-i", iface, "list_networks"])
        .await
        .is_ok_and(|o| o.contains("TEMP-DISABLED"))
}

pub async fn set_wifi(
    ssid: &str,
    password: &str,
    static_cfg: Option<&crate::network::StaticConfig>,
) -> Result<()> {
    let country = get_softap_config().await.country;
    crate::network::connect_wifi(ssid, password, &country, static_cfg).await
}

pub async fn delete_wifi() -> Result<()> {
    crate::network::disconnect_wifi().await
}

pub async fn wifi_scan() -> WifiScanResult {
    let ap_active = crate::network::is_ap_active().await;
    match crate::network::scan_networks().await {
        Ok(networks) => WifiScanResult {
            networks: networks
                .into_iter()
                .map(|n| WifiNetwork {
                    ssid: n.ssid,
                    signal: n.signal,
                    security: n.security,
                })
                .collect(),
            status: "ok".into(),
            ap_active,
        },
        Err(e) => {
            tracing::warn!("WiFi scan failed: {e:#}");
            WifiScanResult {
                networks: Vec::new(),
                status: if ap_active {
                    "unavailable_ap_mode"
                } else {
                    "error"
                }
                .into(),
                ap_active,
            }
        }
    }
}

/// True when the device has a working way in besides the setup AP: `WiFi`
/// associated, or an ethernet interface with an IPv4 address. Used to guard
/// against disabling the setup AP into a permanent lockout.
pub async fn has_connectivity() -> bool {
    let wifi = get_wifi().await;
    if wifi.connected && !wifi.ip.is_empty() {
        return true;
    }
    for iface in crate::network::detect_ethernet_interfaces().await {
        let (ip, _) = ipv4_address(&iface).await.unwrap_or_default();
        if !ip.is_empty() {
            return true;
        }
    }
    false
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

/// One HAT in the catalog — the single source of truth for both the manual
/// overlay dropdown and EEPROM auto-detection. Adding a board is one row here.
struct HatEntry {
    /// dtoverlay id written to `/boot/config.txt` (empty = the "auto-detect" row).
    id: &'static str,
    /// Human-readable label shown in the dropdown.
    label: &'static str,
    /// Required HAT `vendor`/`product` substring, or `None` for any. Guards
    /// generic model names (e.g. "Digi", shared by `HiFiBerry` and `JustBoom`).
    vendor: Option<&'static str>,
    /// `product`-atom substrings that auto-select this overlay (any match; rows
    /// are tried top-to-bottom, so specific models must precede catch-alls).
    /// Empty = manual-only: shown in the dropdown but never auto-detected.
    eeprom: &'static [&'static str],
}

/// HAT catalog + EEPROM detection table (the SSOT). Product strings mirror
/// `HiFiBerry`'s own detect-hifiberry (bare model names; the brand is often only
/// in `vendor`). Only overlays present in the mainline Raspberry Pi kernel are
/// used — there is no hifiberry-amp4, so a plain Amp4 (like Amp2/DAC+) uses dacplus.
const HATS: &[HatEntry] = &[
    HatEntry {
        id: "",
        label: "Auto-detect (HAT EEPROM)",
        vendor: None,
        eeprom: &[],
    },
    // HiFiBerry — specific models first, DAC+ catch-all last.
    HatEntry {
        id: "hifiberry-dacplushd",
        label: "HiFiBerry DAC2 HD",
        vendor: None,
        eeprom: &["DAC 2 HD", "DAC2 HD"],
    },
    HatEntry {
        id: "hifiberry-dacplusadcpro",
        label: "HiFiBerry DAC+ ADC Pro",
        vendor: None,
        eeprom: &["DAC+ ADC Pro"],
    },
    HatEntry {
        id: "hifiberry-dacplusadc",
        label: "HiFiBerry DAC+ ADC",
        vendor: None,
        eeprom: &["DAC+ ADC"],
    },
    HatEntry {
        id: "hifiberry-dacplusdsp",
        label: "HiFiBerry DAC+ DSP",
        vendor: None,
        eeprom: &["DAC+ DSP", "DAC+DSP"],
    },
    HatEntry {
        id: "hifiberry-digi-pro",
        label: "HiFiBerry Digi+ Pro/Digi2 Pro",
        vendor: Some("HiFiBerry"),
        eeprom: &["Digi2", "Digi+ Pro"],
    },
    HatEntry {
        id: "hifiberry-digi",
        label: "HiFiBerry Digi+",
        vendor: Some("HiFiBerry"),
        eeprom: &["Digi"],
    },
    HatEntry {
        id: "hifiberry-amp100",
        label: "HiFiBerry Amp100",
        vendor: Some("HiFiBerry"),
        eeprom: &["Amp100"],
    },
    HatEntry {
        id: "hifiberry-amp4pro",
        label: "HiFiBerry Amp4 Pro",
        vendor: Some("HiFiBerry"),
        eeprom: &["Amp4 Pro", "Amp4Pro"],
    },
    HatEntry {
        id: "hifiberry-amp3",
        label: "HiFiBerry Amp3",
        vendor: Some("HiFiBerry"),
        eeprom: &["Amp3"],
    },
    HatEntry {
        id: "hifiberry-dacplus",
        label: "HiFiBerry DAC+/Amp2/Amp4",
        vendor: Some("HiFiBerry"),
        eeprom: &[""],
    },
    // IQaudio / Raspberry Pi — Codec Zero before the generic DAC+/DigiAMP+ match.
    HatEntry {
        id: "iqaudio-codec",
        label: "Raspberry Pi Codec Zero",
        vendor: None,
        eeprom: &["Codec Zero"],
    },
    HatEntry {
        id: "iqaudio-dacplus",
        label: "Raspberry Pi DAC+ / DAC Pro / DigiAMP+",
        vendor: None,
        eeprom: &["DigiAMP", "IQaudIO", "IQaudio"],
    },
    // JustBoom — Digi before the generic DAC/Amp match.
    HatEntry {
        id: "justboom-digi",
        label: "JustBoom Digi HAT",
        vendor: None,
        eeprom: &["JustBoom Digi"],
    },
    HatEntry {
        id: "justboom-dac",
        label: "JustBoom DAC/Amp HAT",
        vendor: None,
        eeprom: &["JustBoom"],
    },
    // Manual-only — no reliable EEPROM identification.
    HatEntry {
        id: "allo-boss-dac-pcm512x-audio",
        label: "Allo Boss DAC",
        vendor: None,
        eeprom: &[],
    },
    HatEntry {
        id: "max98357a",
        label: "MAX98357A (Adafruit, Google AIY)",
        vendor: None,
        eeprom: &[],
    },
    HatEntry {
        id: "googlevoicehat-soundcard",
        label: "Google AIY Voice HAT",
        vendor: None,
        eeprom: &[],
    },
    HatEntry {
        id: "vc4-kms-v3d",
        label: "HDMI Audio",
        vendor: None,
        eeprom: &[],
    },
];

/// The overlay catalog for the manual dropdown — derived from [`HATS`] (SSOT).
pub fn overlay_catalog() -> Vec<DacOverlay> {
    HATS.iter()
        .map(|h| DacOverlay {
            id: h.id.into(),
            name: h.label.into(),
        })
        .collect()
}

/// Auto-apply DAC overlay on first boot if EEPROM detected and no overlay configured.
/// Returns true if overlay was applied (caller should reboot).
pub async fn auto_apply_dac_overlay() -> bool {
    let current = crate::config_txt::get_audio_overlay()
        .await
        .unwrap_or_default();
    if !current.is_empty() {
        return false; // Already configured
    }
    if let Some(overlay) = detect_hat_overlay().await {
        tracing::info!("First boot DAC auto-detect: applying overlay '{overlay}'");
        if crate::config_txt::set_audio_overlay(overlay).await.is_ok() {
            return true;
        }
    }
    false
}

/// Auto-detect the DAC overlay from the HAT EEPROM, using [`HATS`] (SSOT).
/// Reads the `product` atom (and `vendor`, since `HiFiBerry` often carries the
/// brand only there) and returns the first entry whose vendor guard passes and
/// one of whose `eeprom` substrings appears in the product string.
async fn detect_hat_overlay() -> Option<&'static str> {
    let product = read_file("/proc/device-tree/hat/product").await.ok()?;
    let vendor = read_file("/proc/device-tree/hat/vendor")
        .await
        .unwrap_or_default();
    match_hat_overlay(product.trim(), vendor.trim())
}

/// Pure [`HATS`] lookup behind [`detect_hat_overlay`] — split out so the tricky
/// vendor-gate / ordering cases can be unit-tested without touching `/proc`.
fn match_hat_overlay(product: &str, vendor: &str) -> Option<&'static str> {
    HATS.iter()
        .find(|h| {
            h.vendor
                .is_none_or(|v| product.contains(v) || vendor.contains(v))
                && h.eeprom.iter().any(|m| product.contains(*m))
        })
        .map(|h| h.id)
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
        available_overlays: overlay_catalog(),
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

const UPDATE_BASE_URL: &str = "https://updates.snapdog.cc/os/bundles";
/// Channel manifest published alongside the bundles (`latest-<channel>.json`).
/// It is the SSOT for the version a channel currently points at, so auto-update
/// can compare it to the running version without downloading the whole bundle.
const UPDATE_MANIFEST_BASE: &str = "https://updates.snapdog.cc/os/images";
/// Running OS version, written into the read-only rootfs at build time (SSOT).
const OS_VERSION_FILE: &str = "/etc/snapdog-os.version";
/// Version we handed to RAUC and are rebooting into, pending confirmation that it
/// actually boots. Written just before the post-install reboot and reconciled on
/// the next boot. Lives on the writable `/data` partition.
const PENDING_UPDATE_FILE: &str = "/data/snapdog-os.pending-update";
/// Version of a bundle that installed but failed to boot (the bootloader rolled
/// back). Auto-update refuses to reinstall this version so a broken bundle cannot
/// drive an endless install→rollback→reinstall loop that wears out the eMMC/SD.
const FAILED_UPDATE_FILE: &str = "/data/snapdog-os.failed-update";

/// Construct the bundle URL for a given channel.
pub async fn bundle_url(channel: &str) -> String {
    let board = detect_board().await;
    // Channel bundles are published as snapdog-os-<board>-<channel>.raucb — the
    // channel is "release" or "beta", matching the CI/CDN naming (the stable
    // channel is called "release" everywhere: manifest latest-release.json etc.).
    format!("{UPDATE_BASE_URL}/{board}-{channel}.raucb")
}

pub async fn detect_board() -> String {
    // Read compatible string from RAUC system.conf (e.g. "snapdog-os-pi4")
    let content = tokio::fs::read_to_string("/etc/rauc/system.conf")
        .await
        .unwrap_or_default();
    content
        .lines()
        .find_map(|l| l.strip_prefix("compatible="))
        .unwrap_or("snapdog-os-pi4")
        .to_string()
}

pub async fn check_update() -> UpdateCheckResponse {
    let current = current_os_version().await;
    let config = get_auto_update().await;
    let url = bundle_url(&config.channel).await;

    // RAUC verifies the bundle signature against the device keyring at install
    // time (`rauc install` refuses an unsigned or untrusted bundle), so when the
    // keyring is present the update is guaranteed to be cryptographically
    // verified before it is applied.
    let signature_verified = tokio::fs::metadata("/etc/rauc/ca.cert.pem").await.is_ok();

    // Compare the running version to the channel MANIFEST version (the SSOT for
    // what the channel points at) rather than treating mere URL reachability as
    // "an update exists". `available` = a strictly newer version is published;
    // `is_downgrade` = the channel points at an OLDER version than we run.
    // When the manifest is unreachable, `latest_version` is left empty and the UI
    // presents that as "cannot reach the update server" (NOT "up to date").
    let remote = remote_channel_version(&config.channel).await;
    let (available, is_downgrade, latest_version) = match remote {
        Some(r) if version_is_newer(&r, &current) => (true, false, r),
        Some(r) if version_is_newer(&current, &r) => (false, true, r),
        Some(r) => (false, false, r), // same version — up to date
        None => (false, false, String::new()), // manifest unknown/unreachable
    };

    UpdateCheckResponse {
        available,
        // A downgrade is manually installable too (e.g. switching the beta channel
        // back to stable): RAUC installs any signature-verified bundle regardless of
        // version, and only AUTO-update refuses to go backwards. Gate on the same
        // signature check as a forward update.
        installable: (available || is_downgrade) && signature_verified,
        current_version: if current.is_empty() {
            "unknown".into()
        } else {
            current
        },
        latest_version,
        channel: config.channel,
        is_downgrade,
        signature_verified,
        bundle_url: url,
    }
}

// --- Auto-Update Version Gating ---

/// Outcome of the auto-update pre-install decision.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum UpdateDecision {
    /// Install this (strictly newer, not known-bad) version.
    Install(String),
    /// Skip this cycle for the given human-readable reason.
    Skip(&'static str),
}

/// The currently running OS version (from the read-only rootfs SSOT), trimmed.
pub async fn current_os_version() -> String {
    read_file(OS_VERSION_FILE)
        .await
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// Fetch the version a channel currently points at from its manifest
/// (`latest-<channel>.json`). Returns `None` when the manifest is unreachable or
/// unparseable — callers must treat "unknown" as "do not install" rather than
/// installing blind.
pub async fn remote_channel_version(channel: &str) -> Option<String> {
    let url = format!("{UPDATE_MANIFEST_BASE}/latest-{channel}.json");
    let body = command_stdout("curl", &["-sf", "--max-time", "10", &url])
        .await
        .ok()?;
    let manifest: serde_json::Value = serde_json::from_str(&body).ok()?;
    let version = manifest.get("version")?.as_str()?.trim().to_string();
    (!version.is_empty()).then_some(version)
}

/// The version last marked bad after a failed boot/rollback, if any.
pub async fn last_failed_update() -> Option<String> {
    let version = read_file(FAILED_UPDATE_FILE).await.ok()?.trim().to_string();
    (!version.is_empty()).then_some(version)
}

/// Record the version we are about to reboot into (pending boot confirmation).
pub async fn record_pending_update(version: &str) {
    if let Err(e) = tokio::fs::write(PENDING_UPDATE_FILE, format!("{version}\n")).await {
        tracing::warn!("auto-update: failed to record pending update {version}: {e}");
    }
}

/// Device-local date (`YYYY-MM-DD`) of the last completed auto-update check. Drives
/// the interval/dedup gate and catch-up so a device that was off at the configured
/// time still updates on its next boot. Lives on the writable `/data` partition.
const LAST_AUTO_UPDATE_FILE: &str = "/data/snapdog-os.last-auto-update";

/// The date of the last completed auto-update run, if recorded.
pub async fn last_auto_update_date() -> Option<chrono::NaiveDate> {
    let raw = read_file(LAST_AUTO_UPDATE_FILE).await.ok()?;
    chrono::NaiveDate::parse_from_str(raw.trim(), "%Y-%m-%d").ok()
}

/// Record `date` (device-local) as the last auto-update run.
pub async fn record_auto_update_date(date: chrono::NaiveDate) {
    if let Err(e) = tokio::fs::write(LAST_AUTO_UPDATE_FILE, format!("{date}\n")).await {
        tracing::warn!("auto-update: failed to record last-run date {date}: {e}");
    }
}

async fn remove_state_file(path: &str) {
    if let Err(e) = tokio::fs::remove_file(path).await {
        if e.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!("auto-update: failed to remove {path}: {e}");
        }
    }
}

/// Reconcile the pending-update marker on boot. Called once at startup.
///
/// If we booted into the version we installed, the update took — clear the marker.
/// If we booted a *different* version, the pending bundle failed to boot and the
/// bootloader rolled us back — remember it as failed so auto-update never retries
/// it. Also clears a stale "failed" marker whenever we are successfully running
/// the version it names (e.g. the admin manually reinstalled a fixed bundle of the
/// same version), so auto-update is unblocked again.
pub async fn reconcile_pending_update() {
    let running = current_os_version().await;

    if !running.is_empty() && last_failed_update().await.as_deref() == Some(running.as_str()) {
        remove_state_file(FAILED_UPDATE_FILE).await;
    }

    let Some(pending) = read_file(PENDING_UPDATE_FILE)
        .await
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    else {
        return;
    };

    if running == pending {
        tracing::info!("auto-update: confirmed boot into {pending}");
        remove_state_file(FAILED_UPDATE_FILE).await;
    } else {
        tracing::warn!(
            "auto-update: bundle {pending} failed to boot (running {running}); \
             marking it bad to prevent a reinstall loop"
        );
        if let Err(e) = tokio::fs::write(FAILED_UPDATE_FILE, format!("{pending}\n")).await {
            tracing::warn!("auto-update: failed to record failed update {pending}: {e}");
        }
    }
    remove_state_file(PENDING_UPDATE_FILE).await;
}

/// Decide whether auto-update should install, given the remote version (if known),
/// the running version, and the last known-bad version. Pure so it can be tested.
#[must_use]
pub fn decide_update(
    remote: Option<&str>,
    current: &str,
    last_failed: Option<&str>,
) -> UpdateDecision {
    let Some(remote) = remote else {
        return UpdateDecision::Skip("remote version unknown");
    };
    if !version_is_newer(remote, current) {
        return UpdateDecision::Skip("already up to date");
    }
    if last_failed == Some(remote) {
        return UpdateDecision::Skip("bundle previously failed to boot");
    }
    UpdateDecision::Install(remote.to_string())
}

/// True when `remote` is a strictly newer version than `current`.
///
/// Compares dotted numeric components (ignoring a leading `v` and any
/// `-prerelease`/`+build` suffix). If either side cannot be parsed, falls back to
/// "install when they differ" so a legitimately different bundle is not blocked —
/// the last-failed gate still prevents a reinstall loop.
fn version_is_newer(remote: &str, current: &str) -> bool {
    match (parse_version(remote), parse_version(current)) {
        (Some(mut r), Some(mut c)) => {
            let width = r.len().max(c.len());
            r.resize(width, 0);
            c.resize(width, 0);
            r > c
        }
        _ => remote.trim() != current.trim(),
    }
}

fn parse_version(version: &str) -> Option<Vec<u64>> {
    let core = version.trim().trim_start_matches('v');
    let core = core.split(['-', '+']).next().unwrap_or(core);
    let parts: Vec<u64> = core
        .split('.')
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    (!parts.is_empty()).then_some(parts)
}

// --- Factory Reset ---

pub async fn factory_reset() -> Result<()> {
    tracing::warn!("Factory reset initiated");

    // Remove configurations directly from the writeable /data partition to preserve symbolic links
    let wifi_iface = crate::network::detect_wifi_interface().await;
    let _ = tokio::fs::remove_file(format!(
        "/data/wpa_supplicant/wpa_supplicant-{wifi_iface}.conf"
    ))
    .await;
    let _ = tokio::fs::remove_file("/data/wpa_supplicant/wpa_supplicant-wlan0.conf").await;
    let _ = tokio::fs::remove_file("/data/systemd/network/10-ethernet.network").await;
    let _ = tokio::fs::remove_file("/data/systemd/network/15-ap.network").await;
    let _ = tokio::fs::remove_file("/data/systemd/network/20-wifi.network").await;
    let _ = tokio::fs::remove_file("/data/default/snapdog-client").await;
    let _ = tokio::fs::remove_file("/data/hostname").await;
    let _ = tokio::fs::remove_file("/data/snapdog-os.channel").await;
    let _ = tokio::fs::remove_file("/data/snapdog-os.last-auto-update").await;
    let _ = tokio::fs::remove_file(PENDING_UPDATE_FILE).await;
    let _ = tokio::fs::remove_file(FAILED_UPDATE_FILE).await;
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

/// Extract the IANA zone name (e.g. `Europe/Berlin`) from a resolved localtime
/// path such as `/usr/share/zoneinfo/Europe/Berlin` or
/// `/usr/share/zoneinfo/posix/Europe/Berlin`.
fn zone_from_path(path: &str) -> Option<String> {
    let after = path.rsplit_once("zoneinfo/")?.1;
    // `canonicalize` may resolve through the parallel posix/ or right/ hierarchies.
    let after = after
        .strip_prefix("posix/")
        .or_else(|| after.strip_prefix("right/"))
        .unwrap_or(after);
    let zone = after.trim_matches('/');
    (!zone.is_empty()).then(|| zone.to_string())
}

/// Read the configured zone name from the localtime symlink chain.
///
/// `timedatectl show` only follows a single symlink level, so with the read-only
/// rootfs indirection `/etc/localtime -> /data/localtime -> zoneinfo/<tz>` it sees
/// `/data/localtime` (not a zoneinfo path) and reports an empty zone — making a
/// persisted timezone read back as UTC. Read the target ourselves: prefer the
/// direct symlink (a clean `zoneinfo/<tz>`), falling back to the fully-resolved
/// canonical path.
async fn read_localtime_zone() -> Option<String> {
    for path in ["/data/localtime", "/etc/localtime"] {
        if let Ok(target) = tokio::fs::read_link(path).await {
            if let Some(z) = zone_from_path(&target.to_string_lossy()) {
                return Some(z);
            }
        }
        if let Ok(real) = tokio::fs::canonicalize(path).await {
            if let Some(z) = zone_from_path(&real.to_string_lossy()) {
                return Some(z);
            }
        }
    }
    None
}

/// Read the zone via `timedatectl show`. Only reliable when `/etc/localtime` is a
/// single symlink straight into `zoneinfo/` (or a copied file) — see
/// [`read_localtime_zone`] for why it can't see our `/data` indirection — so it is
/// used as a fallback for environments configured via `timedatectl set-timezone`.
async fn timedatectl_zone() -> Option<String> {
    let out = tokio::process::Command::new("timedatectl")
        .args(["show", "--property=Timezone", "--value"])
        .output()
        .await
        .ok()?;
    let zone = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!zone.is_empty()).then_some(zone)
}

pub async fn get_timezone() -> TimezoneInfo {
    // Prefer the localtime symlink chain (see read_localtime_zone), then fall back
    // to `timedatectl show` for environments where /etc/localtime is a plain copied
    // file configured via `timedatectl set-timezone` (the set_timezone fallback),
    // and finally to UTC so the dropdown lands on UTC rather than the first
    // alphabetical zone (Africa/Abidjan), which reads as an unintended selection.
    let current = match read_localtime_zone().await {
        Some(zone) => zone,
        None => timedatectl_zone().await.unwrap_or_else(|| "UTC".into()),
    };

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
    let path = std::path::Path::new("/data/localtime");
    if path.exists() || path.is_symlink() {
        tokio::fs::remove_file(path).await.ok();
        let target = format!("/usr/share/zoneinfo/{tz}");
        tokio::fs::symlink(target, path)
            .await
            .context("failed to update /data/localtime timezone symlink")?;
        // Re-create /etc/localtime → /data/localtime so its OWN mtime bumps. chrono's
        // `Local` cache keys invalidation on the /etc/localtime symlink's mtime (it lstats,
        // does not follow the link), and set_timezone only rewrites /data/localtime — so
        // without this the long-running auto-update scheduler keeps using the OLD zone
        // until a restart. /etc/localtime is in the unit's ReadWritePaths.
        let etc = std::path::Path::new("/etc/localtime");
        tokio::fs::remove_file(etc).await.ok();
        tokio::fs::symlink("/data/localtime", etc).await.ok();
        Ok(())
    } else {
        run_cmd("timedatectl", &["set-timezone", tz]).await
    }
}

// --- Soundcards ---

pub async fn list_soundcards() -> Vec<Soundcard> {
    let output = tokio::process::Command::new("aplay")
        .args(["-l"])
        .output()
        .await
        .ok();

    output
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter_map(parse_soundcard_line)
                .collect()
        })
        .unwrap_or_default()
}

/// Parse one `aplay -l` line — `card N: id [name], device M: ...` — into the
/// ALSA device (`hw:N`) plus a friendly name (the bracketed name, or the id).
fn parse_soundcard_line(line: &str) -> Option<Soundcard> {
    let (num, after) = line.strip_prefix("card ")?.split_once(':')?;
    let num: u8 = num.trim().parse().ok()?;
    let name = after
        .split_once('[')
        .and_then(|(_, rest)| rest.split_once(']'))
        .map(|(n, _)| n.trim())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| after.split(',').next().unwrap_or(after).trim())
        .to_string();
    Some(Soundcard {
        device: format!("hw:{num}"),
        name,
    })
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
            .unwrap_or("release")
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
    atomic_write(CTRL_CONFIG, &doc.to_string()).await?;
    Ok(())
}

// --- SoftAP Settings ---

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct SoftApConfig {
    pub enabled: bool,
    pub password: String,
    #[serde(default = "default_country")]
    pub country: String,
}

fn default_country() -> String {
    crate::network::DEFAULT_COUNTRY.to_string()
}

/// Read-only view for GET — never leaks the passphrase onto the LAN. It's
/// included only while the setup AP is active (the requester is on the AP and
/// already knows it, and needs it shown on the setup page).
#[derive(serde::Serialize)]
pub struct SoftApView {
    pub enabled: bool,
    pub ssid: String,
    pub country: String,
    pub password: Option<String>,
}

/// A random 12-char passphrase over an unambiguous alphabet (no 0/o/1/l), so
/// each device ships with a unique default instead of a shared hardcoded one.
/// It is logged to the console at AP start for out-of-band first-join retrieval.
fn gen_ap_password() -> String {
    const ALPHABET: &[u8] = b"abcdefghijkmnpqrstuvwxyz23456789";
    let mut buf = [0u8; 12];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        use std::io::Read as _;
        let _ = f.read_exact(&mut buf);
    }
    buf.iter()
        .map(|b| ALPHABET[(*b as usize) % ALPHABET.len()] as char)
        .collect()
}

pub async fn get_softap_config() -> SoftApConfig {
    let content = read_file(CTRL_CONFIG).await.unwrap_or_default();
    let doc: toml_edit::DocumentMut = content.parse().unwrap_or_default();
    let ap = doc.get("softap");
    let enabled = ap
        .and_then(|t| t.get("enabled"))
        .and_then(toml_edit::Item::as_bool)
        .unwrap_or(true);
    let country = ap
        .and_then(|t| t.get("country"))
        .and_then(|v| v.as_str())
        .unwrap_or(crate::network::DEFAULT_COUNTRY)
        .to_string();
    let stored = ap
        .and_then(|t| t.get("password"))
        .and_then(|v| v.as_str())
        .filter(|p| !p.is_empty());
    let password = if let Some(p) = stored {
        p.to_string()
    } else {
        // First access with no stored passphrase → generate + persist a unique one.
        let pw = gen_ap_password();
        let cfg = SoftApConfig {
            enabled,
            password: pw.clone(),
            country: country.clone(),
        };
        if let Err(e) = set_softap_config(cfg).await {
            tracing::warn!("failed to persist generated AP password: {e:#}");
        }
        pw
    };
    SoftApConfig {
        enabled,
        password,
        country,
    }
}

pub async fn get_softap_view() -> SoftApView {
    let cfg = get_softap_config().await;
    let password = if crate::network::is_ap_active().await {
        Some(cfg.password)
    } else {
        None
    };
    SoftApView {
        enabled: cfg.enabled,
        ssid: crate::network::ap_ssid().await,
        country: cfg.country,
        password,
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
    ap["country"] = toml_edit::value(&config.country);

    if let Some(parent) = std::path::Path::new(CTRL_CONFIG).parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    atomic_write(CTRL_CONFIG, &doc.to_string()).await?;
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

/// Writable flag that gates `sshd.service` on the read-only rootfs. `post-build.sh`
/// gives sshd a drop-in with `ConditionPathExists=/data/ssh.enabled`, so sshd only
/// starts (at boot or on demand) while this file exists. We toggle the flag instead
/// of `systemctl mask/unmask` or enable/disable, which cannot write to read-only /etc.
const SSH_ENABLED_FLAG: &str = "/data/ssh.enabled";

/// Create or remove the flag that gates `sshd.service`.
async fn set_ssh_enabled_flag(enabled: bool) -> Result<()> {
    if enabled {
        tokio::fs::write(SSH_ENABLED_FLAG, b"")
            .await
            .context("failed to create SSH enable flag")?;
    } else if let Err(e) = tokio::fs::remove_file(SSH_ENABLED_FLAG).await {
        if e.kind() != std::io::ErrorKind::NotFound {
            return Err(e).context("failed to remove SSH enable flag");
        }
    }
    Ok(())
}

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
            if *key == "ssh" {
                let _ = set_ssh_enabled_flag(true).await;
            }
            let _ = run_cmd("systemctl", &["start", unit]).await;
        } else {
            let _ = run_cmd("systemctl", &["stop", unit]).await;
            if *key == "ssh" {
                let _ = set_ssh_enabled_flag(false).await;
            }
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
    atomic_write(CTRL_CONFIG, &doc.to_string()).await?;

    if enabled {
        if name == "ssh" {
            set_ssh_enabled_flag(true).await?;
        }
        run_cmd("systemctl", &["start", unit]).await?;
    } else {
        run_cmd("systemctl", &["stop", unit]).await?;
        if name == "ssh" {
            set_ssh_enabled_flag(false).await?;
        }
    }
    Ok(())
}

pub async fn is_service_enabled(name: &str) -> bool {
    let config = get_service_config().await;
    *config.get(name).unwrap_or(&false)
}

// --- Server Connectivity Test ---

pub async fn test_server(host: &str, port: u16) -> bool {
    // Only allow connections to private/link-local IPs (prevent SSRF)
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        let is_private = match ip {
            std::net::IpAddr::V4(v4) => v4.is_private() || v4.is_link_local() || v4.is_loopback(),
            std::net::IpAddr::V6(v6) => v6.is_loopback(),
        };
        if !is_private {
            return false;
        }
    }
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

/// Write file atomically: write to temp, fsync, rename.
pub async fn atomic_write(path: &str, content: &str) -> Result<()> {
    let tmp = format!("{path}.tmp");
    tokio::fs::write(&tmp, content).await?;
    // fsync the file
    let f = tokio::fs::File::open(&tmp).await?;
    f.sync_all().await?;
    drop(f);
    tokio::fs::rename(&tmp, path).await?;
    Ok(())
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
    fn wifi_state_completed_maps_by_ip() {
        assert_eq!(classify_wifi_state("COMPLETED", true, false), "connected");
        assert_eq!(
            classify_wifi_state("COMPLETED", false, false),
            "acquiring_ip"
        );
    }

    #[test]
    fn wifi_state_temp_disabled_beats_retry_states() {
        // Regression: after WRONG_KEY the network is TEMP-DISABLED but wpa_supplicant
        // cycles back through SCANNING/ASSOCIATING while retrying — it must surface as
        // auth_failed, not a perpetual "associating".
        assert_eq!(classify_wifi_state("SCANNING", false, true), "auth_failed");
        assert_eq!(
            classify_wifi_state("ASSOCIATING", false, true),
            "auth_failed"
        );
        assert_eq!(
            classify_wifi_state("DISCONNECTED", false, true),
            "auth_failed"
        );
    }

    #[test]
    fn wifi_state_first_attempt_is_associating() {
        // Before any failure the network is not temp-disabled → normal progress.
        assert_eq!(
            classify_wifi_state("ASSOCIATING", false, false),
            "associating"
        );
        assert_eq!(
            classify_wifi_state("4WAY_HANDSHAKE", false, false),
            "associating"
        );
        assert_eq!(classify_wifi_state("SCANNING", false, false), "associating");
    }

    #[test]
    fn wifi_state_idle_is_disconnected() {
        assert_eq!(
            classify_wifi_state("DISCONNECTED", false, false),
            "disconnected"
        );
        assert_eq!(
            classify_wifi_state("INACTIVE", false, false),
            "disconnected"
        );
    }

    #[test]
    fn hat_detection_covers_tricky_cases() {
        // HiFiBerry: bare model names, brand only in `vendor`.
        assert_eq!(
            match_hat_overlay("Amp3", "HiFiBerry"),
            Some("hifiberry-amp3")
        );
        assert_eq!(
            match_hat_overlay("Amp4 Pro", "HiFiBerry"),
            Some("hifiberry-amp4pro")
        );
        assert_eq!(
            match_hat_overlay("Amp100", "HiFiBerry"),
            Some("hifiberry-amp100")
        );
        assert_eq!(
            match_hat_overlay("DAC 2 HD", "HiFiBerry"),
            Some("hifiberry-dacplushd")
        );
        assert_eq!(
            match_hat_overlay("Digi2 Pro", "HiFiBerry"),
            Some("hifiberry-digi-pro")
        );
        assert_eq!(
            match_hat_overlay("Digi+", "HiFiBerry"),
            Some("hifiberry-digi")
        );
        // Amp2 reports as "HiFiBerry DAC+"; Amp4/DAC+ hit the dacplus catch-all.
        assert_eq!(
            match_hat_overlay("HiFiBerry DAC+", "HiFiBerry"),
            Some("hifiberry-dacplus")
        );
        // IQaudio / official RPi must NOT be stolen by the HiFiBerry Digi row
        // ("DigiAMP+" contains "Digi") or the HiFiBerry DAC+ catch-all.
        assert_eq!(
            match_hat_overlay("DigiAMP+", "IQaudIO Limited"),
            Some("iqaudio-dacplus")
        );
        assert_eq!(
            match_hat_overlay("IQaudIO DAC+", "IQaudIO Limited"),
            Some("iqaudio-dacplus")
        );
        // Codec Zero is more specific than DAC+ — its row wins despite "IQaudIO".
        assert_eq!(
            match_hat_overlay("IQaudIO Codec Zero", "IQaudIO Limited"),
            Some("iqaudio-codec")
        );
        // JustBoom "Digi" must not be caught by the HiFiBerry Digi row.
        assert_eq!(
            match_hat_overlay("JustBoom Digi HAT", "JustBoom"),
            Some("justboom-digi")
        );
        assert_eq!(
            match_hat_overlay("JustBoom DAC HAT", "JustBoom"),
            Some("justboom-dac")
        );
        // Unknown board → no auto-overlay.
        assert_eq!(match_hat_overlay("Mystery Board", "Nobody Inc"), None);
    }

    #[test]
    fn parse_soundcard_line_extracts_device_and_name() {
        let sc = parse_soundcard_line(
            "card 1: sndrpihifiberry [snd_rpi_hifiberry_dacplus], device 0: HiFiBerry [x]",
        )
        .unwrap();
        assert_eq!(sc.device, "hw:1");
        assert_eq!(sc.name, "snd_rpi_hifiberry_dacplus");
        // Non-card header lines are ignored.
        assert!(parse_soundcard_line("**** List of PLAYBACK Hardware Devices ****").is_none());
    }

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
    fn version_comparison_is_strict_and_numeric() {
        assert!(version_is_newer("0.4.0", "0.3.0"));
        assert!(version_is_newer("0.3.1", "0.3.0"));
        assert!(version_is_newer("1.0.0", "0.9.9"));
        // Equal is not newer — this is what stops the daily reinstall of the
        // bundle already running.
        assert!(!version_is_newer("0.3.0", "0.3.0"));
        // Never auto-downgrade.
        assert!(!version_is_newer("0.2.9", "0.3.0"));
        // Numeric, not lexicographic (would fail a naive string compare).
        assert!(version_is_newer("0.10.0", "0.9.0"));
        // Leading `v` and build/prerelease suffixes are ignored for the core cmp.
        assert!(version_is_newer("v0.4.0", "0.3.0"));
        assert!(!version_is_newer("0.3.0+build.7", "0.3.0"));
        // Unparseable but different → treated as installable (last-failed gate is
        // the backstop); identical unparseable → not newer.
        assert!(version_is_newer("nightly-b", "nightly-a"));
        assert!(!version_is_newer("weird", "weird"));
    }

    #[test]
    fn decide_update_only_installs_strictly_newer_non_bad_versions() {
        // Newer and never failed → install.
        assert_eq!(
            decide_update(Some("0.4.0"), "0.3.0", None),
            UpdateDecision::Install("0.4.0".into())
        );
        // Same version → skip (prevents the flash-wearing daily reinstall).
        assert_eq!(
            decide_update(Some("0.3.0"), "0.3.0", None),
            UpdateDecision::Skip("already up to date")
        );
        // Newer but previously rolled back → skip (breaks the reinstall loop).
        assert_eq!(
            decide_update(Some("0.4.0"), "0.3.0", Some("0.4.0")),
            UpdateDecision::Skip("bundle previously failed to boot")
        );
        // A newer version than the known-bad one is still allowed through.
        assert_eq!(
            decide_update(Some("0.5.0"), "0.3.0", Some("0.4.0")),
            UpdateDecision::Install("0.5.0".into())
        );
        // Manifest unreachable → never install blind.
        assert_eq!(
            decide_update(None, "0.3.0", None),
            UpdateDecision::Skip("remote version unknown")
        );
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

    #[test]
    fn zone_from_path_extracts_iana_name() {
        assert_eq!(
            zone_from_path("/usr/share/zoneinfo/Europe/Berlin").as_deref(),
            Some("Europe/Berlin")
        );
        // canonicalize() can resolve through the parallel posix/ hierarchy.
        assert_eq!(
            zone_from_path("/usr/share/zoneinfo/posix/Europe/Berlin").as_deref(),
            Some("Europe/Berlin")
        );
        assert_eq!(
            zone_from_path("../usr/share/zoneinfo/UTC").as_deref(),
            Some("UTC")
        );
        // Not a zoneinfo path (the one-level /data/localtime target) → no zone.
        assert_eq!(zone_from_path("/data/localtime"), None);
        assert_eq!(zone_from_path("/usr/share/zoneinfo/"), None);
    }
}
