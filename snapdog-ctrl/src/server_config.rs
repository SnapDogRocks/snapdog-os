// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! Read/write/validate `snapdog.toml` using `toml_edit` to preserve comments.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use toml_edit::{ArrayOfTables, DocumentMut, Item, Table};

const CONFIG_PATH: &str = "/etc/snapdog/snapdog.toml";
const CONFIG_BACKUP: &str = "/etc/snapdog/snapdog.toml.bak";
const CONFIG_CANDIDATE: &str = "/etc/snapdog/.snapdog.toml.candidate";
const SNAPDOG_BINARY: &str = "/usr/bin/snapdog";
const SERVICE_NAME: &str = "snapdog";
static CONFIG_APPLY_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Complete server configuration as exposed via the API.
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct ServerConfig {
    /// Revision of the source TOML. Prevents overwriting a concurrent edit.
    #[serde(default)]
    pub revision: String,
    /// Complete source TOML for advanced, forward-compatible editing.
    #[serde(default)]
    pub raw_toml: String,
    /// Client hint for the advanced editor; never emitted by the read API.
    #[serde(default, skip_serializing)]
    pub raw_toml_changed: bool,
    pub name: String,
    pub http: HttpConfig,
    pub audio: AudioConfig,
    pub snapcast: SnapcastConfig,
    pub mdns: MdnsConfig,
    pub dbus: DbusConfig,
    pub subsonic: Option<SubsonicConfig>,
    pub spotify: Option<SpotifyConfig>,
    pub airplay: Option<AirplayConfig>,
    pub mqtt: Option<MqttConfig>,
    pub knx: Option<KnxConfig>,
    pub zones: Vec<ZoneConfig>,
    pub clients: Vec<ClientEntry>,
    pub radio: Vec<RadioStation>,
    pub system: SystemConfig,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct HttpConfig {
    pub port: u16,
    pub bind: String,
    pub base_url: String,
    pub tls_cert: Option<String>,
    pub tls_key: Option<String>,
    pub api_keys: Vec<String>,
    pub api_docs: bool,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            port: 5555,
            bind: "::".into(),
            base_url: "http://localhost:5555".into(),
            tls_cert: None,
            tls_key: None,
            api_keys: Vec::new(),
            api_docs: true,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct AudioConfig {
    pub sample_rate: u32,
    pub bit_depth: u8,
    pub channels: u8,
    pub source_conflict: String,
    pub zone_switch_fade_ms: u16,
    pub source_switch_fade_ms: u16,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            sample_rate: 48000,
            bit_depth: 32,
            channels: 2,
            source_conflict: "last_wins".into(),
            zone_switch_fade_ms: 300,
            source_switch_fade_ms: 300,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SnapcastConfig {
    pub address: String,
    pub jsonrpc_tcp_port: u16,
    pub streaming_port: u16,
    pub managed: bool,
    pub verbose: bool,
    pub codec: String,
    pub encryption_psk: Option<String>,
    pub group_volume_mode: String,
    pub unknown_clients: String,
    pub default_zone: Option<String>,
}

impl Default for SnapcastConfig {
    fn default() -> Self {
        Self {
            address: "127.0.0.1".into(),
            jsonrpc_tcp_port: 1705,
            streaming_port: 1704,
            managed: true,
            verbose: false,
            codec: "f32lz4".into(),
            encryption_psk: None,
            group_volume_mode: "compressed".into(),
            unknown_clients: "accept".into(),
            default_zone: None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct MdnsConfig {
    pub enabled: bool,
    pub advertise_snapcast: bool,
}

impl Default for MdnsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            advertise_snapcast: false,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct DbusConfig {
    pub enabled: bool,
}

impl Default for DbusConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SubsonicConfig {
    pub url: String,
    pub username: String,
    pub password: String,
    pub format: String,
    pub tls_skip_verify: bool,
    pub cache: SubsonicCacheConfig,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SubsonicCacheConfig {
    pub path: String,
    pub max_size_mb: u64,
}

impl Default for SubsonicCacheConfig {
    fn default() -> Self {
        Self {
            path: String::new(),
            max_size_mb: 2048,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SpotifyConfig {
    pub name: String,
    pub bitrate: u16,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct AirplayConfig {
    pub password: Option<String>,
    pub mode: String,
    pub bind: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct MqttConfig {
    pub broker: String,
    pub client_id: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub base_topic: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct KnxConfig {
    pub role: String,
    pub url: Option<String>,
    pub individual_address: Option<String>,
    pub persist_ets_config: Option<bool>,
    pub restart_after_ets: Option<bool>,
    pub start_prog_mode: bool,
    pub server_online: Option<String>,
    pub all_stop: Option<String>,
    pub all_mute: Option<String>,
    pub all_mute_status: Option<String>,
    pub system_fault: Option<String>,
    pub knx_time: Option<String>,
    pub heartbeat_minutes: u16,
    pub sync_system_clock: bool,
}

impl Default for KnxConfig {
    fn default() -> Self {
        Self {
            role: "client".into(),
            url: None,
            individual_address: None,
            persist_ets_config: None,
            restart_after_ets: None,
            start_prog_mode: false,
            server_online: None,
            all_stop: None,
            all_mute: None,
            all_mute_status: None,
            system_fault: None,
            knx_time: None,
            heartbeat_minutes: 5,
            sync_system_clock: false,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ZoneConfig {
    /// Original array index used only to preserve unknown TOML fields across edits.
    #[serde(default)]
    pub source_index: Option<usize>,
    pub name: String,
    pub icon: String,
    pub sink: Option<String>,
    pub airplay_name: Option<String>,
    pub spotify_name: Option<String>,
    pub group_volume_mode: Option<String>,
    pub knx: Option<KnxGroupObjects>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct KnxGroupObjects {
    pub play: Option<String>,
    pub pause: Option<String>,
    pub stop: Option<String>,
    pub track_next: Option<String>,
    pub track_previous: Option<String>,
    pub control_status: Option<String>,
    pub volume: Option<String>,
    pub volume_status: Option<String>,
    pub volume_dim: Option<String>,
    pub mute: Option<String>,
    pub mute_status: Option<String>,
    pub mute_toggle: Option<String>,
    pub track_title_status: Option<String>,
    pub track_artist_status: Option<String>,
    pub track_album_status: Option<String>,
    pub track_progress_status: Option<String>,
    pub track_playing_status: Option<String>,
    pub track_repeat: Option<String>,
    pub track_repeat_status: Option<String>,
    pub track_repeat_toggle: Option<String>,
    pub playlist: Option<String>,
    pub playlist_status: Option<String>,
    pub playlist_next: Option<String>,
    pub playlist_previous: Option<String>,
    pub shuffle: Option<String>,
    pub shuffle_status: Option<String>,
    pub shuffle_toggle: Option<String>,
    pub repeat: Option<String>,
    pub repeat_status: Option<String>,
    pub repeat_toggle: Option<String>,
    pub presence: Option<String>,
    pub presence_enable: Option<String>,
    pub presence_enable_status: Option<String>,
    pub presence_timer_status: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ClientEntry {
    /// Original array index used only to preserve unknown TOML fields across edits.
    #[serde(default)]
    pub source_index: Option<usize>,
    pub name: String,
    pub mac: String,
    pub zone: String,
    pub icon: String,
    pub max_volume: u8,
    pub default_volume: u8,
    pub default_latency: i32,
    pub knx: Option<ClientKnxGOs>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct ClientKnxGOs {
    pub volume: Option<String>,
    pub volume_status: Option<String>,
    pub volume_dim: Option<String>,
    pub mute: Option<String>,
    pub mute_status: Option<String>,
    pub mute_toggle: Option<String>,
    pub latency: Option<String>,
    pub latency_status: Option<String>,
    pub zone: Option<String>,
    pub zone_status: Option<String>,
    pub connected_status: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct RadioStation {
    /// Original array index used only to preserve unknown TOML fields across edits.
    #[serde(default)]
    pub source_index: Option<usize>,
    pub name: String,
    pub url: String,
    pub cover: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SystemConfig {
    pub log_level: String,
    pub log_file: Option<String>,
    pub state_dir: String,
}

impl Default for SystemConfig {
    fn default() -> Self {
        Self {
            log_level: "info".into(),
            log_file: None,
            state_dir: "/var/lib/snapdog".into(),
        }
    }
}

/// Read the server config, parsing it into our struct.
pub async fn read_config() -> Result<ServerConfig> {
    let content = match tokio::fs::read_to_string(CONFIG_PATH).await {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(error).context("failed to read snapdog.toml");
        }
    };

    if content.is_empty() {
        return Ok(ServerConfig {
            revision: config_revision(""),
            raw_toml: String::new(),
            ..ServerConfig::default()
        });
    }

    let doc: DocumentMut = content.parse().context("failed to parse snapdog.toml")?;
    let mut config = parse_document(&doc);
    config.revision = config_revision(&content);
    config.raw_toml = content;
    Ok(config)
}

/// Validate, atomically install, restart, and health-check a server configuration.
///
/// Any failure after activation restores the previous file and restarts the old
/// configuration before returning the error to the caller.
pub async fn apply_and_restart(config: &ServerConfig) -> Result<()> {
    let _guard = CONFIG_APPLY_LOCK.lock().await;
    if !uses_advanced_toml(config) {
        validate(config)?;
    }

    let previous = tokio::fs::read_to_string(CONFIG_PATH).await.ok();
    let source = previous.as_deref().unwrap_or("");
    ensure_current_revision(config, source)?;

    let candidate = render_candidate(source, config)?;
    tokio::fs::create_dir_all("/etc/snapdog").await?;
    durable_atomic_write(CONFIG_CANDIDATE, &candidate)
        .await
        .context("failed to stage server configuration")?;

    let validation = validate_with_server(CONFIG_CANDIDATE).await;
    if let Err(error) = validation {
        let _ = tokio::fs::remove_file(CONFIG_CANDIDATE).await;
        return Err(error);
    }

    if let Some(old) = &previous {
        durable_atomic_write(CONFIG_BACKUP, old)
            .await
            .context("failed to create server configuration backup")?;
    }
    durable_atomic_write(CONFIG_PATH, &candidate)
        .await
        .context("failed to activate server configuration")?;
    let _ = tokio::fs::remove_file(CONFIG_CANDIDATE).await;

    if let Err(apply_error) = restart_and_wait_healthy().await {
        let rollback_result = rollback(previous.as_deref()).await;
        return match rollback_result {
            Ok(()) => Err(apply_error.context(
                "new configuration was rejected at runtime; the previous configuration was restored",
            )),
            Err(rollback_error) => Err(anyhow::anyhow!(
                "new configuration failed: {apply_error:#}; rollback also failed: {rollback_error:#}"
            )),
        };
    }

    Ok(())
}

fn render_candidate(source: &str, config: &ServerConfig) -> Result<String> {
    if uses_advanced_toml(config) {
        config
            .raw_toml
            .parse::<DocumentMut>()
            .context("advanced TOML is invalid")?;
        return Ok(config.raw_toml.clone());
    }
    let mut doc = if source.is_empty() {
        DocumentMut::new()
    } else {
        source
            .parse::<DocumentMut>()
            .context("refusing to overwrite an invalid snapdog.toml")?
    };
    apply_config(&mut doc, config);
    Ok(doc.to_string())
}

async fn validate_with_server(path: &str) -> Result<()> {
    let output = tokio::process::Command::new(SNAPDOG_BINARY)
        .args(["--config", path, "--check-config"])
        .output()
        .await
        .context("failed to execute the SnapDog configuration guard")?;
    anyhow::ensure!(
        output.status.success(),
        "SnapDog rejected the configuration: {}",
        command_error(&output)
    );
    Ok(())
}

async fn restart_and_wait_healthy() -> Result<()> {
    run_systemctl(&["restart", SERVICE_NAME]).await?;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        if run_systemctl(&["is-active", "--quiet", SERVICE_NAME])
            .await
            .is_ok()
        {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            if run_systemctl(&["is-active", "--quiet", SERVICE_NAME])
                .await
                .is_ok()
            {
                return Ok(());
            }
        }
        anyhow::ensure!(
            tokio::time::Instant::now() < deadline,
            "SnapDog did not become healthy after restart"
        );
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
}

async fn rollback(previous: Option<&str>) -> Result<()> {
    if let Some(content) = previous {
        durable_atomic_write(CONFIG_PATH, content).await?;
    } else if tokio::fs::try_exists(CONFIG_PATH).await? {
        tokio::fs::remove_file(CONFIG_PATH).await?;
        #[cfg(unix)]
        tokio::fs::File::open("/etc/snapdog")
            .await?
            .sync_all()
            .await?;
    }
    restart_and_wait_healthy().await
}

async fn durable_atomic_write(path: &str, content: &str) -> Result<()> {
    crate::system::atomic_write(path, content).await?;
    #[cfg(unix)]
    if let Some(parent) = std::path::Path::new(path).parent() {
        tokio::fs::File::open(parent).await?.sync_all().await?;
    }
    Ok(())
}

async fn run_systemctl(args: &[&str]) -> Result<()> {
    let output = tokio::process::Command::new("systemctl")
        .args(args)
        .output()
        .await?;
    anyhow::ensure!(
        output.status.success(),
        "systemctl {} failed: {}",
        args.join(" "),
        command_error(&output)
    );
    Ok(())
}

fn command_error(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if stderr.is_empty() {
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    } else {
        stderr
    }
}

fn config_revision(content: &str) -> String {
    format!("{:x}", Sha256::digest(content.as_bytes()))
}

fn ensure_current_revision(config: &ServerConfig, source: &str) -> Result<()> {
    anyhow::ensure!(
        !config.revision.is_empty() && config.revision == config_revision(source),
        "The server configuration changed since it was loaded. Reload it before saving."
    );
    Ok(())
}

/// Whether the complete TOML editor changed the source document.
#[must_use]
pub fn uses_advanced_toml(config: &ServerConfig) -> bool {
    config.raw_toml_changed
        || (!config.raw_toml.is_empty() && config_revision(&config.raw_toml) != config.revision)
}

/// Validate config before writing.
pub fn validate(config: &ServerConfig) -> Result<()> {
    anyhow::ensure!(
        [44100, 48000, 88200, 96000, 176_400, 192_000].contains(&config.audio.sample_rate),
        "Invalid sample rate"
    );
    anyhow::ensure!(
        [16, 24, 32].contains(&config.audio.bit_depth),
        "Invalid bit depth"
    );
    anyhow::ensure!(
        ["pcm", "flac", "f32lz4", "f32lz4e"].contains(&config.snapcast.codec.as_str()),
        "Invalid codec"
    );
    anyhow::ensure!(
        (1..=8).contains(&config.audio.channels),
        "channels must be 1-8"
    );
    anyhow::ensure!(
        ["last_wins", "receiver_wins"].contains(&config.audio.source_conflict.as_str()),
        "Invalid source_conflict"
    );
    anyhow::ensure!(
        ["relative", "absolute", "compressed"]
            .contains(&config.snapcast.group_volume_mode.as_str()),
        "Invalid group_volume_mode"
    );
    anyhow::ensure!(
        ["accept", "ignore", "reject"].contains(&config.snapcast.unknown_clients.as_str()),
        "Invalid unknown_clients"
    );
    anyhow::ensure!(
        config.audio.zone_switch_fade_ms <= 1000,
        "zone_switch_fade_ms must be 0-1000"
    );
    anyhow::ensure!(
        config.audio.source_switch_fade_ms <= 1000,
        "source_switch_fade_ms must be 0-1000"
    );
    anyhow::ensure!(
        ["trace", "debug", "info", "warn", "error"].contains(&config.system.log_level.as_str()),
        "Invalid log_level"
    );
    anyhow::ensure!(
        config.http.port != config.snapcast.streaming_port,
        "HTTP and Snapcast streaming ports must be different"
    );
    anyhow::ensure!(
        config.http.tls_cert.is_some() == config.http.tls_key.is_some(),
        "TLS certificate and private key must be configured together"
    );

    validate_integrations(config)?;
    validate_topology(config)
}

fn validate_integrations(config: &ServerConfig) -> Result<()> {
    if let Some(knx) = &config.knx {
        anyhow::ensure!(
            ["client", "device"].contains(&knx.role.as_str()),
            "Invalid KNX role"
        );
        if knx.role == "client" {
            let url = knx.url.as_deref().unwrap_or_default().trim();
            anyhow::ensure!(!url.is_empty(), "KNX gateway URL required in client mode");
            anyhow::ensure!(
                !url.contains('\n') && !url.contains('\r'),
                "KNX gateway URL must be a single line"
            );
        }
    }

    for station in &config.radio {
        anyhow::ensure!(!station.name.is_empty(), "Radio station name required");
        anyhow::ensure!(!station.url.is_empty(), "Radio station URL required");
    }

    Ok(())
}

fn validate_topology(config: &ServerConfig) -> Result<()> {
    anyhow::ensure!(!config.zones.is_empty(), "At least one zone is required");
    let mut zone_names = std::collections::HashSet::new();
    for zone in &config.zones {
        anyhow::ensure!(!zone.name.is_empty(), "Zone name required");
        anyhow::ensure!(
            zone_names.insert(zone.name.as_str()),
            "Duplicate zone name: '{}'",
            zone.name
        );
        if let Some(knx) = &zone.knx {
            validate_zone_knx(&zone.name, knx)?;
        }
        if let Some(mode) = zone.group_volume_mode.as_deref() {
            anyhow::ensure!(
                ["relative", "absolute", "compressed"].contains(&mode),
                "Invalid group_volume_mode for zone '{}'",
                zone.name
            );
        }
    }

    let mut client_names = std::collections::HashSet::new();
    let mut client_macs = std::collections::HashSet::new();
    for client in &config.clients {
        anyhow::ensure!(!client.name.is_empty(), "Client name required");
        anyhow::ensure!(!client.mac.is_empty(), "Client MAC required");
        anyhow::ensure!(client.max_volume <= 100, "Client max_volume must be 0-100");
        anyhow::ensure!(
            client.default_volume <= 100,
            "Client default_volume must be 0-100"
        );
        anyhow::ensure!(
            client_names.insert(client.name.as_str()),
            "Duplicate client name: '{}'",
            client.name
        );
        let normalized_mac = client.mac.to_ascii_lowercase();
        anyhow::ensure!(
            valid_mac_address(&normalized_mac),
            "Invalid client MAC address: '{}'",
            client.mac
        );
        anyhow::ensure!(
            client_macs.insert(normalized_mac),
            "Duplicate client MAC address: '{}'",
            client.mac
        );
        anyhow::ensure!(
            zone_names.contains(client.zone.as_str()),
            "Client '{}' references unknown zone '{}'",
            client.name,
            client.zone
        );
        if let Some(knx) = &client.knx {
            validate_client_knx(&client.name, knx)?;
        }
    }

    if config.snapcast.unknown_clients != "accept" {
        anyhow::ensure!(
            !config.clients.is_empty(),
            "At least one client is required unless unknown clients are accepted"
        );
    }
    if let Some(default_zone) = config.snapcast.default_zone.as_deref() {
        anyhow::ensure!(
            zone_names.contains(default_zone),
            "Default zone '{default_zone}' does not exist"
        );
    }

    Ok(())
}

fn valid_mac_address(value: &str) -> bool {
    let parts: Vec<&str> = value.split(':').collect();
    parts.len() == 6
        && parts
            .iter()
            .all(|part| part.len() == 2 && part.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Generate a default config file.
pub fn default_config_toml() -> String {
    r#"# SnapDog Server Configuration
# Managed by snapdog-ctrl — do not edit manually.

[system]
log_level = "info"

[audio]
sample_rate = 48000
bit_depth = 32
channels = 2
source_conflict = "last_wins"
zone_switch_fade_ms = 300
source_switch_fade_ms = 300

[snapcast]
streaming_port = 1704
codec = "f32lz4"
group_volume_mode = "relative"
unknown_clients = "accept"

[mdns]
enabled = true
advertise_snapcast = false

[[zone]]
name = "Default Zone"
icon = "🔊"
"#
    .to_string()
}

// ── Internal ──────────────────────────────────────────────────

// Sequential field-by-field parsing of a flat TOML structure — splitting would reduce readability.
#[allow(clippy::too_many_lines)]
fn parse_document(doc: &DocumentMut) -> ServerConfig {
    let mut config = ServerConfig::default();

    if let Some(system) = doc.get("system").and_then(Item::as_table) {
        config.system.log_level = get_str(system, "log_level", "info");
        config.system.log_file = get_optional_str(system, "log_file");
        config.system.state_dir = get_str(system, "state_dir", "/var/lib/snapdog");
    }

    if let Some(http) = doc.get("http").and_then(Item::as_table) {
        config.http.port = get_u16(http, "port", 5555);
        config.http.bind = get_str(http, "bind", "::");
        config.http.base_url = get_str(http, "base_url", "http://localhost:5555");
        config.http.tls_cert = get_optional_str(http, "tls_cert");
        config.http.tls_key = get_optional_str(http, "tls_key");
        config.http.api_docs = get_bool(http, "api_docs", true);
        config.http.api_keys = get_string_array(http, "api_keys");
    }

    if let Some(audio) = doc.get("audio").and_then(Item::as_table) {
        config.audio.sample_rate = get_u32(audio, "sample_rate", 48000);
        config.audio.bit_depth = get_u8(audio, "bit_depth", 16);
        config.audio.channels = get_u8(audio, "channels", 2);
        config.audio.source_conflict = get_str(audio, "source_conflict", "last_wins");
        config.audio.zone_switch_fade_ms = get_u16(audio, "zone_switch_fade_ms", 300);
        config.audio.source_switch_fade_ms = get_u16(audio, "source_switch_fade_ms", 300);
    }

    if let Some(snap) = doc.get("snapcast").and_then(Item::as_table) {
        config.snapcast.address = get_str(snap, "address", "127.0.0.1");
        config.snapcast.jsonrpc_tcp_port =
            get_u16_alias(snap, "jsonrpc_tcp_port", "jsonrpc_port", 1705);
        config.snapcast.streaming_port = get_u16(snap, "streaming_port", 1704);
        config.snapcast.managed = get_bool(snap, "managed", true);
        config.snapcast.verbose = get_bool(snap, "verbose", false);
        config.snapcast.codec = get_str(snap, "codec", "flac");
        config.snapcast.encryption_psk = snap
            .get("encryption_psk")
            .and_then(|v| v.as_str())
            .map(String::from);
        config.snapcast.group_volume_mode = get_str(snap, "group_volume_mode", "relative");
        config.snapcast.unknown_clients = get_str(snap, "unknown_clients", "accept");
        config.snapcast.default_zone = get_optional_str(snap, "default_zone");
    }
    if let Some(mdns) = doc.get("mdns").and_then(Item::as_table) {
        config.mdns.enabled = mdns
            .get("enabled")
            .and_then(toml_edit::Item::as_bool)
            .unwrap_or(true);
        config.mdns.advertise_snapcast = mdns
            .get("advertise_snapcast")
            .and_then(toml_edit::Item::as_bool)
            .unwrap_or(false);
    }
    if let Some(dbus) = doc.get("dbus").and_then(Item::as_table) {
        config.dbus.enabled = get_bool(dbus, "enabled", true);
    }

    // Top-level name
    config.name = doc
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("SnapDog")
        .to_string();

    if let Some(sub) = doc.get("subsonic").and_then(Item::as_table) {
        let cache = sub.get("cache").and_then(Item::as_table);
        config.subsonic = Some(SubsonicConfig {
            url: get_str(sub, "url", ""),
            username: get_str(sub, "username", ""),
            password: get_str(sub, "password", ""),
            format: get_str(sub, "format", "raw"),
            tls_skip_verify: get_bool(sub, "tls_skip_verify", false),
            cache: SubsonicCacheConfig {
                path: cache.map_or_else(String::new, |table| get_str(table, "path", "")),
                max_size_mb: cache.map_or(2048, |table| get_u64(table, "max_size_mb", 2048)),
            },
        });
    }

    if let Some(spot) = doc.get("spotify").and_then(Item::as_table) {
        config.spotify = Some(SpotifyConfig {
            name: get_str(spot, "name", "SnapDog"),
            bitrate: get_u16(spot, "bitrate", 320),
        });
    }

    if let Some(air) = doc.get("airplay").and_then(Item::as_table) {
        config.airplay = Some(AirplayConfig {
            password: air
                .get("password")
                .and_then(|v| v.as_str())
                .map(String::from),
            mode: get_str(air, "mode", "airplay2"),
            bind: get_string_array(air, "bind"),
        });
    }

    if let Some(mqtt) = doc.get("mqtt").and_then(Item::as_table) {
        config.mqtt = Some(MqttConfig {
            broker: get_str(mqtt, "broker", ""),
            client_id: get_str(mqtt, "client_id", "snapdog"),
            username: mqtt
                .get("username")
                .and_then(|v| v.as_str())
                .map(String::from),
            password: mqtt
                .get("password")
                .and_then(|v| v.as_str())
                .map(String::from),
            base_topic: get_str(mqtt, "base_topic", "snapdog"),
        });
    }

    if let Some(knx) = doc.get("knx").and_then(Item::as_table) {
        config.knx = Some(KnxConfig {
            role: get_str(knx, "role", "client"),
            url: knx.get("url").and_then(|v| v.as_str()).map(String::from),
            individual_address: get_optional_str(knx, "individual_address"),
            persist_ets_config: get_optional_bool(knx, "persist_ets_config"),
            restart_after_ets: get_optional_bool(knx, "restart_after_ets"),
            start_prog_mode: get_bool(knx, "start_prog_mode", false),
            server_online: get_optional_str(knx, "server_online"),
            all_stop: get_optional_str(knx, "all_stop"),
            all_mute: get_optional_str(knx, "all_mute"),
            all_mute_status: get_optional_str(knx, "all_mute_status"),
            system_fault: get_optional_str(knx, "system_fault"),
            knx_time: get_optional_str(knx, "knx_time"),
            heartbeat_minutes: get_u16(knx, "heartbeat_minutes", 5),
            sync_system_clock: get_bool(knx, "sync_system_clock", false),
        });
    }

    if let Some(zones) = doc.get("zone").and_then(Item::as_array_of_tables) {
        for (source_index, zone) in zones.iter().enumerate() {
            config.zones.push(ZoneConfig {
                source_index: Some(source_index),
                name: get_str(zone, "name", ""),
                icon: get_str(zone, "icon", "🏠"),
                sink: get_optional_str(zone, "sink"),
                airplay_name: get_optional_str(zone, "airplay_name"),
                spotify_name: get_optional_str(zone, "spotify_name"),
                group_volume_mode: get_optional_str(zone, "group_volume_mode"),
                knx: zone.get("knx").and_then(Item::as_table).map(parse_zone_knx),
            });
        }
    }

    if let Some(clients) = doc.get("client").and_then(Item::as_array_of_tables) {
        for (source_index, client) in clients.iter().enumerate() {
            config.clients.push(ClientEntry {
                source_index: Some(source_index),
                name: get_str(client, "name", ""),
                mac: get_str(client, "mac", ""),
                zone: get_str(client, "zone", ""),
                icon: get_str(client, "icon", "🔊"),
                max_volume: get_u8(client, "max_volume", 100),
                default_volume: get_u8(client, "default_volume", 50),
                default_latency: get_i32(client, "default_latency", 0),
                knx: client
                    .get("knx")
                    .and_then(Item::as_table)
                    .map(parse_client_knx),
            });
        }
    }

    if let Some(radios) = doc.get("radio").and_then(Item::as_array_of_tables) {
        for (source_index, radio) in radios.iter().enumerate() {
            config.radio.push(RadioStation {
                source_index: Some(source_index),
                name: get_str(radio, "name", ""),
                url: get_str(radio, "url", ""),
                cover: radio
                    .get("cover")
                    .and_then(|v| v.as_str())
                    .map(String::from),
            });
        }
    }

    config
}

fn apply_config(doc: &mut DocumentMut, config: &ServerConfig) {
    apply_config_sections(doc, config);
    apply_config_arrays(doc, config);
}

fn apply_config_sections(doc: &mut DocumentMut, config: &ServerConfig) {
    // Name
    set_item_preserving_decor(doc.as_table_mut(), "name", toml_edit::value(&config.name));

    // HTTP
    set_table_u32(doc, "http", "port", u32::from(config.http.port));
    set_table_str(doc, "http", "bind", &config.http.bind);
    set_table_str(doc, "http", "base_url", &config.http.base_url);
    set_optional_table_str(doc, "http", "tls_cert", config.http.tls_cert.as_deref());
    set_optional_table_str(doc, "http", "tls_key", config.http.tls_key.as_deref());
    set_table_bool(doc, "http", "api_docs", config.http.api_docs);
    if !config.http.api_keys.is_empty() {
        let http = doc
            .entry("http")
            .or_insert_with(|| Item::Table(toml_edit::Table::new()));
        let arr: toml_edit::Array = config.http.api_keys.iter().map(String::as_str).collect();
        if let Some(http) = http.as_table_mut() {
            set_item_preserving_decor(http, "api_keys", toml_edit::value(arr));
        }
    } else if let Some(http) = doc.get_mut("http").and_then(|t| t.as_table_mut()) {
        http.remove("api_keys");
    }

    // System
    set_table_str(doc, "system", "log_level", &config.system.log_level);
    set_optional_table_str(doc, "system", "log_file", config.system.log_file.as_deref());
    set_table_str(doc, "system", "state_dir", &config.system.state_dir);

    // Audio
    set_table_u32(doc, "audio", "sample_rate", config.audio.sample_rate);
    set_table_u32(doc, "audio", "bit_depth", u32::from(config.audio.bit_depth));
    set_table_u32(doc, "audio", "channels", u32::from(config.audio.channels));
    set_table_str(
        doc,
        "audio",
        "source_conflict",
        &config.audio.source_conflict,
    );
    set_table_u32(
        doc,
        "audio",
        "zone_switch_fade_ms",
        u32::from(config.audio.zone_switch_fade_ms),
    );
    set_table_u32(
        doc,
        "audio",
        "source_switch_fade_ms",
        u32::from(config.audio.source_switch_fade_ms),
    );

    // Snapcast
    set_table_str(doc, "snapcast", "address", &config.snapcast.address);
    set_table_u32(
        doc,
        "snapcast",
        "jsonrpc_tcp_port",
        u32::from(config.snapcast.jsonrpc_tcp_port),
    );
    set_table_u32(
        doc,
        "snapcast",
        "streaming_port",
        u32::from(config.snapcast.streaming_port),
    );
    set_table_str(doc, "snapcast", "codec", &config.snapcast.codec);
    set_table_bool(doc, "snapcast", "managed", config.snapcast.managed);
    set_table_bool(doc, "snapcast", "verbose", config.snapcast.verbose);
    set_table_str(
        doc,
        "snapcast",
        "group_volume_mode",
        &config.snapcast.group_volume_mode,
    );
    set_table_str(
        doc,
        "snapcast",
        "unknown_clients",
        &config.snapcast.unknown_clients,
    );
    set_optional_table_str(
        doc,
        "snapcast",
        "default_zone",
        config.snapcast.default_zone.as_deref(),
    );
    if let Some(snapcast) = doc.get_mut("snapcast").and_then(Item::as_table_mut) {
        // Removed server field from early SnapDog development versions.
        snapcast.remove("mdns_name");
    }
    if let Some(psk) = &config.snapcast.encryption_psk {
        set_table_str(doc, "snapcast", "encryption_psk", psk);
    } else if let Some(snapcast) = doc.get_mut("snapcast").and_then(Item::as_table_mut) {
        snapcast.remove("encryption_psk");
    }

    // mDNS
    set_table_bool(doc, "mdns", "enabled", config.mdns.enabled);
    set_table_bool(
        doc,
        "mdns",
        "advertise_snapcast",
        config.mdns.advertise_snapcast,
    );
    set_table_bool(doc, "dbus", "enabled", config.dbus.enabled);
    apply_source_sections(doc, config);
    apply_integration_sections(doc, config);
}

fn apply_source_sections(doc: &mut DocumentMut, config: &ServerConfig) {
    // Optional sections: add or remove based on Some/None
    set_optional_section(
        doc,
        "subsonic",
        &["url", "username", "password", "format", "tls_skip_verify"],
        config.subsonic.as_ref().map(|s| {
            let mut t = Table::new();
            t["url"] = toml_edit::value(&s.url);
            t["username"] = toml_edit::value(&s.username);
            t["password"] = toml_edit::value(&s.password);
            if s.format != "raw" {
                t["format"] = toml_edit::value(&s.format);
            }
            t["tls_skip_verify"] = toml_edit::value(s.tls_skip_verify);
            t
        }),
    );
    if let Some(subsonic) = &config.subsonic {
        let mut cache = Table::new();
        if !subsonic.cache.path.is_empty() {
            cache["path"] = toml_edit::value(&subsonic.cache.path);
        }
        cache["max_size_mb"] =
            toml_edit::value(i64::try_from(subsonic.cache.max_size_mb).unwrap_or(i64::MAX));
        merge_document_nested_table(doc, "subsonic", "cache", &cache, &["path", "max_size_mb"]);
    }

    set_optional_section(
        doc,
        "spotify",
        &["name", "bitrate"],
        config.spotify.as_ref().map(|s| {
            let mut t = Table::new();
            t["name"] = toml_edit::value(&s.name);
            t["bitrate"] = toml_edit::value(i64::from(s.bitrate));
            t
        }),
    );

    set_optional_section(
        doc,
        "airplay",
        &["mode", "password", "bind"],
        config.airplay.as_ref().map(|a| {
            let mut t = Table::new();
            if a.mode != "airplay2" {
                t["mode"] = toml_edit::value(&a.mode);
            }
            if let Some(pw) = &a.password {
                t["password"] = toml_edit::value(pw);
            }
            if !a.bind.is_empty() {
                let bind: toml_edit::Array = a.bind.iter().map(String::as_str).collect();
                t["bind"] = toml_edit::value(bind);
            }
            t
        }),
    );
}

fn apply_integration_sections(doc: &mut DocumentMut, config: &ServerConfig) {
    set_optional_section(
        doc,
        "mqtt",
        &["broker", "client_id", "username", "password", "base_topic"],
        config.mqtt.as_ref().map(|m| {
            let mut t = Table::new();
            t["broker"] = toml_edit::value(&m.broker);
            t["client_id"] = toml_edit::value(&m.client_id);
            if let Some(u) = &m.username {
                t["username"] = toml_edit::value(u);
            }
            if let Some(p) = &m.password {
                t["password"] = toml_edit::value(p);
            }
            t["base_topic"] = toml_edit::value(&m.base_topic);
            t
        }),
    );

    set_optional_section(
        doc,
        "knx",
        &[
            "role",
            "url",
            "individual_address",
            "persist_ets_config",
            "restart_after_ets",
            "start_prog_mode",
            "server_online",
            "all_stop",
            "all_mute",
            "all_mute_status",
            "system_fault",
            "knx_time",
            "heartbeat_minutes",
            "sync_system_clock",
        ],
        config.knx.as_ref().map(|k| {
            let mut t = Table::new();
            t["role"] = toml_edit::value(&k.role);
            if let Some(url) = &k.url {
                t["url"] = toml_edit::value(url);
            }
            insert_optional_str(
                &mut t,
                "individual_address",
                k.individual_address.as_deref(),
            );
            insert_optional_bool(&mut t, "persist_ets_config", k.persist_ets_config);
            insert_optional_bool(&mut t, "restart_after_ets", k.restart_after_ets);
            t["start_prog_mode"] = toml_edit::value(k.start_prog_mode);
            insert_optional_str(&mut t, "server_online", k.server_online.as_deref());
            insert_optional_str(&mut t, "all_stop", k.all_stop.as_deref());
            insert_optional_str(&mut t, "all_mute", k.all_mute.as_deref());
            insert_optional_str(&mut t, "all_mute_status", k.all_mute_status.as_deref());
            insert_optional_str(&mut t, "system_fault", k.system_fault.as_deref());
            insert_optional_str(&mut t, "knx_time", k.knx_time.as_deref());
            t["heartbeat_minutes"] = toml_edit::value(i64::from(k.heartbeat_minutes));
            t["sync_system_clock"] = toml_edit::value(k.sync_system_clock);
            t
        }),
    );
}

fn apply_config_arrays(doc: &mut DocumentMut, config: &ServerConfig) {
    let old_zones = source_tables(doc, "zone");
    let old_clients = source_tables(doc, "client");
    let old_radio = source_tables(doc, "radio");
    doc.remove("zone");
    doc.remove("client");
    doc.remove("radio");

    for zone in &config.zones {
        let mut t = source_table(&old_zones, zone.source_index);
        set_item_preserving_decor(&mut t, "name", toml_edit::value(&zone.name));
        set_item_preserving_decor(&mut t, "icon", toml_edit::value(&zone.icon));
        set_optional_item_str(&mut t, "sink", zone.sink.as_deref());
        set_optional_item_str(&mut t, "airplay_name", zone.airplay_name.as_deref());
        set_optional_item_str(&mut t, "spotify_name", zone.spotify_name.as_deref());
        set_optional_item_str(
            &mut t,
            "group_volume_mode",
            zone.group_volume_mode.as_deref(),
        );
        if let Some(knx) = &zone.knx {
            merge_nested_table(&mut t, "knx", &build_knx_go_table(knx), ZONE_KNX_KEYS);
        } else {
            t.remove("knx");
        }
        push_table(doc, "zone", t);
    }

    for client in &config.clients {
        let mut t = source_table(&old_clients, client.source_index);
        set_item_preserving_decor(&mut t, "name", toml_edit::value(&client.name));
        set_item_preserving_decor(&mut t, "mac", toml_edit::value(&client.mac));
        set_item_preserving_decor(&mut t, "zone", toml_edit::value(&client.zone));
        set_item_preserving_decor(&mut t, "icon", toml_edit::value(&client.icon));
        if client.max_volume < 100 {
            set_item_preserving_decor(
                &mut t,
                "max_volume",
                toml_edit::value(i64::from(client.max_volume)),
            );
        } else {
            t.remove("max_volume");
        }
        if client.default_volume == 50 {
            t.remove("default_volume");
        } else {
            set_item_preserving_decor(
                &mut t,
                "default_volume",
                toml_edit::value(i64::from(client.default_volume)),
            );
        }
        if client.default_latency != 0 {
            set_item_preserving_decor(
                &mut t,
                "default_latency",
                toml_edit::value(i64::from(client.default_latency)),
            );
        } else {
            t.remove("default_latency");
        }
        if let Some(knx) = &client.knx {
            merge_nested_table(&mut t, "knx", &build_client_knx_table(knx), CLIENT_KNX_KEYS);
        } else {
            t.remove("knx");
        }
        push_table(doc, "client", t);
    }

    for station in &config.radio {
        let mut t = source_table(&old_radio, station.source_index);
        set_item_preserving_decor(&mut t, "name", toml_edit::value(&station.name));
        set_item_preserving_decor(&mut t, "url", toml_edit::value(&station.url));
        if let Some(cover) = &station.cover {
            set_item_preserving_decor(&mut t, "cover", toml_edit::value(cover));
        } else {
            t.remove("cover");
        }
        push_table(doc, "radio", t);
    }
}

fn source_tables(doc: &DocumentMut, key: &str) -> Vec<Table> {
    doc.get(key)
        .and_then(Item::as_array_of_tables)
        .map(|tables| tables.iter().cloned().collect())
        .unwrap_or_default()
}

fn set_optional_item_str(table: &mut Table, key: &str, value: Option<&str>) {
    if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
        set_item_preserving_decor(table, key, toml_edit::value(value));
    } else {
        table.remove(key);
    }
}

fn source_table(tables: &[Table], source_index: Option<usize>) -> Table {
    source_index
        .and_then(|index| tables.get(index))
        .cloned()
        .unwrap_or_default()
}

fn push_table(doc: &mut DocumentMut, key: &str, table: Table) {
    if let Some(tables) = doc
        .as_table_mut()
        .entry(key)
        .or_insert(Item::ArrayOfTables(ArrayOfTables::default()))
        .as_array_of_tables_mut()
    {
        tables.push(table);
    }
}

fn merge_nested_table(parent: &mut Table, key: &str, replacement: &Table, managed_keys: &[&str]) {
    let item = parent
        .entry(key)
        .or_insert_with(|| Item::Table(Table::new()));
    if !item.is_table() {
        *item = Item::Table(Table::new());
    }
    if let Some(existing) = item.as_table_mut() {
        merge_managed_table(existing, replacement, managed_keys);
    }
}

fn merge_document_nested_table(
    doc: &mut DocumentMut,
    section: &str,
    key: &str,
    replacement: &Table,
    managed_keys: &[&str],
) {
    let parent = doc
        .as_table_mut()
        .entry(section)
        .or_insert_with(|| Item::Table(Table::new()));
    if !parent.is_table() {
        *parent = Item::Table(Table::new());
    }
    if let Some(parent) = parent.as_table_mut() {
        merge_nested_table(parent, key, replacement, managed_keys);
    }
}

// ── Helpers ───────────────────────────────────────────────────

fn get_str(table: &Table, key: &str, default: &str) -> String {
    table
        .get(key)
        .and_then(Item::as_str)
        .unwrap_or(default)
        .to_string()
}

fn get_u32(table: &Table, key: &str, default: u32) -> u32 {
    table
        .get(key)
        .and_then(Item::as_integer)
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(default)
}

fn get_u16(table: &Table, key: &str, default: u16) -> u16 {
    table
        .get(key)
        .and_then(Item::as_integer)
        .and_then(|v| u16::try_from(v).ok())
        .unwrap_or(default)
}

fn get_u16_alias(table: &Table, key: &str, alias: &str, default: u16) -> u16 {
    table
        .get(key)
        .or_else(|| table.get(alias))
        .and_then(Item::as_integer)
        .and_then(|value| u16::try_from(value).ok())
        .unwrap_or(default)
}

fn get_u8(table: &Table, key: &str, default: u8) -> u8 {
    table
        .get(key)
        .and_then(Item::as_integer)
        .and_then(|v| u8::try_from(v).ok())
        .unwrap_or(default)
}

fn get_u64(table: &Table, key: &str, default: u64) -> u64 {
    table
        .get(key)
        .and_then(Item::as_integer)
        .and_then(|value| u64::try_from(value).ok())
        .unwrap_or(default)
}

fn get_i32(table: &Table, key: &str, default: i32) -> i32 {
    table
        .get(key)
        .and_then(Item::as_integer)
        .and_then(|value| i32::try_from(value).ok())
        .unwrap_or(default)
}

fn get_bool(table: &Table, key: &str, default: bool) -> bool {
    table.get(key).and_then(Item::as_bool).unwrap_or(default)
}

fn get_optional_bool(table: &Table, key: &str) -> Option<bool> {
    table.get(key).and_then(Item::as_bool)
}

fn get_string_array(table: &Table, key: &str) -> Vec<String> {
    table
        .get(key)
        .and_then(Item::as_array)
        .map(|array| {
            array
                .iter()
                .filter_map(|value| value.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

fn set_table_str(doc: &mut DocumentMut, section: &str, key: &str, value: &str) {
    if let Some(t) = doc
        .as_table_mut()
        .entry(section)
        .or_insert(Item::Table(Table::new()))
        .as_table_mut()
    {
        set_item_preserving_decor(t, key, toml_edit::value(value));
    }
}

fn set_optional_table_str(doc: &mut DocumentMut, section: &str, key: &str, value: Option<&str>) {
    if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
        set_table_str(doc, section, key, value);
    } else if let Some(table) = doc.get_mut(section).and_then(Item::as_table_mut) {
        table.remove(key);
    }
}

fn set_table_u32(doc: &mut DocumentMut, section: &str, key: &str, value: u32) {
    if let Some(t) = doc
        .as_table_mut()
        .entry(section)
        .or_insert(Item::Table(Table::new()))
        .as_table_mut()
    {
        set_item_preserving_decor(t, key, toml_edit::value(i64::from(value)));
    }
}

fn set_table_bool(doc: &mut DocumentMut, section: &str, key: &str, value: bool) {
    if let Some(table) = doc
        .as_table_mut()
        .entry(section)
        .or_insert(Item::Table(Table::new()))
        .as_table_mut()
    {
        set_item_preserving_decor(table, key, toml_edit::value(value));
    }
}

fn set_optional_section(
    doc: &mut DocumentMut,
    key: &str,
    managed_keys: &[&str],
    table: Option<Table>,
) {
    match table {
        Some(replacement) => {
            let existing = doc
                .as_table_mut()
                .entry(key)
                .or_insert(Item::Table(Table::new()));
            let Some(existing) = existing.as_table_mut() else {
                *existing = Item::Table(Table::new());
                let Some(existing) = existing.as_table_mut() else {
                    return;
                };
                merge_managed_table(existing, &replacement, managed_keys);
                return;
            };
            merge_managed_table(existing, &replacement, managed_keys);
        }
        None => {
            doc.remove(key);
        }
    }
}

fn merge_managed_table(existing: &mut Table, replacement: &Table, managed_keys: &[&str]) {
    for key in managed_keys {
        if let Some(item) = replacement.get(key) {
            set_item_preserving_decor(existing, key, item.clone());
        } else {
            existing.remove(key);
        }
    }
}

fn set_item_preserving_decor(table: &mut Table, key: &str, mut replacement: Item) {
    if let Some(previous) = table.get_mut(key) {
        if let (Some(previous), Some(next)) = (previous.as_value(), replacement.as_value_mut()) {
            *next.decor_mut() = previous.decor().clone();
        }
        *previous = replacement;
    } else {
        table.insert(key, replacement);
    }
}

const ZONE_KNX_KEYS: &[&str] = &[
    "play",
    "pause",
    "stop",
    "track_next",
    "track_previous",
    "control_status",
    "volume",
    "volume_status",
    "volume_dim",
    "mute",
    "mute_status",
    "mute_toggle",
    "track_title_status",
    "track_artist_status",
    "track_album_status",
    "track_progress_status",
    "track_playing_status",
    "track_repeat",
    "track_repeat_status",
    "track_repeat_toggle",
    "playlist",
    "playlist_status",
    "playlist_next",
    "playlist_previous",
    "shuffle",
    "shuffle_status",
    "shuffle_toggle",
    "repeat",
    "repeat_status",
    "repeat_toggle",
    "presence",
    "presence_enable",
    "presence_enable_status",
    "presence_timer_status",
];

const CLIENT_KNX_KEYS: &[&str] = &[
    "volume",
    "volume_status",
    "volume_dim",
    "mute",
    "mute_status",
    "mute_toggle",
    "latency",
    "latency_status",
    "zone",
    "zone_status",
    "connected_status",
];

fn parse_zone_knx(table: &Table) -> KnxGroupObjects {
    KnxGroupObjects {
        play: get_optional_str(table, "play"),
        pause: get_optional_str(table, "pause"),
        stop: get_optional_str(table, "stop"),
        track_next: get_optional_str(table, "track_next"),
        track_previous: get_optional_str(table, "track_previous"),
        control_status: get_optional_str(table, "control_status"),
        volume: get_optional_str(table, "volume"),
        volume_status: get_optional_str(table, "volume_status"),
        volume_dim: get_optional_str(table, "volume_dim"),
        mute: get_optional_str(table, "mute"),
        mute_status: get_optional_str(table, "mute_status"),
        mute_toggle: get_optional_str(table, "mute_toggle"),
        track_title_status: get_optional_str_alias(table, "track_title_status", "track_title"),
        track_artist_status: get_optional_str_alias(table, "track_artist_status", "track_artist"),
        track_album_status: get_optional_str(table, "track_album_status"),
        track_progress_status: get_optional_str(table, "track_progress_status"),
        track_playing_status: get_optional_str(table, "track_playing_status"),
        track_repeat: get_optional_str(table, "track_repeat"),
        track_repeat_status: get_optional_str(table, "track_repeat_status"),
        track_repeat_toggle: get_optional_str(table, "track_repeat_toggle"),
        playlist: get_optional_str(table, "playlist"),
        playlist_status: get_optional_str(table, "playlist_status"),
        playlist_next: get_optional_str(table, "playlist_next"),
        playlist_previous: get_optional_str(table, "playlist_previous"),
        shuffle: get_optional_str(table, "shuffle"),
        shuffle_status: get_optional_str(table, "shuffle_status"),
        shuffle_toggle: get_optional_str(table, "shuffle_toggle"),
        repeat: get_optional_str(table, "repeat"),
        repeat_status: get_optional_str(table, "repeat_status"),
        repeat_toggle: get_optional_str(table, "repeat_toggle"),
        presence: get_optional_str(table, "presence"),
        presence_enable: get_optional_str(table, "presence_enable"),
        presence_enable_status: get_optional_str(table, "presence_enable_status"),
        presence_timer_status: get_optional_str(table, "presence_timer_status"),
    }
}

fn parse_client_knx(table: &Table) -> ClientKnxGOs {
    ClientKnxGOs {
        volume: get_optional_str(table, "volume"),
        volume_status: get_optional_str(table, "volume_status"),
        volume_dim: get_optional_str(table, "volume_dim"),
        mute: get_optional_str(table, "mute"),
        mute_status: get_optional_str(table, "mute_status"),
        mute_toggle: get_optional_str(table, "mute_toggle"),
        latency: get_optional_str(table, "latency"),
        latency_status: get_optional_str(table, "latency_status"),
        zone: get_optional_str(table, "zone"),
        zone_status: get_optional_str(table, "zone_status"),
        connected_status: get_optional_str(table, "connected_status"),
    }
}

fn build_knx_go_table(knx: &KnxGroupObjects) -> Table {
    let mut t = Table::new();
    for (key, value) in zone_knx_values(knx) {
        insert_knx_value(&mut t, key, value);
    }
    t
}

fn build_client_knx_table(knx: &ClientKnxGOs) -> Table {
    let mut t = Table::new();
    for (key, value) in client_knx_values(knx) {
        insert_knx_value(&mut t, key, value);
    }
    t
}

fn get_optional_str(table: &Table, key: &str) -> Option<String> {
    table.get(key).and_then(|v| v.as_str()).map(String::from)
}

fn get_optional_str_alias(table: &Table, key: &str, legacy_key: &str) -> Option<String> {
    get_optional_str(table, key).or_else(|| get_optional_str(table, legacy_key))
}

fn insert_knx_value(table: &mut Table, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        let value = value.trim();
        if !value.is_empty() {
            table[key] = toml_edit::value(value);
        }
    }
}

fn insert_optional_str(table: &mut Table, key: &str, value: Option<&str>) {
    if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
        table[key] = toml_edit::value(value);
    }
}

fn insert_optional_bool(table: &mut Table, key: &str, value: Option<bool>) {
    if let Some(value) = value {
        table[key] = toml_edit::value(value);
    }
}

fn zone_knx_values(knx: &KnxGroupObjects) -> Vec<(&'static str, Option<&str>)> {
    vec![
        ("play", knx.play.as_deref()),
        ("pause", knx.pause.as_deref()),
        ("stop", knx.stop.as_deref()),
        ("track_next", knx.track_next.as_deref()),
        ("track_previous", knx.track_previous.as_deref()),
        ("control_status", knx.control_status.as_deref()),
        ("volume", knx.volume.as_deref()),
        ("volume_status", knx.volume_status.as_deref()),
        ("volume_dim", knx.volume_dim.as_deref()),
        ("mute", knx.mute.as_deref()),
        ("mute_status", knx.mute_status.as_deref()),
        ("mute_toggle", knx.mute_toggle.as_deref()),
        ("track_title_status", knx.track_title_status.as_deref()),
        ("track_artist_status", knx.track_artist_status.as_deref()),
        ("track_album_status", knx.track_album_status.as_deref()),
        (
            "track_progress_status",
            knx.track_progress_status.as_deref(),
        ),
        ("track_playing_status", knx.track_playing_status.as_deref()),
        ("track_repeat", knx.track_repeat.as_deref()),
        ("track_repeat_status", knx.track_repeat_status.as_deref()),
        ("track_repeat_toggle", knx.track_repeat_toggle.as_deref()),
        ("playlist", knx.playlist.as_deref()),
        ("playlist_status", knx.playlist_status.as_deref()),
        ("playlist_next", knx.playlist_next.as_deref()),
        ("playlist_previous", knx.playlist_previous.as_deref()),
        ("shuffle", knx.shuffle.as_deref()),
        ("shuffle_status", knx.shuffle_status.as_deref()),
        ("shuffle_toggle", knx.shuffle_toggle.as_deref()),
        ("repeat", knx.repeat.as_deref()),
        ("repeat_status", knx.repeat_status.as_deref()),
        ("repeat_toggle", knx.repeat_toggle.as_deref()),
        ("presence", knx.presence.as_deref()),
        ("presence_enable", knx.presence_enable.as_deref()),
        (
            "presence_enable_status",
            knx.presence_enable_status.as_deref(),
        ),
        (
            "presence_timer_status",
            knx.presence_timer_status.as_deref(),
        ),
    ]
}

fn client_knx_values(knx: &ClientKnxGOs) -> Vec<(&'static str, Option<&str>)> {
    vec![
        ("volume", knx.volume.as_deref()),
        ("volume_status", knx.volume_status.as_deref()),
        ("volume_dim", knx.volume_dim.as_deref()),
        ("mute", knx.mute.as_deref()),
        ("mute_status", knx.mute_status.as_deref()),
        ("mute_toggle", knx.mute_toggle.as_deref()),
        ("latency", knx.latency.as_deref()),
        ("latency_status", knx.latency_status.as_deref()),
        ("zone", knx.zone.as_deref()),
        ("zone_status", knx.zone_status.as_deref()),
        ("connected_status", knx.connected_status.as_deref()),
    ]
}

fn validate_zone_knx(name: &str, knx: &KnxGroupObjects) -> Result<()> {
    for (key, value) in zone_knx_values(knx) {
        validate_optional_knx_group_address(&format!("zone '{name}' KNX {key}"), value)?;
    }
    Ok(())
}

fn validate_client_knx(name: &str, knx: &ClientKnxGOs) -> Result<()> {
    for (key, value) in client_knx_values(knx) {
        validate_optional_knx_group_address(&format!("client '{name}' KNX {key}"), value)?;
    }
    Ok(())
}

fn validate_optional_knx_group_address(label: &str, value: Option<&str>) -> Result<()> {
    if let Some(value) = value {
        let value = value.trim();
        if !value.is_empty() {
            validate_knx_group_address(label, value)?;
        }
    }
    Ok(())
}

fn validate_knx_group_address(label: &str, value: &str) -> Result<()> {
    let parts: Vec<&str> = value.split('/').collect();
    match parts.as_slice() {
        [main, sub] => {
            parse_group_address_part(label, "main", main, 31)?;
            parse_group_address_part(label, "sub", sub, 2047)?;
        }
        [main, middle, sub] => {
            parse_group_address_part(label, "main", main, 31)?;
            parse_group_address_part(label, "middle", middle, 7)?;
            parse_group_address_part(label, "sub", sub, 255)?;
        }
        _ => anyhow::bail!("{label} must use main/sub or main/middle/sub KNX group address format"),
    }
    Ok(())
}

fn parse_group_address_part(label: &str, part_name: &str, value: &str, max: u16) -> Result<u16> {
    anyhow::ensure!(
        !value.is_empty() && value.chars().all(|c| c.is_ascii_digit()),
        "{label} has an invalid {part_name} group address part"
    );
    let parsed = value
        .parse::<u16>()
        .with_context(|| format!("{label} has an invalid {part_name} group address part"))?;
    anyhow::ensure!(
        parsed <= max,
        "{label} has a {part_name} group address part outside 0-{max}"
    );
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_writes_all_zone_knx_group_addresses() {
        let mut source = Table::new();
        for (index, key) in ZONE_KNX_KEYS.iter().enumerate() {
            source[*key] = toml_edit::value(format!("1/{}/{}", index % 8, index));
        }

        let parsed = parse_zone_knx(&source);
        let built = build_knx_go_table(&parsed);

        for key in ZONE_KNX_KEYS {
            assert_eq!(
                built.get(key).and_then(|v| v.as_str()),
                source.get(key).and_then(|v| v.as_str()),
                "{key}"
            );
        }
    }

    #[test]
    fn parses_and_writes_all_client_knx_group_addresses() {
        let mut source = Table::new();
        for (index, key) in CLIENT_KNX_KEYS.iter().enumerate() {
            source[*key] = toml_edit::value(format!("2/{}/{}", index % 8, index));
        }

        let parsed = parse_client_knx(&source);
        let built = build_client_knx_table(&parsed);

        for key in CLIENT_KNX_KEYS {
            assert_eq!(
                built.get(key).and_then(|v| v.as_str()),
                source.get(key).and_then(|v| v.as_str()),
                "{key}"
            );
        }
    }

    #[test]
    fn accepts_legacy_track_title_and_artist_keys() {
        let mut source = Table::new();
        source["track_title"] = toml_edit::value("1/2/3");
        source["track_artist"] = toml_edit::value("1/2/4");

        let parsed = parse_zone_knx(&source);

        assert_eq!(parsed.track_title_status.as_deref(), Some("1/2/3"));
        assert_eq!(parsed.track_artist_status.as_deref(), Some("1/2/4"));
    }

    #[test]
    fn writes_knx_group_addresses_in_full_config_toml() {
        let mut doc = DocumentMut::new();
        let config = ServerConfig {
            name: "SnapDog".into(),
            knx: Some(KnxConfig {
                role: "device".into(),
                ..KnxConfig::default()
            }),
            zones: vec![ZoneConfig {
                source_index: None,
                name: "Living".into(),
                icon: "speaker".into(),
                sink: None,
                airplay_name: None,
                spotify_name: None,
                group_volume_mode: None,
                knx: Some(KnxGroupObjects {
                    play: Some("1/2/3".into()),
                    presence_timer_status: Some("1/2/4".into()),
                    ..Default::default()
                }),
            }],
            clients: vec![ClientEntry {
                source_index: None,
                name: "Kitchen".into(),
                mac: "aa:bb:cc:dd:ee:ff".into(),
                zone: "Living".into(),
                icon: "speaker".into(),
                max_volume: 100,
                default_volume: 50,
                default_latency: 0,
                knx: Some(ClientKnxGOs {
                    latency_status: Some("2/1/9".into()),
                    connected_status: Some("2/1/10".into()),
                    ..Default::default()
                }),
            }],
            ..ServerConfig::default()
        };

        apply_config(&mut doc, &config);
        let output = doc.to_string();
        let reparsed_doc: DocumentMut = output.parse().unwrap();
        let reparsed = parse_document(&reparsed_doc);

        assert_eq!(
            reparsed.zones[0]
                .knx
                .as_ref()
                .and_then(|knx| knx.presence_timer_status.as_deref()),
            Some("1/2/4")
        );
        assert_eq!(
            reparsed.clients[0]
                .knx
                .as_ref()
                .and_then(|knx| knx.connected_status.as_deref()),
            Some("2/1/10")
        );
    }

    #[test]
    fn validates_knx_group_address_ranges() {
        assert!(validate_knx_group_address("test", "31/7/255").is_ok());
        assert!(validate_knx_group_address("test", "32/0/0").is_err());
        assert!(validate_knx_group_address("test", "1/8/0").is_err());
        assert!(validate_knx_group_address("test", "1/0/256").is_err());
        assert!(validate_knx_group_address("test", "1/2047").is_ok());
        assert!(validate_knx_group_address("test", "1/2048").is_err());
        assert!(validate_knx_group_address("test", "1").is_err());
    }

    #[test]
    fn candidate_preserves_unknown_fields_comments_and_nested_tables() {
        let source = r#"# keep this comment
name = "Old name" # keep inline comment
future_top_level = "keep"

[http]
port = 5555

[subsonic]
url = "https://music.example"
username = "user"
password = "secret"
tls_skip_verify = true

[subsonic.cache]
path = "/data/cache"
max_size_mb = 4096

[[zone]]
name = "Removed"
sink = "pipe:///removed"

[[zone]]
name = "Living"
icon = "speaker"
sink = "pipe:///living"
airplay_name = "Living AirPlay"

[zone.presence]
auto_off_delay = 600

[[client]]
name = "Speaker"
mac = "aa:bb:cc:dd:ee:ff"
zone = "Living"
default_volume = 42
default_latency = 125
"#;
        let doc: DocumentMut = source.parse().unwrap();
        let mut config = parse_document(&doc);
        config.revision = config_revision(source);
        config.raw_toml = source.into();
        config.name = "New name".into();
        config.zones.remove(0);

        let rendered = render_candidate(source, &config).unwrap();
        assert!(rendered.contains("# keep this comment"));
        assert!(rendered.contains("# keep inline comment"));
        let candidate: DocumentMut = rendered.parse().unwrap();
        assert_eq!(candidate["future_top_level"].as_str(), Some("keep"));
        assert_eq!(candidate["http"]["port"].as_integer(), Some(5555));
        assert_eq!(
            candidate["subsonic"]["tls_skip_verify"].as_bool(),
            Some(true)
        );
        assert_eq!(
            candidate["subsonic"]["cache"]["max_size_mb"].as_integer(),
            Some(4096)
        );
        let zones = candidate["zone"].as_array_of_tables().unwrap();
        assert_eq!(zones.len(), 1);
        assert_eq!(
            zones.get(0).unwrap()["sink"].as_str(),
            Some("pipe:///living")
        );
        assert_eq!(
            zones.get(0).unwrap()["presence"]["auto_off_delay"].as_integer(),
            Some(600)
        );
        let clients = candidate["client"].as_array_of_tables().unwrap();
        assert_eq!(
            clients.get(0).unwrap()["default_volume"].as_integer(),
            Some(42)
        );
        assert_eq!(
            clients.get(0).unwrap()["default_latency"].as_integer(),
            Some(125)
        );
    }

    #[test]
    fn candidate_refuses_to_replace_invalid_existing_toml() {
        let error = render_candidate("not = [valid", &ServerConfig::default()).unwrap_err();
        assert!(error.to_string().contains("refusing to overwrite"));
    }

    #[test]
    fn revision_changes_with_source_content() {
        assert_eq!(config_revision("same"), config_revision("same"));
        assert_ne!(config_revision("old"), config_revision("new"));
    }

    #[test]
    fn revision_guard_rejects_stale_and_missing_revisions() {
        let current = "name = \"Current\"\n";
        let matching = ServerConfig {
            revision: config_revision(current),
            ..ServerConfig::default()
        };
        assert!(ensure_current_revision(&matching, current).is_ok());

        let stale = ServerConfig {
            revision: config_revision("name = \"Old\"\n"),
            ..ServerConfig::default()
        };
        assert!(ensure_current_revision(&stale, current).is_err());
        assert!(ensure_current_revision(&ServerConfig::default(), current).is_err());
    }

    #[test]
    fn advanced_editor_uses_complete_toml_without_structured_rewrite() {
        let source = "[[zone]]\nname = \"Old\"\n";
        let edited = "# advanced\nfuture = true\n\n[[zone]]\nname = \"New\"\n";
        let config = ServerConfig {
            revision: config_revision(source),
            raw_toml: edited.into(),
            raw_toml_changed: true,
            ..ServerConfig::default()
        };

        assert!(uses_advanced_toml(&config));
        assert_eq!(render_candidate(source, &config).unwrap(), edited);
    }

    #[test]
    fn advanced_editor_rejects_invalid_toml_before_activation() {
        let source = "[[zone]]\nname = \"Old\"\n";
        let config = ServerConfig {
            revision: config_revision(source),
            raw_toml: "broken = [".into(),
            raw_toml_changed: true,
            ..ServerConfig::default()
        };

        assert!(render_candidate(source, &config).is_err());
    }
}
