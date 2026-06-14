// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! Read/write/validate `snapdog.toml` using `toml_edit` to preserve comments.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use toml_edit::{ArrayOfTables, DocumentMut, Item, Table};

const CONFIG_PATH: &str = "/etc/snapdog/snapdog.toml";
const CONFIG_BACKUP: &str = "/etc/snapdog/snapdog.toml.bak";

/// Complete server configuration as exposed via the API.
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct ServerConfig {
    pub name: String,
    pub http: HttpConfig,
    pub audio: AudioConfig,
    pub snapcast: SnapcastConfig,
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

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct HttpConfig {
    pub api_keys: Vec<String>,
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
    pub streaming_port: u16,
    pub codec: String,
    pub encryption_psk: Option<String>,
    pub group_volume_mode: String,
    pub unknown_clients: String,
    pub default_zone: String,
    pub mdns_name: String,
    pub advertise_snapcast: bool,
}

impl Default for SnapcastConfig {
    fn default() -> Self {
        Self {
            streaming_port: 1704,
            codec: "f32lz4".into(),
            encryption_psk: None,
            group_volume_mode: "relative".into(),
            unknown_clients: "accept".into(),
            default_zone: String::new(),
            mdns_name: "SnapDog".into(),
            advertise_snapcast: false,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SubsonicConfig {
    pub url: String,
    pub username: String,
    pub password: String,
    pub format: String,
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
}

#[derive(Serialize, Deserialize, Clone)]
pub struct MqttConfig {
    pub broker: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub base_topic: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct KnxConfig {
    pub role: String,
    pub url: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ZoneConfig {
    pub name: String,
    pub icon: String,
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
    pub presence_timeout: Option<String>,
    pub presence_timeout_status: Option<String>,
    pub presence_timer_status: Option<String>,
    pub presence_source_override: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ClientEntry {
    pub name: String,
    pub mac: String,
    pub zone: String,
    pub icon: String,
    pub max_volume: u8,
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
    pub name: String,
    pub url: String,
    pub cover: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SystemConfig {
    pub log_level: String,
}

impl Default for SystemConfig {
    fn default() -> Self {
        Self {
            log_level: "info".into(),
        }
    }
}

/// Read the server config, parsing it into our struct.
pub async fn read_config() -> Result<ServerConfig> {
    let content = tokio::fs::read_to_string(CONFIG_PATH)
        .await
        .unwrap_or_default();

    if content.is_empty() {
        return Ok(ServerConfig::default());
    }

    let doc: DocumentMut = content.parse().context("failed to parse snapdog.toml")?;

    Ok(parse_document(&doc))
}

/// Write the server config, preserving comments where possible.
pub async fn write_config(config: &ServerConfig) -> Result<()> {
    // Read existing document to preserve comments
    let content = tokio::fs::read_to_string(CONFIG_PATH)
        .await
        .unwrap_or_default();

    let mut doc: DocumentMut = if content.is_empty() {
        DocumentMut::new()
    } else {
        content.parse().unwrap_or_default()
    };

    apply_config(&mut doc, config);

    // Backup
    if tokio::fs::metadata(CONFIG_PATH).await.is_ok() {
        let _ = tokio::fs::copy(CONFIG_PATH, CONFIG_BACKUP).await;
    }

    // Ensure directory exists
    tokio::fs::create_dir_all("/etc/snapdog").await?;
    tokio::fs::write(CONFIG_PATH, doc.to_string()).await?;

    Ok(())
}

/// Validate config before writing.
pub fn validate(config: &ServerConfig) -> Result<()> {
    anyhow::ensure!(
        [44100, 48000, 96000].contains(&config.audio.sample_rate),
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
        config.snapcast.codec != "f32lz4e" || config.snapcast.encryption_psk.is_some(),
        "f32lz4e requires encryption_psk"
    );
    anyhow::ensure!(
        ["last_wins", "receiver_wins"].contains(&config.audio.source_conflict.as_str()),
        "Invalid source_conflict"
    );
    anyhow::ensure!(
        ["relative", "absolute"].contains(&config.snapcast.group_volume_mode.as_str()),
        "Invalid group_volume_mode"
    );
    anyhow::ensure!(
        ["accept", "ignore", "reject"].contains(&config.snapcast.unknown_clients.as_str()),
        "Invalid unknown_clients"
    );
    anyhow::ensure!(
        config.audio.zone_switch_fade_ms <= 500,
        "zone_switch_fade_ms must be 0-500"
    );
    anyhow::ensure!(
        config.audio.source_switch_fade_ms <= 500,
        "source_switch_fade_ms must be 0-500"
    );

    // Validate KNX GAs
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

    for zone in &config.zones {
        anyhow::ensure!(!zone.name.is_empty(), "Zone name required");
        if let Some(knx) = &zone.knx {
            validate_zone_knx(&zone.name, knx)?;
        }
    }

    for client in &config.clients {
        anyhow::ensure!(!client.name.is_empty(), "Client name required");
        anyhow::ensure!(!client.mac.is_empty(), "Client MAC required");
        if let Some(knx) = &client.knx {
            validate_client_knx(&client.name, knx)?;
        }
    }

    Ok(())
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
mdns_name = "SnapDog"

[subsonic.cache]
path = "/tmp/snapdog-cache"
max_size_mb = 512
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
    }

    if let Some(http) = doc.get("http").and_then(Item::as_table) {
        if let Some(keys) = http.get("api_keys").and_then(|v| v.as_array()) {
            config.http.api_keys = keys
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
        }
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
        config.snapcast.streaming_port = get_u16(snap, "streaming_port", 1704);
        config.snapcast.codec = get_str(snap, "codec", "flac");
        config.snapcast.encryption_psk = snap
            .get("encryption_psk")
            .and_then(|v| v.as_str())
            .map(String::from);
        config.snapcast.group_volume_mode = get_str(snap, "group_volume_mode", "relative");
        config.snapcast.unknown_clients = get_str(snap, "unknown_clients", "accept");
        config.snapcast.default_zone = get_str(snap, "default_zone", "");
        config.snapcast.mdns_name = get_str(snap, "mdns_name", "SnapDog");
    }
    if let Some(mdns) = doc.get("mdns").and_then(Item::as_table) {
        config.snapcast.advertise_snapcast = mdns
            .get("advertise_snapcast")
            .and_then(toml_edit::Item::as_bool)
            .unwrap_or(false);
    }

    // Top-level name
    config.name = doc
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("SnapDog")
        .to_string();

    if let Some(sub) = doc.get("subsonic").and_then(Item::as_table) {
        config.subsonic = Some(SubsonicConfig {
            url: get_str(sub, "url", ""),
            username: get_str(sub, "username", ""),
            password: get_str(sub, "password", ""),
            format: get_str(sub, "format", "raw"),
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
        });
    }

    if let Some(mqtt) = doc.get("mqtt").and_then(Item::as_table) {
        config.mqtt = Some(MqttConfig {
            broker: get_str(mqtt, "broker", ""),
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
        });
    }

    if let Some(zones) = doc.get("zone").and_then(Item::as_array_of_tables) {
        for zone in zones {
            config.zones.push(ZoneConfig {
                name: get_str(zone, "name", ""),
                icon: get_str(zone, "icon", "🏠"),
                knx: zone.get("knx").and_then(Item::as_table).map(parse_zone_knx),
            });
        }
    }

    if let Some(clients) = doc.get("client").and_then(Item::as_array_of_tables) {
        for client in clients {
            config.clients.push(ClientEntry {
                name: get_str(client, "name", ""),
                mac: get_str(client, "mac", ""),
                zone: get_str(client, "zone", ""),
                icon: get_str(client, "icon", "🔊"),
                max_volume: get_u8(client, "max_volume", 100),
                knx: client
                    .get("knx")
                    .and_then(Item::as_table)
                    .map(parse_client_knx),
            });
        }
    }

    if let Some(radios) = doc.get("radio").and_then(Item::as_array_of_tables) {
        for radio in radios {
            config.radio.push(RadioStation {
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
    doc["name"] = toml_edit::value(&config.name);

    // HTTP
    if !config.http.api_keys.is_empty() {
        let http = doc
            .entry("http")
            .or_insert_with(|| Item::Table(toml_edit::Table::new()));
        let arr: toml_edit::Array = config.http.api_keys.iter().map(String::as_str).collect();
        http["api_keys"] = toml_edit::value(arr);
    } else if let Some(http) = doc.get_mut("http").and_then(|t| t.as_table_mut()) {
        http.remove("api_keys");
    }

    // System
    set_table_str(doc, "system", "log_level", &config.system.log_level);

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
    set_table_u32(
        doc,
        "snapcast",
        "streaming_port",
        u32::from(config.snapcast.streaming_port),
    );
    set_table_str(doc, "snapcast", "codec", &config.snapcast.codec);
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
    set_table_str(
        doc,
        "snapcast",
        "default_zone",
        &config.snapcast.default_zone,
    );
    set_table_str(doc, "snapcast", "mdns_name", &config.snapcast.mdns_name);
    if let Some(psk) = &config.snapcast.encryption_psk {
        set_table_str(doc, "snapcast", "encryption_psk", psk);
    }

    // mDNS
    let mdns = doc
        .entry("mdns")
        .or_insert_with(|| Item::Table(toml_edit::Table::new()));
    mdns["advertise_snapcast"] = toml_edit::value(config.snapcast.advertise_snapcast);
    apply_config_optional(doc, config);
}

fn apply_config_optional(doc: &mut DocumentMut, config: &ServerConfig) {
    // Optional sections: add or remove based on Some/None
    set_optional_section(
        doc,
        "subsonic",
        config.subsonic.as_ref().map(|s| {
            let mut t = Table::new();
            t["url"] = toml_edit::value(&s.url);
            t["username"] = toml_edit::value(&s.username);
            t["password"] = toml_edit::value(&s.password);
            if s.format != "raw" {
                t["format"] = toml_edit::value(&s.format);
            }
            t
        }),
    );

    set_optional_section(
        doc,
        "spotify",
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
        config.airplay.as_ref().map(|a| {
            let mut t = Table::new();
            if a.mode != "airplay2" {
                t["mode"] = toml_edit::value(&a.mode);
            }
            if let Some(pw) = &a.password {
                t["password"] = toml_edit::value(pw);
            }
            t
        }),
    );

    set_optional_section(
        doc,
        "mqtt",
        config.mqtt.as_ref().map(|m| {
            let mut t = Table::new();
            t["broker"] = toml_edit::value(&m.broker);
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
        config.knx.as_ref().map(|k| {
            let mut t = Table::new();
            t["role"] = toml_edit::value(&k.role);
            if let Some(url) = &k.url {
                t["url"] = toml_edit::value(url);
            }
            t
        }),
    );
}

fn apply_config_arrays(doc: &mut DocumentMut, config: &ServerConfig) {
    // Arrays: zones, clients, radio — rebuild from scratch
    doc.remove("zone");
    doc.remove("client");
    doc.remove("radio");

    // Re-add zones
    for zone in &config.zones {
        let mut t = Table::new();
        t["name"] = toml_edit::value(&zone.name);
        t["icon"] = toml_edit::value(&zone.icon);
        if let Some(knx) = &zone.knx {
            t["knx"] = Item::Table(build_knx_go_table(knx));
        }
        if let Some(arr) = doc
            .as_table_mut()
            .entry("zone")
            .or_insert(Item::ArrayOfTables(ArrayOfTables::default()))
            .as_array_of_tables_mut()
        {
            arr.push(t);
        }
    }

    // Re-add clients
    for client in &config.clients {
        let mut t = Table::new();
        t["name"] = toml_edit::value(&client.name);
        t["mac"] = toml_edit::value(&client.mac);
        t["zone"] = toml_edit::value(&client.zone);
        t["icon"] = toml_edit::value(&client.icon);
        if client.max_volume < 100 {
            t["max_volume"] = toml_edit::value(i64::from(client.max_volume));
        }
        if let Some(knx) = &client.knx {
            t["knx"] = Item::Table(build_client_knx_table(knx));
        }
        if let Some(arr) = doc
            .as_table_mut()
            .entry("client")
            .or_insert(Item::ArrayOfTables(ArrayOfTables::default()))
            .as_array_of_tables_mut()
        {
            arr.push(t);
        }
    }

    // Re-add radio
    for station in &config.radio {
        let mut t = Table::new();
        t["name"] = toml_edit::value(&station.name);
        t["url"] = toml_edit::value(&station.url);
        if let Some(cover) = &station.cover {
            t["cover"] = toml_edit::value(cover);
        }
        if let Some(arr) = doc
            .as_table_mut()
            .entry("radio")
            .or_insert(Item::ArrayOfTables(ArrayOfTables::default()))
            .as_array_of_tables_mut()
        {
            arr.push(t);
        }
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

fn get_u8(table: &Table, key: &str, default: u8) -> u8 {
    table
        .get(key)
        .and_then(Item::as_integer)
        .and_then(|v| u8::try_from(v).ok())
        .unwrap_or(default)
}

fn set_table_str(doc: &mut DocumentMut, section: &str, key: &str, value: &str) {
    if let Some(t) = doc
        .as_table_mut()
        .entry(section)
        .or_insert(Item::Table(Table::new()))
        .as_table_mut()
    {
        t[key] = toml_edit::value(value);
    }
}

fn set_table_u32(doc: &mut DocumentMut, section: &str, key: &str, value: u32) {
    if let Some(t) = doc
        .as_table_mut()
        .entry(section)
        .or_insert(Item::Table(Table::new()))
        .as_table_mut()
    {
        t[key] = toml_edit::value(i64::from(value));
    }
}

fn set_optional_section(doc: &mut DocumentMut, key: &str, table: Option<Table>) {
    match table {
        Some(t) => {
            doc[key] = Item::Table(t);
        }
        None => {
            doc.remove(key);
        }
    }
}

#[cfg(test)]
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
    "presence_timeout",
    "presence_timeout_status",
    "presence_timer_status",
    "presence_source_override",
];

#[cfg(test)]
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
        presence_timeout: get_optional_str(table, "presence_timeout"),
        presence_timeout_status: get_optional_str(table, "presence_timeout_status"),
        presence_timer_status: get_optional_str(table, "presence_timer_status"),
        presence_source_override: get_optional_str(table, "presence_source_override"),
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
        ("presence_timeout", knx.presence_timeout.as_deref()),
        (
            "presence_timeout_status",
            knx.presence_timeout_status.as_deref(),
        ),
        (
            "presence_timer_status",
            knx.presence_timer_status.as_deref(),
        ),
        (
            "presence_source_override",
            knx.presence_source_override.as_deref(),
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
    anyhow::ensure!(
        parts.len() == 3,
        "{label} must use main/middle/sub format (0-31/0-7/0-255)"
    );
    parse_group_address_part(label, "main", parts[0], 31)?;
    parse_group_address_part(label, "middle", parts[1], 7)?;
    parse_group_address_part(label, "sub", parts[2], 255)?;
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
                built.get(*key).and_then(|v| v.as_str()),
                source.get(*key).and_then(|v| v.as_str()),
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
                built.get(*key).and_then(|v| v.as_str()),
                source.get(*key).and_then(|v| v.as_str()),
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
                url: None,
            }),
            zones: vec![ZoneConfig {
                name: "Living".into(),
                icon: "speaker".into(),
                knx: Some(KnxGroupObjects {
                    play: Some("1/2/3".into()),
                    presence_source_override: Some("1/2/4".into()),
                    ..Default::default()
                }),
            }],
            clients: vec![ClientEntry {
                name: "Kitchen".into(),
                mac: "aa:bb:cc:dd:ee:ff".into(),
                zone: "Living".into(),
                icon: "speaker".into(),
                max_volume: 100,
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
                .and_then(|knx| knx.presence_source_override.as_deref()),
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
        assert!(validate_knx_group_address("test", "1/2").is_err());
    }
}
