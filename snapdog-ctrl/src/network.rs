// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! Network management — `WiFi` AP mode, client mode, ethernet, and scanning.
//! Replaces the shell-script based raspi-wifi package.

use anyhow::{Context, Result};
use tokio::process::Command;

const WPA_CONF: &str = "/etc/wpa_supplicant/wpa_supplicant-wlan0.conf";
const HOSTAPD_CONF: &str = "/etc/hostapd/hostapd.conf";
const DNSMASQ_CONF: &str = "/etc/dnsmasq.d/snapdog-ap.conf";
pub const ETH_NETWORK_PATH: &str = "/etc/systemd/network/10-ethernet.network";
const WIFI_NETWORK: &str = "/etc/systemd/network/20-wifi.network";

/// Check if `WiFi` is configured (has at least one network block).
pub async fn is_wifi_configured() -> bool {
    tokio::fs::read_to_string(WPA_CONF)
        .await
        .is_ok_and(|c| c.contains("network="))
}

/// Start temporary AP mode for initial setup.
pub async fn start_ap(password: &str) -> Result<()> {
    tracing::info!("Starting temporary AP mode");

    // Derive unique SSID from last 4 hex chars of wlan0 MAC
    let ssid = tokio::fs::read_to_string("/sys/class/net/wlan0/address")
        .await
        .map_or_else(
            |_| "SnapDog-Setup".into(),
            |mac| {
                let suffix: String = mac
                    .trim()
                    .replace(':', "")
                    .chars()
                    .rev()
                    .take(4)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect();
                format!("SnapDog-{}", suffix.to_uppercase())
            },
        );

    // Write hostapd config
    let hostapd = format!(
        "interface=wlan0\ndriver=nl80211\nssid={ssid}\nhw_mode=g\nchannel=6\n\
         ieee80211n=1\nwmm_enabled=1\nwpa=2\nwpa_passphrase={password}\n\
         wpa_key_mgmt=WPA-PSK\nrsn_pairwise=CCMP\n"
    );
    write_config(HOSTAPD_CONF, &hostapd).await?;

    // Write dnsmasq config for DHCP on AP
    let dnsmasq = "interface=wlan0\nbind-interfaces\n\
         dhcp-range=10.11.12.100,10.11.12.200,255.255.255.0,24h\n\
         address=/#/10.11.12.13\n";
    write_config(DNSMASQ_CONF, dnsmasq).await?;

    // Assign static IP to wlan0 for AP mode
    run("ip", &["addr", "flush", "dev", "wlan0"]).await?;
    run("ip", &["addr", "add", "10.11.12.13/24", "dev", "wlan0"]).await?;
    run("ip", &["link", "set", "wlan0", "up"]).await?;

    // Stop wpa_supplicant, start hostapd + dnsmasq
    let _ = run("systemctl", &["stop", "wpa_supplicant@wlan0"]).await;
    run("systemctl", &["start", "hostapd"]).await?;
    run("systemctl", &["start", "dnsmasq"]).await?;

    Ok(())
}

/// Stop AP mode and switch to `WiFi` client mode.
pub async fn stop_ap() -> Result<()> {
    tracing::info!("Stopping AP mode, switching to client");
    let _ = run("systemctl", &["stop", "hostapd"]).await;
    let _ = run("systemctl", &["stop", "dnsmasq"]).await;
    run("ip", &["addr", "flush", "dev", "wlan0"]).await?;
    run("systemctl", &["start", "systemd-resolved"]).await?;
    run("systemctl", &["restart", "wpa_supplicant@wlan0"]).await?;
    run("systemctl", &["restart", "systemd-networkd"]).await?;
    Ok(())
}

/// Connect to a `WiFi` network.
pub async fn connect_wifi(
    ssid: &str,
    password: &str,
    static_ip: Option<&StaticConfig>,
) -> Result<()> {
    tracing::info!("Connecting to WiFi: {ssid}");
    if let Some(config) = static_ip {
        validate_static_config(config)?;
    }
    let ssid = wpa_quoted_string("ssid", ssid)?;
    let password = wpa_quoted_string("password", password)?;

    let wpa = format!(
        "ctrl_interface=/var/run/wpa_supplicant\nupdate_config=1\ncountry=DE\np2p_disabled=1\n\n\
         network={{\n  ssid=\"{ssid}\"\n  psk=\"{password}\"\n  key_mgmt=WPA-PSK\n}}\n"
    );
    write_config(WPA_CONF, &wpa).await?;

    // Write networkd config for wlan0
    let network = static_ip.map_or_else(
        || "[Match]\nName=wlan0\n\n[Network]\nDHCP=yes\n".to_string(),
        |s| {
            format!(
                "[Match]\nName=wlan0\n\n[Network]\nAddress={}/{}\nGateway={}\nDNS={}\n",
                s.ip,
                subnet_to_prefix(&s.subnet),
                s.gateway,
                s.dns
            )
        },
    );
    write_config(WIFI_NETWORK, &network).await?;

    stop_ap().await?;
    Ok(())
}

/// Disconnect `WiFi` and remove saved credentials.
pub async fn disconnect_wifi() -> Result<()> {
    tracing::info!("Disconnecting WiFi");
    let wpa =
        "ctrl_interface=/var/run/wpa_supplicant\nupdate_config=1\ncountry=DE\np2p_disabled=1\n";
    write_config(WPA_CONF, wpa).await?;
    run("systemctl", &["restart", "wpa_supplicant@wlan0"]).await?;
    Ok(())
}

/// Scan for available `WiFi` networks.
pub async fn scan_networks() -> Result<Vec<ScannedNetwork>> {
    // Trigger scan
    let _ = Command::new("wpa_cli")
        .args(["-i", "wlan0", "scan"])
        .output()
        .await;

    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let output = Command::new("wpa_cli")
        .args(["-i", "wlan0", "scan_results"])
        .output()
        .await
        .context("wpa_cli scan_results failed")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let networks = stdout
        .lines()
        .skip(1) // header line
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 5 {
                let signal = parts[2].parse::<i32>().unwrap_or(-100);
                let flags = parts[3];
                let ssid = parts[4].to_string();
                if ssid.is_empty() {
                    return None;
                }
                let security = if flags.contains("WPA") {
                    "wpa2"
                } else if flags.contains("WEP") {
                    "wep"
                } else {
                    "open"
                };
                Some(ScannedNetwork {
                    ssid,
                    signal,
                    security: security.into(),
                })
            } else {
                None
            }
        })
        .collect();

    Ok(networks)
}

/// Configure ethernet (DHCP or static).
pub async fn configure_ethernet(static_ip: Option<&StaticConfig>) -> Result<()> {
    if let Some(config) = static_ip {
        validate_static_config(config)?;
    }

    let network = static_ip.map_or_else(
        || "[Match]\nName=eth0 end0\n\n[Network]\nDHCP=yes\n".to_string(),
        |s| {
            format!(
                "[Match]\nName=eth0 end0\n\n[Network]\nAddress={}/{}\nGateway={}\nDNS={}\n",
                s.ip,
                subnet_to_prefix(&s.subnet),
                s.gateway,
                s.dns
            )
        },
    );
    write_config(ETH_NETWORK_PATH, &network).await?;
    run("systemctl", &["restart", "systemd-networkd"]).await?;
    Ok(())
}

/// Configure systemd-resolved (disable stub resolver).
pub async fn configure_resolved() -> Result<()> {
    // Stop resolved entirely — dnsmasq takes over DNS in AP mode
    run("systemctl", &["stop", "systemd-resolved"]).await?;
    Ok(())
}

// ── Types ─────────────────────────────────────────────────────

pub struct StaticConfig {
    pub ip: String,
    pub subnet: String,
    pub gateway: String,
    pub dns: String,
}

pub struct ScannedNetwork {
    pub ssid: String,
    pub signal: i32,
    pub security: String,
}

// ── Helpers ───────────────────────────────────────────────────

async fn write_config(path: &str, content: &str) -> Result<()> {
    if let Some(parent) = std::path::Path::new(path).parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(path, content)
        .await
        .context("failed to write network configuration file")
}

fn validate_static_config(config: &StaticConfig) -> Result<()> {
    validate_single_line("ip", &config.ip)?;
    validate_single_line("subnet", &config.subnet)?;
    validate_single_line("gateway", &config.gateway)?;
    validate_single_line("dns", &config.dns)
}

fn wpa_quoted_string(field: &str, value: &str) -> Result<String> {
    validate_single_line(field, value)?;
    Ok(value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn validate_single_line(field: &str, value: &str) -> Result<()> {
    anyhow::ensure!(
        !value.chars().any(|c| matches!(c, '\n' | '\r' | '\0')),
        "{field} contains unsupported control characters"
    );
    Ok(())
}

async fn run(cmd: &str, args: &[&str]) -> Result<()> {
    let output = Command::new(cmd).args(args).output().await?;
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

fn subnet_to_prefix(subnet: &str) -> u8 {
    let bits: u32 = subnet
        .split('.')
        .filter_map(|o| o.parse::<u8>().ok())
        .map(u8::count_ones)
        .sum();
    u8::try_from(bits).unwrap_or(32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wpa_quoted_string_escapes_quotes_and_backslashes() {
        assert_eq!(
            wpa_quoted_string("ssid", r#"Kitchen "DAC" \ 1"#).unwrap(),
            r#"Kitchen \"DAC\" \\ 1"#
        );
    }

    #[test]
    fn wpa_quoted_string_rejects_newlines() {
        assert!(wpa_quoted_string("ssid", "bad\nssid").is_err());
    }
}
