// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! Robust parser and editor for Raspberry Pi `/boot/config.txt`.
//!
//! Preserves comments, blank lines, section headers, and ordering.
//! Only modifies the specific key being changed.

use anyhow::{Context, Result};
use std::fmt::Write as _;

const CONFIG_PATH: &str = "/boot/config.txt";
const BACKUP_PATH: &str = "/boot/config.txt.bak";

/// Known DAC/AMP overlay prefixes that we manage.
const AUDIO_OVERLAY_PREFIXES: &[&str] = &[
    "hifiberry-",
    "allo-",
    "iqaudio-",
    "justboom-",
    "max98357a",
    "googlevoicehat-",
    "i-sabre-",
    "fe-pi-",
    "adau7002-",
];

/// Read config.txt, returning its content.
pub async fn read() -> Result<String> {
    tokio::fs::read_to_string(CONFIG_PATH)
        .await
        .context("failed to read /boot/config.txt")
}

/// Write config.txt with automatic backup.
pub async fn write(content: &str) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        let _ = tokio::process::Command::new("mount")
            .args(["-o", "remount,rw", "/boot"])
            .output()
            .await;
    }

    let write_res = async {
        // Backup current file
        if tokio::fs::metadata(CONFIG_PATH).await.is_ok() {
            tokio::fs::copy(CONFIG_PATH, BACKUP_PATH).await?;
        }
        tokio::fs::write(CONFIG_PATH, content).await?;
        Ok::<(), anyhow::Error>(())
    }
    .await;

    #[cfg(target_os = "linux")]
    {
        let _ = tokio::process::Command::new("mount")
            .args(["-o", "remount,ro", "/boot"])
            .output()
            .await;
    }

    write_res
}

/// Get the current audio DAC overlay (if any).
pub async fn get_audio_overlay() -> Result<String> {
    let content = read().await?;
    Ok(find_audio_overlay(&content).unwrap_or_default())
}

/// Set the audio DAC overlay. Removes any existing audio overlay and adds the new one.
/// Pass empty string to remove (auto-detect via EEPROM).
pub async fn set_audio_overlay(overlay: &str) -> Result<()> {
    let content = read().await?;
    let new_content = replace_audio_overlay(&content, overlay);
    write(&new_content).await
}

// ── Internal parsing ──────────────────────────────────────────

/// Helper to parse a line into parts by '=', supporting nested '=' (like in dtparam=audio=off),
/// ignoring spaces around '=', and ignoring trailing comments.
fn match_line_key_value(line: &str, search_key: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.starts_with('#') {
        return None;
    }

    // Split inline comment if any
    let without_comment = trimmed.split('#').next()?;

    // Split line by '=' and trim each part
    let line_parts: Vec<&str> = without_comment.split('=').map(str::trim).collect();
    // Split search key by '=' and trim each part
    let search_parts: Vec<&str> = search_key.split('=').map(str::trim).collect();

    if line_parts.len() > search_parts.len() {
        // Check if the prefix matches the search key parts
        if line_parts[..search_parts.len()] == search_parts[..] {
            // The value is the rest of the parts joined by '='
            return Some(line_parts[search_parts.len()..].join("="));
        }
    }
    None
}

fn find_audio_overlay(content: &str) -> Option<String> {
    content
        .lines()
        .filter_map(|line| {
            let val = match_line_key_value(line, "dtoverlay")?;
            // Split by comma to get the actual overlay name
            let name = val.split(',').next()?.trim().to_string();
            Some(name)
        })
        .find(|name| is_audio_overlay(name))
}

fn is_audio_overlay(name: &str) -> bool {
    AUDIO_OVERLAY_PREFIXES
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

fn replace_audio_overlay(content: &str, new_overlay: &str) -> String {
    let mut result = Vec::new();
    let mut found = false;

    for line in content.lines() {
        let mut is_audio = false;

        if let Some(val) = match_line_key_value(line, "dtoverlay") {
            let name = val.split(',').next().unwrap_or("").trim();
            if is_audio_overlay(name) {
                is_audio = true;
                if !new_overlay.is_empty() && !found {
                    result.push(format!("dtoverlay={new_overlay}"));
                    found = true;
                }
            }
        }

        if !is_audio {
            result.push(line.to_string());
        }
    }

    // If no existing audio overlay was found, append the new one
    if !found && !new_overlay.is_empty() {
        result.push(format!("dtoverlay={new_overlay}"));
    }

    result.join("\n") + "\n"
}

pub fn find_value(content: &str, key: &str) -> Option<String> {
    content.lines().find_map(|l| match_line_key_value(l, key))
}

pub fn upsert_value(content: &str, key: &str, value: &str) -> String {
    let new_line = format!("{key}={value}");
    let mut result = Vec::new();
    let mut found = false;

    for line in content.lines() {
        if match_line_key_value(line, key).is_some() {
            result.push(new_line.clone());
            found = true;
        } else {
            result.push(line.to_string());
        }
    }

    if !found {
        result.push(new_line);
    }

    result.join("\n") + "\n"
}

pub fn has_dtoverlay(content: &str, overlay: &str) -> bool {
    content.lines().any(|l| {
        match_line_key_value(l, "dtoverlay").is_some_and(|val| {
            let name = val.split(',').next().unwrap_or("").trim();
            name == overlay
        })
    })
}

pub fn add_dtoverlay(content: &str, overlay: &str) -> String {
    if has_dtoverlay(content, overlay) {
        return content.to_string();
    }
    let mut result = content.trim_end().to_string();
    if !result.is_empty() {
        result.push('\n');
    }
    let _ = writeln!(result, "dtoverlay={overlay}");
    result
}

pub fn remove_dtoverlay(content: &str, overlay: &str) -> String {
    let mut result = Vec::new();
    for line in content.lines() {
        if let Some(val) = match_line_key_value(line, "dtoverlay") {
            let name = val.split(',').next().unwrap_or("").trim();
            if name == overlay {
                continue;
            }
        }
        result.push(line.to_string());
    }
    result.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_CONFIG: &str = "\
# RPi config
dtparam=i2c=on
dtparam=spi=on
dtparam=audio=off
dtoverlay=i2c-gpio,i2c_gpio_sda=0,i2c_gpio_scl=1
dtoverlay=vc4-kms-v3d
dtoverlay=allo-boss-dac-pcm512x-audio
# dtoverlay=iqaudio-dacplus
kernel=Image
";

    #[test]
    fn finds_audio_overlay() {
        assert_eq!(
            find_audio_overlay(SAMPLE_CONFIG),
            Some("allo-boss-dac-pcm512x-audio".into())
        );
    }

    #[test]
    fn ignores_commented_overlay() {
        let config = "# dtoverlay=allo-boss-dac-pcm512x-audio\n";
        assert_eq!(find_audio_overlay(config), None);
    }

    #[test]
    fn replaces_audio_overlay() {
        let result = replace_audio_overlay(SAMPLE_CONFIG, "iqaudio-dacplus");
        assert!(result.contains("dtoverlay=iqaudio-dacplus"));
        assert!(!result.contains("dtoverlay=allo-boss-dac-pcm512x-audio"));
        // Preserves non-audio overlays
        assert!(result.contains("dtoverlay=vc4-kms-v3d"));
        assert!(result.contains("dtoverlay=i2c-gpio"));
    }

    #[test]
    fn removes_audio_overlay() {
        let result = replace_audio_overlay(SAMPLE_CONFIG, "");
        assert!(!result.contains("allo-boss-dac-pcm512x-audio"));
        assert!(result.contains("dtoverlay=vc4-kms-v3d"));
    }

    #[test]
    fn adds_overlay_when_none_exists() {
        let config = "dtparam=i2c=on\nkernel=Image\n";
        let result = replace_audio_overlay(config, "max98357a");
        assert!(result.contains("dtoverlay=max98357a"));
    }

    #[test]
    fn find_value_works() {
        assert_eq!(
            find_value(SAMPLE_CONFIG, "dtparam=audio"),
            Some("off".into())
        );
        assert_eq!(find_value(SAMPLE_CONFIG, "kernel"), Some("Image".into()));
        assert_eq!(find_value(SAMPLE_CONFIG, "nonexistent"), None);
    }

    #[test]
    fn upsert_existing_value() {
        let result = upsert_value(SAMPLE_CONFIG, "kernel", "zImage");
        assert!(result.contains("kernel=zImage"));
        assert!(!result.contains("kernel=Image"));
    }

    #[test]
    fn upsert_new_value() {
        let result = upsert_value(SAMPLE_CONFIG, "gpu_mem", "128");
        assert!(result.contains("gpu_mem=128"));
    }

    #[test]
    fn robust_spacing_and_comments() {
        let config = "\
# RPi config
  dtparam  =  audio  =  off   # disable built-in audio
dtoverlay = disable-wifi,someparam=1 # disable wifi
";
        assert_eq!(find_value(config, "dtparam=audio"), Some("off".into()));
        assert!(has_dtoverlay(config, "disable-wifi"));

        let removed = remove_dtoverlay(config, "disable-wifi");
        assert!(!has_dtoverlay(&removed, "disable-wifi"));

        let upserted = upsert_value(config, "dtparam=audio", "on");
        assert_eq!(find_value(&upserted, "dtparam=audio"), Some("on".into()));
    }
}
