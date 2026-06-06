// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! MPRIS2 D-Bus client — connects to snapdog-client for now-playing state.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tokio::sync::{Mutex, broadcast};
use zbus::zvariant::Value;

const BUS_NAME: &str = "org.mpris.MediaPlayer2.snapdog_client";
const OBJECT_PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER_IFACE: &str = "org.mpris.MediaPlayer2.Player";
const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Now-playing state exposed to the `WebUI`.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct NowPlaying {
    pub playing: bool,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub cover_url: Option<String>,
    pub duration_ms: i64,
    pub position_ms: i64,
    pub seekable: bool,
    pub can_next: bool,
    pub can_prev: bool,
    pub volume: u16,
    pub muted: bool,
}

pub type SharedNowPlaying = Arc<Mutex<NowPlaying>>;

/// Start polling the MPRIS2 interface. Returns shared state + change broadcast.
pub fn start(ws_tx: broadcast::Sender<String>) -> (SharedNowPlaying, tokio::task::JoinHandle<()>) {
    let state: SharedNowPlaying = Arc::new(Mutex::new(NowPlaying::default()));
    let state_clone = Arc::clone(&state);

    let handle = tokio::spawn(async move {
        loop {
            if let Err(e) = poll_loop(&state_clone, &ws_tx).await {
                tracing::debug!("MPRIS2 poll error (client not running?): {e}");
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });

    (state, handle)
}

async fn poll_loop(
    state: &SharedNowPlaying,
    ws_tx: &broadcast::Sender<String>,
) -> anyhow::Result<()> {
    let conn = zbus::Connection::system().await?;
    let proxy = zbus::fdo::PropertiesProxy::builder(&conn)
        .destination(BUS_NAME)?
        .path(OBJECT_PATH)?
        .build()
        .await?;

    loop {
        let props = proxy.get_all(PLAYER_IFACE.try_into()?).await?;
        let new_state = parse_props(&props);

        let mut current = state.lock().await;
        if *current != new_state {
            *current = new_state.clone();
            drop(current);
            if let Ok(json) = serde_json::to_string(&new_state) {
                let _ = ws_tx.send(format!("now_playing:{json}"));
            }
        }

        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

fn parse_props(props: &HashMap<String, zbus::zvariant::OwnedValue>) -> NowPlaying {
    let playing = props
        .get("PlaybackStatus")
        .and_then(|v| <&str>::try_from(v).ok())
        .is_some_and(|s| s == "Playing");

    let volume_f = props
        .get("Volume")
        .and_then(|v| v.downcast_ref::<f64>().ok())
        .unwrap_or(1.0);
    let muted = volume_f == 0.0;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let volume = (volume_f * 100.0) as u16;

    let position_us = props
        .get("Position")
        .and_then(|v| v.downcast_ref::<i64>().ok())
        .unwrap_or(0);

    let seekable = props
        .get("CanSeek")
        .and_then(|v| v.downcast_ref::<bool>().ok())
        .unwrap_or(false);

    let can_next = props
        .get("CanGoNext")
        .and_then(|v| v.downcast_ref::<bool>().ok())
        .unwrap_or(false);

    let can_prev = props
        .get("CanGoPrevious")
        .and_then(|v| v.downcast_ref::<bool>().ok())
        .unwrap_or(false);

    let metadata = props
        .get("Metadata")
        .and_then(|v| HashMap::<String, Value<'static>>::try_from(Value::from(v.clone())).ok());

    let (title, artist, album, cover_url, duration_ms) = metadata.map_or_else(
        || (String::new(), String::new(), String::new(), None, 0),
        |meta| {
            let title = meta
                .get("xesam:title")
                .and_then(|v| <&str>::try_from(v).ok())
                .unwrap_or("")
                .to_string();
            let artist = meta
                .get("xesam:artist")
                .and_then(|v| {
                    // MPRIS2 spec: xesam:artist is Vec<String>
                    zbus::zvariant::Array::try_from(v.clone())
                        .ok()
                        .and_then(|arr| {
                            arr.inner()
                                .first()
                                .and_then(|val| <&str>::try_from(val).ok().map(String::from))
                        })
                        .or_else(|| <&str>::try_from(v).ok().map(String::from))
                })
                .unwrap_or_default();
            let album = meta
                .get("xesam:album")
                .and_then(|v| <&str>::try_from(v).ok())
                .unwrap_or("")
                .to_string();
            let cover_url = meta
                .get("mpris:artUrl")
                .and_then(|v| <&str>::try_from(v).ok())
                .map(String::from);
            let duration_us = meta
                .get("mpris:length")
                .and_then(|v| v.downcast_ref::<i64>().ok())
                .unwrap_or(0);
            (title, artist, album, cover_url, duration_us / 1000)
        },
    );

    NowPlaying {
        playing,
        title,
        artist,
        album,
        cover_url,
        duration_ms,
        position_ms: position_us / 1000,
        seekable,
        can_next,
        can_prev,
        volume,
        muted,
    }
}

/// Send a transport command to snapdog-client via D-Bus.
pub async fn send_command(command: &str) -> anyhow::Result<()> {
    let conn = zbus::Connection::system().await?;
    let proxy: zbus::Proxy<'_> = zbus::proxy::Builder::new(&conn)
        .destination(BUS_NAME)?
        .path(OBJECT_PATH)?
        .interface(PLAYER_IFACE)?
        .build()
        .await?;

    match command {
        "play" => proxy.call_noreply("Play", &()).await?,
        "pause" => proxy.call_noreply("Pause", &()).await?,
        "play_pause" => proxy.call_noreply("PlayPause", &()).await?,
        "stop" => proxy.call_noreply("Stop", &()).await?,
        "next" => proxy.call_noreply("Next", &()).await?,
        "previous" => proxy.call_noreply("Previous", &()).await?,
        _ => anyhow::bail!("unknown command: {command}"),
    }
    Ok(())
}

/// Set volume via D-Bus (0.0–1.0).
pub async fn set_volume(volume: f64) -> anyhow::Result<()> {
    let conn = zbus::Connection::system().await?;
    let proxy = zbus::fdo::PropertiesProxy::builder(&conn)
        .destination(BUS_NAME)?
        .path(OBJECT_PATH)?
        .build()
        .await?;
    proxy
        .set(PLAYER_IFACE.try_into()?, "Volume", Value::from(volume))
        .await?;
    Ok(())
}

/// Seek to position (microseconds) via D-Bus.
pub async fn seek(offset_us: i64) -> anyhow::Result<()> {
    let conn = zbus::Connection::system().await?;
    let proxy: zbus::Proxy<'_> = zbus::proxy::Builder::new(&conn)
        .destination(BUS_NAME)?
        .path(OBJECT_PATH)?
        .interface(PLAYER_IFACE)?
        .build()
        .await?;
    proxy.call_noreply("Seek", &(offset_us,)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use zbus::zvariant::OwnedValue;

    #[test]
    fn test_parse_props_basic() {
        let mut props = HashMap::new();
        props.insert("PlaybackStatus".to_string(), OwnedValue::from("Playing"));
        props.insert("Volume".to_string(), OwnedValue::from(0.8));
        props.insert("Position".to_string(), OwnedValue::from(15000000i64));
        props.insert("CanSeek".to_string(), OwnedValue::from(true));
        props.insert("CanGoNext".to_string(), OwnedValue::from(true));
        props.insert("CanGoPrevious".to_string(), OwnedValue::from(false));

        let now_playing = parse_props(&props);
        assert!(now_playing.playing);
        assert_eq!(now_playing.volume, 80);
        assert!(!now_playing.muted);
        assert_eq!(now_playing.position_ms, 15000);
        assert!(now_playing.seekable);
        assert!(now_playing.can_next);
        assert!(!now_playing.can_prev);
    }
}
