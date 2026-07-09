// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

use std::net::Ipv4Addr;
use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::process::Command;
use tokio::sync::Mutex;

const HOSTAPD_CONF: &str = "/etc/hostapd/hostapd.conf";
pub const ETH_NETWORK_PATH: &str = "/etc/systemd/network/10-ethernet.network";
// 15- sorts before 20-wifi, so while present it takes precedence on the wlan iface.
const AP_NETWORK: &str = "/etc/systemd/network/15-ap.network";
const WIFI_NETWORK: &str = "/etc/systemd/network/20-wifi.network";

// ── Setup-AP profile (single source of truth) ──
/// Static address the device serves in setup-AP mode: gateway, DNS, and the host
/// every captive-portal probe resolves to (see `captive_dns`).
pub const AP_IP: Ipv4Addr = Ipv4Addr::new(10, 11, 12, 13);
/// Subnet prefix length for the AP network.
const AP_PREFIX: u8 = 24;
/// DHCP pool inside the AP subnet: first-host offset and number of leases.
const AP_DHCP_POOL_OFFSET: u32 = 100;
const AP_DHCP_POOL_SIZE: u32 = 100;
/// Default regulatory country when none is configured. Governs both the AP
/// (hostapd) and client (`wpa_supplicant`) radio behaviour.
pub const DEFAULT_COUNTRY: &str = "DE";
/// How long `connect_wifi` waits before tearing the setup AP down, so the HTTP
/// response reaches the browser BEFORE its link to the AP drops.
const AP_TEARDOWN_GRACE: Duration = Duration::from_millis(1500);

/// Serializes every AP teardown. Both `connect_wifi`'s deferred task and the
/// boot auto-close loop can call `stop_ap` concurrently; without this they race
/// on hostapd/networkd (double-stop, half-applied config). The teardown is also
/// idempotent (a no-op when the AP is already down).
fn ap_teardown_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn wpa_conf_path(iface: &str) -> String {
    format!("/etc/wpa_supplicant/wpa_supplicant-{iface}.conf")
}

/// Dynamically detects the primary wireless interface name.
/// Falls back to "wlan0" if none is found.
pub async fn detect_wifi_interface() -> String {
    if let Ok(mut entries) = tokio::fs::read_dir("/sys/class/net").await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            if let Ok(name) = entry.file_name().into_string() {
                if name.starts_with("wl") {
                    return name;
                }
            }
        }
    }
    "wlan0".to_string()
}

/// Dynamically detects all ethernet interface names.
/// Falls back to `["eth0"]` if none are found.
pub async fn detect_ethernet_interfaces() -> Vec<String> {
    let mut eths = Vec::new();
    if let Ok(mut entries) = tokio::fs::read_dir("/sys/class/net").await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            if let Ok(name) = entry.file_name().into_string() {
                if name.starts_with("eth") || name.starts_with("en") {
                    eths.push(name);
                }
            }
        }
    }
    if eths.is_empty() {
        vec!["eth0".to_string()]
    } else {
        eths
    }
}

/// Check if `WiFi` is configured (has at least one network block).
pub async fn is_wifi_configured() -> bool {
    let iface = detect_wifi_interface().await;
    tokio::fs::read_to_string(wpa_conf_path(&iface))
        .await
        .is_ok_and(|c| c.contains("network="))
}

/// True while the device is serving the setup access point. A single-radio Pi
/// cannot beacon as an AP and run a managed-mode client scan at the same time,
/// so callers use this to explain why a scan is unavailable rather than return
/// an empty list. Checks both the live hostapd unit and the networkd AP profile
/// (either is authoritative depending on how far a teardown has progressed).
pub async fn is_ap_active() -> bool {
    if tokio::fs::try_exists(AP_NETWORK).await.unwrap_or(false) {
        return true;
    }
    Command::new("systemctl")
        .args(["is-active", "--quiet", "hostapd"])
        .status()
        .await
        .is_ok_and(|s| s.success())
}

/// The setup-AP SSID for this device: `SnapDog-<last 4 MAC hex>`.
pub async fn ap_ssid() -> String {
    let iface = detect_wifi_interface().await;
    let address_path = format!("/sys/class/net/{iface}/address");
    tokio::fs::read_to_string(&address_path)
        .await
        .map_or_else(|_| "SnapDog-Setup".into(), |mac| derive_ssid(&mac))
}

/// Sanitize a caller-supplied regulatory country to an uppercase ISO-3166 alpha-2
/// (two ASCII letters), falling back to the default. Prevents config injection
/// via the country field.
fn sanitize_country(country: &str) -> String {
    let c = country.trim();
    if c.len() == 2 && c.chars().all(|ch| ch.is_ascii_alphabetic()) {
        c.to_ascii_uppercase()
    } else {
        DEFAULT_COUNTRY.to_string()
    }
}

/// Start temporary AP mode for initial setup.
pub async fn start_ap(password: &str, country: &str) -> Result<()> {
    let iface = detect_wifi_interface().await;
    let country = sanitize_country(country);
    tracing::info!("Starting temporary AP mode on interface {iface} (country {country})");

    let ssid = ap_ssid().await;

    // Write hostapd config. country_code + ieee80211d let the radio honour the
    // regulatory domain (needs wireless-regdb in the image).
    let hostapd = format!(
        "interface={iface}\ndriver=nl80211\nssid={ssid}\ncountry_code={country}\nieee80211d=1\n\
         hw_mode=g\nchannel=6\nieee80211n=1\nwmm_enabled=1\nwpa=2\nwpa_passphrase={password}\n\
         wpa_key_mgmt=WPA-PSK\nrsn_pairwise=CCMP\n"
    );
    write_config(HOSTAPD_CONF, &hostapd).await?;

    // networkd owns addressing on the AP interface too: static address, built-in
    // DHCP server, and the RFC 8910 captive-portal URL (DHCP option 114) for
    // modern clients. ConfigureWithoutCarrier so it applies before hostapd brings
    // the radio (and thus carrier) up.
    let ap_network = format!(
        "[Match]\nName={iface}\n\n\
         [Network]\nAddress={AP_IP}/{AP_PREFIX}\nDHCPServer=yes\nConfigureWithoutCarrier=yes\n\n\
         [DHCPServer]\nPoolOffset={AP_DHCP_POOL_OFFSET}\nPoolSize={AP_DHCP_POOL_SIZE}\nEmitDNS=yes\nDNS={AP_IP}\n\
         SendOption=114:string:http://{AP_IP}/\n"
    );
    write_config(AP_NETWORK, &ap_network).await?;

    // Apply the AP config without a full networkd restart.
    run("networkctl", &["reload"]).await?;
    run("networkctl", &["reconfigure", &iface]).await?;

    // Stop the wpa_supplicant client, start hostapd (radio). The captive-portal
    // wildcard DNS (every name -> AP_IP) is served in-process, see captive_dns.
    let _ = run("systemctl", &["stop", &format!("wpa_supplicant@{iface}")]).await;
    run("systemctl", &["start", "hostapd"]).await?;
    crate::captive_dns::start().await;

    Ok(())
}

/// Stop AP mode and switch to `WiFi` client mode. Idempotent and serialized:
/// safe to call from both `connect_wifi` and the boot auto-close loop.
pub async fn stop_ap() -> Result<()> {
    let _guard = ap_teardown_lock().lock().await;
    if !is_ap_active().await {
        tracing::debug!("stop_ap: AP already down, nothing to do");
        return Ok(());
    }
    let iface = detect_wifi_interface().await;
    tracing::info!("Stopping AP mode on interface {iface}, switching to client");
    let _ = run("systemctl", &["stop", "hostapd"]).await;
    crate::captive_dns::stop();
    // Drop the AP config so the client config (20-wifi.network) applies again,
    // then reconfigure the interface without a full networkd restart.
    let _ = tokio::fs::remove_file(AP_NETWORK).await;
    run("networkctl", &["reload"]).await?;
    run("networkctl", &["reconfigure", &iface]).await?;
    run("systemctl", &["start", "systemd-resolved"]).await?;
    run("systemctl", &["start", &format!("wpa_supplicant@{iface}")]).await?;
    Ok(())
}

/// Start the `WiFi` client (`wpa_supplicant`) for the detected interface.
/// Used at boot when `WiFi` is already configured but AP mode was never entered
/// (nothing else brings the supplicant up in that path).
pub async fn start_wifi_client() -> Result<()> {
    let iface = detect_wifi_interface().await;
    tracing::info!("Starting WiFi client on interface {iface}");
    ensure_base_wpa_conf(&iface, DEFAULT_COUNTRY).await?;
    run("systemctl", &["start", &format!("wpa_supplicant@{iface}")]).await
}

/// Ensure a minimal `wpa_supplicant` config exists so the per-interface supplicant
/// can start (and expose its control socket) even before any network is saved.
/// Without this, `wpa_supplicant@<iface>` exits 255 (no config) and scans on an
/// otherwise-idle device return nothing because there is no control socket.
async fn ensure_base_wpa_conf(iface: &str, country: &str) -> Result<()> {
    let path = wpa_conf_path(iface);
    if tokio::fs::try_exists(&path).await.unwrap_or(false) {
        return Ok(());
    }
    let country = sanitize_country(country);
    let base = format!(
        "ctrl_interface=/var/run/wpa_supplicant\nupdate_config=1\ncountry={country}\np2p_disabled=1\n"
    );
    write_config(&path, &base).await
}

/// Minimum/maximum length of a WPA-PSK passphrase, per the 802.11i spec (a 64-hex
/// raw PSK is not accepted here — the setup UI collects a passphrase).
const WPA_PASSPHRASE_LEN: std::ops::RangeInclusive<usize> = 8..=63;

/// Build a `wpa_supplicant` config for one network.
///
/// An EMPTY passphrase means an open network → `key_mgmt=NONE` with no `psk`
/// line. Emitting `psk=""` + `key_mgmt=WPA-PSK` (the old unconditional path) makes
/// `wpa_supplicant` reject the ENTIRE file ("Invalid passphrase length 0"), which
/// leaves the supplicant unable to start for ANY operation — including scanning —
/// until the config is cleared by hand. A non-empty passphrase must be 8..=63
/// characters; anything else is rejected here rather than persisted as a config
/// that bricks Wi-Fi.
fn build_wpa_config(ssid: &str, password: &str, country: &str) -> Result<String> {
    let ssid = wpa_quoted_string("ssid", ssid)?;
    let country = sanitize_country(country);

    // scan_ssid=1 so hidden SSIDs (not in beacons) are probed for and associated.
    let network = if password.is_empty() {
        format!("network={{\n  ssid=\"{ssid}\"\n  scan_ssid=1\n  key_mgmt=NONE\n}}\n")
    } else {
        // Length is checked on the RAW passphrase; wpa_supplicant validates the
        // unescaped value, so escaping must not change the counted length.
        let len = password.chars().count();
        anyhow::ensure!(
            WPA_PASSPHRASE_LEN.contains(&len),
            "WiFi passphrase must be 8–63 characters (got {len})"
        );
        let psk = wpa_quoted_string("password", password)?;
        format!(
            "network={{\n  ssid=\"{ssid}\"\n  scan_ssid=1\n  psk=\"{psk}\"\n  key_mgmt=WPA-PSK\n}}\n"
        )
    };

    Ok(format!(
        "ctrl_interface=/var/run/wpa_supplicant\nupdate_config=1\ncountry={country}\np2p_disabled=1\n\n{network}"
    ))
}

/// Connect to a `WiFi` network. Writes the supplicant + networkd config and then
/// tears the setup AP down on a short delay so the HTTP response reaches the
/// browser first (its link to the AP dies with the teardown). Returns as soon as
/// the config is persisted — association progress is observed via `WifiState`.
pub async fn connect_wifi(
    ssid: &str,
    password: &str,
    country: &str,
    static_ip: Option<&StaticConfig>,
) -> Result<()> {
    let iface = detect_wifi_interface().await;
    tracing::info!("Connecting to WiFi on interface {iface}: {ssid}");
    if let Some(config) = static_ip {
        validate_static_config(config)?;
    }
    let wpa = build_wpa_config(ssid, password, country)?;
    write_config(&wpa_conf_path(&iface), &wpa).await?;

    // Write networkd config for wifi
    let network = static_ip.map_or_else(
        || format!("[Match]\nName={iface}\n\n[Network]\nDHCP=yes\n"),
        |s| {
            format!(
                "[Match]\nName={iface}\n\n[Network]\nAddress={}/{}\nGateway={}\nDNS={}\n",
                s.ip,
                subnet_to_prefix(&s.subnet),
                s.gateway,
                s.dns
            )
        },
    );
    write_config(WIFI_NETWORK, &network).await?;

    // Apply the new network. During setup the AP is up: defer its teardown so the
    // caller's 202 response reaches the browser BEFORE its AP link drops (the
    // teardown then brings the supplicant up on the new config). On the LAN
    // (no AP, e.g. adding WiFi while on ethernet) there is no link to protect and
    // nothing else restarts the supplicant, so apply the new config now.
    if is_ap_active().await {
        tokio::spawn(async move {
            tokio::time::sleep(AP_TEARDOWN_GRACE).await;
            if let Err(e) = stop_ap().await {
                tracing::warn!("deferred AP teardown after connect failed: {e:#}");
            }
        });
    } else {
        run(
            "systemctl",
            &["restart", &format!("wpa_supplicant@{iface}")],
        )
        .await?;
    }
    Ok(())
}

/// Disconnect `WiFi` and remove saved credentials.
pub async fn disconnect_wifi() -> Result<()> {
    let iface = detect_wifi_interface().await;
    tracing::info!("Disconnecting WiFi on interface {iface}");
    let wpa =
        "ctrl_interface=/var/run/wpa_supplicant\nupdate_config=1\ncountry=DE\np2p_disabled=1\n";
    write_config(&wpa_conf_path(&iface), wpa).await?;
    run(
        "systemctl",
        &["restart", &format!("wpa_supplicant@{iface}")],
    )
    .await?;
    // Tear down networkd's addressing for the interface too. Restarting the
    // supplicant drops the association, but the DHCP lease/address networkd
    // assigned lingers on the interface — and `get_wifi()` treats a present IP
    // (or an up operstate) as `connected`. Without this, the status endpoint
    // keeps answering `connected: true` for as long as the stale address
    // survives (indefinitely, since nothing else removes it), so the WebUI
    // never sees the disconnect land. Mirror the AP teardown: remove the
    // profile, then reload + reconfigure so networkd flushes the address it
    // owned. Interface-scoped `reconfigure` leaves ethernet untouched.
    let _ = tokio::fs::remove_file(WIFI_NETWORK).await;
    run("networkctl", &["reload"]).await?;
    run("networkctl", &["reconfigure", &iface]).await?;
    Ok(())
}

/// Scan for available `WiFi` networks. Errors (rather than returning an empty
/// list) so the caller can tell "nothing nearby" from "scan impossible right
/// now" — most importantly the single-radio/AP-mode case, which the caller maps
/// to a distinct status. Ensures a supplicant with a control socket is up first,
/// otherwise `wpa_cli` has nothing to talk to.
pub async fn scan_networks() -> Result<Vec<ScannedNetwork>> {
    anyhow::ensure!(
        !is_ap_active().await,
        "cannot scan while the setup access point is active (single radio)"
    );
    let iface = detect_wifi_interface().await;
    ensure_supplicant_running(&iface).await?;

    // Trigger a fresh scan. A cold scan (radio idle since boot) sweeps every
    // channel and can take several seconds to finish; a single fixed sleep read
    // `scan_results` before the sweep completed, so the FIRST caller-issued scan
    // returned an empty set and only the NEXT call saw the now-populated cache —
    // the frontend does exactly one scan on mount, so it showed "no networks".
    // Poll instead: return as soon as the sweep yields results, up to a deadline.
    let _ = Command::new("wpa_cli")
        .args(["-i", &iface, "scan"])
        .output()
        .await;

    let mut networks = Vec::new();
    let mut saw_results = false;
    let mut last_err = String::new();
    // ~6.4 s worst case (8 × 800 ms); the common warm case returns in ~1 sweep.
    for _ in 0..8 {
        tokio::time::sleep(Duration::from_millis(800)).await;
        let output = Command::new("wpa_cli")
            .args(["-i", &iface, "scan_results"])
            .output()
            .await
            .context("wpa_cli scan_results failed")?;
        if !output.status.success() {
            last_err = String::from_utf8_lossy(&output.stderr).into_owned();
            continue;
        }
        saw_results = true;
        networks = parse_scan_results(&String::from_utf8_lossy(&output.stdout));
        if !networks.is_empty() {
            break;
        }
    }
    // Only error if EVERY read failed; an empty-but-successful result is a valid
    // "no networks in range" answer the caller should surface as such.
    anyhow::ensure!(saw_results, "wpa_cli scan_results failed: {last_err}");
    Ok(networks)
}

/// Parse `wpa_cli scan_results` tab-separated output into deduped networks: one
/// entry per SSID keeping the strongest signal (a network on 2.4+5 GHz or behind
/// multiple APs appears several times), strongest first. Hidden (empty-SSID)
/// beacons are dropped.
fn parse_scan_results(stdout: &str) -> Vec<ScannedNetwork> {
    let mut best: std::collections::HashMap<String, ScannedNetwork> =
        std::collections::HashMap::new();
    for line in stdout.lines().skip(1) {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 5 {
            continue;
        }
        let signal = parts[2].parse::<i32>().unwrap_or(-100);
        let security = parse_security(parts[3]);
        let ssid = parts[4].to_string();
        if ssid.is_empty() {
            continue; // hidden network — beacon carries no SSID
        }
        best.entry(ssid.clone())
            .and_modify(|n| {
                if signal > n.signal {
                    n.signal = signal;
                    n.security.clone_from(&security);
                }
            })
            .or_insert(ScannedNetwork {
                ssid,
                signal,
                security,
            });
    }
    let mut networks: Vec<ScannedNetwork> = best.into_values().collect();
    networks.sort_by_key(|n| std::cmp::Reverse(n.signal));
    networks
}

/// Map a `wpa_supplicant` `scan_results` flags field to a coarse security label the
/// UI renders (lock icon + "WPA3"/"WPA2"/"Open"). Order matters: WPA3 (SAE)
/// before WPA2 before WPA.
fn parse_security(flags: &str) -> String {
    let f = flags.to_ascii_uppercase();
    if f.contains("SAE") || f.contains("WPA3") {
        "wpa3".into()
    } else if f.contains("WPA2") || f.contains("RSN") {
        "wpa2".into()
    } else if f.contains("WPA") {
        "wpa".into()
    } else if f.contains("WEP") {
        "wep".into()
    } else {
        "open".into()
    }
}

/// Ensure the per-interface supplicant is running so `wpa_cli` has a control
/// socket. Idempotent; only acts when not in AP mode.
async fn ensure_supplicant_running(iface: &str) -> Result<()> {
    let active = Command::new("systemctl")
        .args(["is-active", "--quiet", &format!("wpa_supplicant@{iface}")])
        .status()
        .await
        .is_ok_and(|s| s.success());
    if active {
        return Ok(());
    }
    ensure_base_wpa_conf(iface, DEFAULT_COUNTRY).await?;
    run("systemctl", &["start", &format!("wpa_supplicant@{iface}")]).await?;
    // Give the control socket a moment to appear.
    tokio::time::sleep(Duration::from_millis(800)).await;
    Ok(())
}

/// Configure ethernet (DHCP or static).
pub async fn configure_ethernet(static_ip: Option<&StaticConfig>) -> Result<()> {
    if let Some(config) = static_ip {
        validate_static_config(config)?;
    }

    let iface_list = detect_ethernet_interfaces().await;
    let ifaces = iface_list.join(" ");
    let network = static_ip.map_or_else(
        || format!("[Match]\nName={ifaces}\n\n[Network]\nDHCP=yes\n"),
        |s| {
            format!(
                "[Match]\nName={ifaces}\n\n[Network]\nAddress={}/{}\nGateway={}\nDNS={}\n",
                s.ip,
                subnet_to_prefix(&s.subnet),
                s.gateway,
                s.dns
            )
        },
    );
    write_config(ETH_NETWORK_PATH, &network).await?;
    // Apply without a full networkd restart (won't disturb other interfaces).
    run("networkctl", &["reload"]).await?;
    for iface in &iface_list {
        run("networkctl", &["reconfigure", iface]).await?;
    }
    Ok(())
}

/// Configure systemd-resolved (disable stub resolver).
pub async fn configure_resolved() -> Result<()> {
    // Stop resolved so the in-process captive DNS responder can bind :53 in AP mode
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

fn derive_ssid(mac: &str) -> String {
    let clean = mac.trim().replace(':', "");
    if clean.len() != 12 || !clean.chars().all(|c| c.is_ascii_hexdigit()) {
        return "SnapDog-Setup".to_string();
    }
    let suffix: String = clean
        .chars()
        .rev()
        .take(4)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("SnapDog-{}", suffix.to_uppercase())
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

    #[test]
    fn test_derive_ssid() {
        assert_eq!(derive_ssid("b8:27:eb:1a:2b:3c"), "SnapDog-2B3C");
        assert_eq!(derive_ssid("  B8:27:EB:1A:2B:3C\n"), "SnapDog-2B3C");
        assert_eq!(derive_ssid(""), "SnapDog-Setup");
        assert_eq!(derive_ssid("12"), "SnapDog-Setup");
        assert_eq!(derive_ssid("not-a-mac-address"), "SnapDog-Setup");
    }

    #[test]
    fn parse_security_ranks_strongest_first() {
        assert_eq!(parse_security("[WPA2-PSK-CCMP][WPS][ESS]"), "wpa2");
        assert_eq!(parse_security("[WPA2-SAE-CCMP][ESS]"), "wpa3");
        assert_eq!(parse_security("[WPA-PSK-TKIP][ESS]"), "wpa");
        assert_eq!(parse_security("[WEP][ESS]"), "wep");
        assert_eq!(parse_security("[ESS]"), "open");
    }

    #[test]
    fn parse_scan_results_dedupes_and_sorts() {
        // Header line + duplicate SSID across bands + a hidden (empty-SSID) beacon.
        let raw = "bssid / frequency / signal level / flags / ssid\n\
            aa:aa:aa:aa:aa:aa\t2412\t-70\t[WPA2-PSK-CCMP][ESS]\tHome\n\
            bb:bb:bb:bb:bb:bb\t5180\t-45\t[WPA2-PSK-CCMP][ESS]\tHome\n\
            cc:cc:cc:cc:cc:cc\t2437\t-60\t[ESS]\tGuest\n\
            dd:dd:dd:dd:dd:dd\t2462\t-80\t[WPA2-PSK-CCMP][ESS]\t";
        let nets = parse_scan_results(raw);
        // Home deduped to its strongest (-45), hidden dropped → 2 networks.
        assert_eq!(nets.len(), 2);
        assert_eq!(nets[0].ssid, "Home"); // strongest first
        assert_eq!(nets[0].signal, -45);
        assert_eq!(nets[0].security, "wpa2");
        assert_eq!(nets[1].ssid, "Guest");
        assert_eq!(nets[1].security, "open");
    }

    #[test]
    fn parse_scan_results_empty_is_empty() {
        // Header only (no APs) must yield an empty vec, not a spurious entry.
        assert!(parse_scan_results("bssid / frequency / signal level / flags / ssid\n").is_empty());
        assert!(parse_scan_results("").is_empty());
    }

    #[test]
    fn sanitize_country_validates() {
        assert_eq!(sanitize_country("de"), "DE");
        assert_eq!(sanitize_country("US"), "US");
        assert_eq!(sanitize_country("bad"), "DE");
        assert_eq!(sanitize_country("D\nE"), "DE");
        assert_eq!(sanitize_country(""), "DE");
    }

    #[test]
    fn build_wpa_config_open_network_uses_key_mgmt_none() {
        // Empty passphrase => open network. Must NOT emit psk="" — that makes
        // wpa_supplicant reject the whole file and breaks every operation (scans too).
        let cfg = build_wpa_config("CoffeeShop", "", "DE").unwrap();
        assert!(cfg.contains("key_mgmt=NONE"));
        assert!(!cfg.contains("psk="));
        assert!(cfg.contains("ssid=\"CoffeeShop\""));
    }

    #[test]
    fn build_wpa_config_secured_network_writes_psk() {
        let cfg = build_wpa_config("Home", "supersecret", "DE").unwrap();
        assert!(cfg.contains("psk=\"supersecret\""));
        assert!(cfg.contains("key_mgmt=WPA-PSK"));
        assert!(!cfg.contains("psk=\"\""));
    }

    #[test]
    fn build_wpa_config_rejects_out_of_range_passphrase() {
        assert!(build_wpa_config("Home", "short", "DE").is_err()); // 5 chars
        assert!(build_wpa_config("Home", &"x".repeat(64), "DE").is_err());
        // Boundaries are valid.
        assert!(build_wpa_config("Home", "12345678", "DE").is_ok()); // 8
        assert!(build_wpa_config("Home", &"y".repeat(63), "DE").is_ok()); // 63
    }
}
