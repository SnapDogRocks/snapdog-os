// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! Settings export/import — tar.gz of `/data` configuration files.

use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use serde::Serialize;

const DATA_DIR: &str = "/data";
const MAX_UPLOAD_SIZE: usize = 10 * 1024 * 1024; // 10 MB

/// Paths to include in export (relative to /data).
const EXPORT_PATHS: &[&str] = &[
    "snapdog",
    "hostname",
    "systemd/network",
    "wpa_supplicant",
    "ssh",
    "snapdog-os.channel",
    "default",
];

/// Summary of an archive's contents for preview.
#[derive(Serialize)]
pub struct SettingsPreview {
    pub hostname: Option<String>,
    pub wifi_configured: bool,
    pub ssh_keys_present: bool,
    pub has_auth: bool,
    pub files: Vec<String>,
}

/// Create a tar.gz archive of the device settings.
pub fn export_settings() -> Result<Vec<u8>> {
    let enc = GzEncoder::new(Vec::new(), Compression::default());
    let mut ar = tar::Builder::new(enc);

    for rel_path in EXPORT_PATHS {
        let full = PathBuf::from(DATA_DIR).join(rel_path);
        if full.is_dir() {
            ar.append_dir_all(rel_path, &full)
                .with_context(|| format!("failed to add directory: {rel_path}"))?;
        } else if full.is_file() {
            ar.append_path_with_name(&full, rel_path)
                .with_context(|| format!("failed to add file: {rel_path}"))?;
        }
    }

    let enc = ar.into_inner().context("failed to finalize tar")?;
    enc.finish().context("failed to finish gzip")
}

/// Parse a tar.gz archive and return a preview without extracting.
pub fn preview_settings(data: &[u8]) -> Result<SettingsPreview> {
    validate_size(data)?;

    let dec = GzDecoder::new(Cursor::new(data));
    let mut ar = tar::Archive::new(dec);

    let mut files = Vec::new();
    let mut hostname = None;
    let mut wifi_configured = false;
    let mut ssh_keys_present = false;
    let mut has_auth = false;

    for entry in ar.entries().context("invalid tar.gz archive")? {
        let mut entry = entry.context("corrupt archive entry")?;
        let path = entry.path().context("invalid path in archive")?;
        let path_str = path.to_string_lossy().to_string();

        validate_path(&path_str)?;
        files.push(path_str.clone());

        if path_str == "hostname" {
            let mut buf = String::new();
            entry.read_to_string(&mut buf).ok();
            hostname = Some(buf.trim().to_string());
        } else if path_str.contains("wpa_supplicant")
            && std::path::Path::new(&path_str)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("conf"))
        {
            let mut buf = String::new();
            if entry.read_to_string(&mut buf).is_ok() {
                wifi_configured = buf.contains("network=");
            }
        } else if path_str.contains("ssh/") && path_str.contains("authorized_keys") {
            ssh_keys_present = true;
        } else if path_str.contains("ctrl.toml") {
            let mut buf = String::new();
            if entry.read_to_string(&mut buf).is_ok() {
                has_auth = buf.contains("password_hash");
            }
        }
    }

    Ok(SettingsPreview {
        hostname,
        wifi_configured,
        ssh_keys_present,
        has_auth,
        files,
    })
}

/// Extract a tar.gz archive to /data after validation.
pub fn import_settings(data: &[u8]) -> Result<()> {
    validate_size(data)?;

    // First pass: validate all paths
    let dec = GzDecoder::new(Cursor::new(data));
    let mut ar = tar::Archive::new(dec);
    for entry in ar.entries().context("invalid tar.gz archive")? {
        let entry = entry.context("corrupt archive entry")?;
        let path = entry.path().context("invalid path in archive")?;
        validate_path(&path.to_string_lossy())?;
    }

    // Second pass: extract
    let dec = GzDecoder::new(Cursor::new(data));
    let mut ar = tar::Archive::new(dec);
    ar.set_preserve_permissions(true);
    ar.unpack(DATA_DIR).context("failed to extract settings")?;

    Ok(())
}

fn validate_size(data: &[u8]) -> Result<()> {
    if data.len() > MAX_UPLOAD_SIZE {
        bail!("archive too large (max 10 MB)");
    }
    Ok(())
}

fn validate_path(path: &str) -> Result<()> {
    if path.contains("..") || Path::new(path).is_absolute() {
        bail!("path traversal detected: {path}");
    }
    Ok(())
}
