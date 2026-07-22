// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! Truthful, phase-based firmware update coordination.
//!
//! RAUC intentionally reports a hierarchical 0-100 progress value. The value is
//! useful inside an individual install step, but it is not an elapsed-time estimate:
//! a network download can sit inside "Checking bundle", and the final image `fsync`
//! has no byte progress at all. This module keeps those phases separate, stages
//! online bundles with real byte telemetry, and retains terminal state so the `WebUI`
//! never has to infer completion from a transient 100% sample.

use std::process::Stdio;
use std::sync::{
    OnceLock,
    atomic::{AtomicBool, Ordering},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;
use tokio::sync::{Mutex, RwLock, broadcast};

use crate::rauc::{InstallProgress, Rauc};

pub const UPDATE_BUNDLE_PATH: &str = "/data/update.raucb";
pub const UPDATE_BUNDLE_PART_PATH: &str = "/data/update.raucb.part";
const ONLINE_UPDATE_BUNDLE_PATH: &str = "/data/online-update.raucb";
const ONLINE_UPDATE_BUNDLE_PART_PATH: &str = "/data/online-update.raucb.part";
pub const MAX_BUNDLE_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_BUNDLE_BYTES_ARG: &str = "1073741824";
const DOWNLOAD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30 * 60);
const DOWNLOAD_TIMEOUT_ARG: &str = "1800";
const INSTALL_WARNING_AFTER: std::time::Duration = std::time::Duration::from_secs(30 * 60);
const RECOVERY_MARKER_PATH: &str = "/run/snapdog-ctrl-firmware-update.json";
const RECOVERY_MARKER_PART_PATH: &str = "/run/snapdog-ctrl-firmware-update.json.part";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdatePhase {
    Idle,
    Downloading,
    Verifying,
    Writing,
    Finalizing,
    ReadyToReboot,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UpdateProgress {
    pub phase: UpdatePhase,
    pub phase_progress: Option<u8>,
    pub overall_progress: Option<u8>,
    pub bytes_done: Option<u64>,
    pub bytes_total: Option<u64>,
    pub detail: String,
    pub last_error: String,
    pub signature_verified: bool,
}

impl Default for UpdateProgress {
    fn default() -> Self {
        Self {
            phase: UpdatePhase::Idle,
            phase_progress: None,
            overall_progress: None,
            bytes_done: None,
            bytes_total: None,
            detail: String::new(),
            last_error: String::new(),
            signature_verified: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedRaucProgress {
    pub phase: UpdatePhase,
    pub phase_progress: Option<u8>,
    pub overall_progress: Option<u8>,
    pub detail: String,
}

static PROGRESS: OnceLock<RwLock<UpdateProgress>> = OnceLock::new();
static BROADCASTER: OnceLock<broadcast::Sender<String>> = OnceLock::new();
static BUSY: AtomicBool = AtomicBool::new(false);
static ACTIVE: AtomicBool = AtomicBool::new(false);
static RECOVERY_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RecoveryState {
    Downloading,
    Installing,
    ReadyToReboot,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RecoveryMarker {
    state: RecoveryState,
    last_error_baseline: String,
    last_error: String,
}

fn progress_state() -> &'static RwLock<UpdateProgress> {
    PROGRESS.get_or_init(|| RwLock::new(UpdateProgress::default()))
}

pub fn set_broadcaster(sender: broadcast::Sender<String>) {
    let _ = BROADCASTER.set(sender);
}

pub async fn snapshot() -> UpdateProgress {
    progress_state().read().await.clone()
}

#[must_use]
pub fn is_active() -> bool {
    ACTIVE.load(Ordering::Acquire)
}

#[must_use]
pub fn is_busy() -> bool {
    BUSY.load(Ordering::Acquire)
}

async fn publish(next: UpdateProgress) {
    *progress_state().write().await = next.clone();
    if let Some(sender) = BROADCASTER.get()
        && let Ok(payload) = serde_json::to_string(&next)
    {
        let _ = sender.send(format!("update_progress:{payload}"));
    }
}

async fn publish_failure(error: &anyhow::Error) {
    let last_error = format!("{error:#}");
    if let Err(marker_error) = mark_recovery_failed(&last_error).await {
        tracing::warn!(%marker_error, "failed to persist firmware failure state");
    }
    publish(UpdateProgress {
        phase: UpdatePhase::Failed,
        last_error,
        detail: "Firmware update failed".into(),
        ..UpdateProgress::default()
    })
    .await;
}

#[must_use = "dropping the guard releases the exclusive firmware-operation lock"]
pub struct BusyGuard {
    installation: bool,
}

impl BusyGuard {
    fn acquire(installation: bool) -> Result<Self> {
        BUSY.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| anyhow::anyhow!("a firmware update is already in progress"))?;
        if installation {
            ACTIVE.store(true, Ordering::Release);
        }
        Ok(Self { installation })
    }
}

impl Drop for BusyGuard {
    fn drop(&mut self) {
        if self.installation {
            ACTIVE.store(false, Ordering::Release);
        }
        BUSY.store(false, Ordering::Release);
    }
}

/// Reserve the firmware pipeline while a manual bundle is uploaded. This blocks
/// online, automatic, and install requests without claiming that RAUC is active.
pub fn reserve_upload() -> Result<BusyGuard> {
    BusyGuard::acquire(false)
}

/// Start an online update in the background. The caller can return HTTP 202 while
/// `/system/update/status` and WebSocket events expose the complete lifecycle.
pub async fn start_online(url: String) -> Result<()> {
    let guard = BusyGuard::acquire(true)?;
    publish(UpdateProgress {
        phase: UpdatePhase::Downloading,
        detail: "Preparing firmware download".into(),
        ..UpdateProgress::default()
    })
    .await;
    if let Err(error) = begin_recovery(RecoveryState::Downloading, String::new()).await {
        publish_failure(&error).await;
        return Err(error);
    }
    tokio::spawn(async move {
        let result = run_online(&url).await;
        if let Err(error) = &result {
            tracing::error!(error = %error, "firmware update failed");
            publish_failure(error).await;
        }
        drop(guard);
    });
    Ok(())
}

/// Start installation of an already-uploaded local bundle in the background.
pub async fn start_local(path: &'static str) -> Result<()> {
    let guard = BusyGuard::acquire(true)?;
    publish(UpdateProgress {
        phase: UpdatePhase::Verifying,
        detail: "Preparing firmware verification".into(),
        ..UpdateProgress::default()
    })
    .await;
    let rauc = match prepare_rauc_install().await {
        Ok(rauc) => rauc,
        Err(error) => {
            publish_failure(&error).await;
            return Err(error);
        }
    };
    tokio::spawn(async move {
        let result = run_local_with_rauc(path, rauc).await;
        if let Err(error) = tokio::fs::remove_file(path).await
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(%error, %path, "failed to remove staged firmware bundle");
        }
        if let Err(error) = &result {
            tracing::error!(error = %error, "local firmware update failed");
            publish_failure(error).await;
        }
        drop(guard);
    });
    Ok(())
}

/// Download and install synchronously. Used by the automatic updater, which must
/// wait for successful installation before it records a trial version and reboots.
#[cfg_attr(debug_assertions, allow(dead_code))]
pub async fn install_online(url: &str) -> Result<BusyGuard> {
    let guard = BusyGuard::acquire(true)?;
    publish(UpdateProgress {
        phase: UpdatePhase::Downloading,
        detail: "Preparing firmware download".into(),
        ..UpdateProgress::default()
    })
    .await;
    if let Err(error) = begin_recovery(RecoveryState::Downloading, String::new()).await {
        publish_failure(&error).await;
        return Err(error);
    }
    let result = run_online(url).await;
    if let Err(error) = &result {
        publish_failure(error).await;
    }
    result.map(|()| guard)
}

async fn run_online(url: &str) -> Result<()> {
    if let Err(error) = download_bundle(url).await {
        let _ = tokio::fs::remove_file(ONLINE_UPDATE_BUNDLE_PART_PATH).await;
        return Err(error);
    }
    // Keep the online staging file separate from a user-uploaded manual bundle.
    // An automatic check can otherwise finish its download while a large upload is
    // still open and silently replace the file the user intended to install.
    let result = run_local(ONLINE_UPDATE_BUNDLE_PATH).await;
    if let Err(error) = tokio::fs::remove_file(ONLINE_UPDATE_BUNDLE_PATH).await
        && error.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(%error, "failed to remove staged online firmware bundle");
    }
    result
}

async fn download_bundle(url: &str) -> Result<()> {
    let _ = tokio::fs::remove_file(ONLINE_UPDATE_BUNDLE_PART_PATH).await;
    let _ = tokio::fs::remove_file(ONLINE_UPDATE_BUNDLE_PATH).await;
    publish(UpdateProgress {
        phase: UpdatePhase::Downloading,
        detail: "Connecting to update server".into(),
        ..UpdateProgress::default()
    })
    .await;

    // Resolve the final response size in parallel with the GET. Some CDNs answer
    // HEAD slowly (or not at all); waiting for it first would replace the old 10%
    // pause with a new, unnecessary "connecting" pause.
    let content_length_url = url.to_owned();
    let mut content_length_task =
        tokio::spawn(async move { remote_content_length(&content_length_url).await });
    let mut content_length_resolved = false;
    let mut total = None;
    publish_download_progress(0, None).await?;

    let mut child = tokio::process::Command::new("curl")
        .kill_on_drop(true)
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--connect-timeout",
            "15",
            "--speed-limit",
            "1024",
            "--speed-time",
            "30",
            "--max-filesize",
            MAX_BUNDLE_BYTES_ARG,
            "--max-time",
            DOWNLOAD_TIMEOUT_ARG,
            "--retry",
            "2",
            "--output",
            ONLINE_UPDATE_BUNDLE_PART_PATH,
            url,
        ])
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to start firmware download")?;
    let mut stderr = child.stderr.take();
    let mut wait = Box::pin(child.wait());
    let mut ticker = tokio::time::interval(std::time::Duration::from_millis(250));
    let timeout = tokio::time::sleep(DOWNLOAD_TIMEOUT);
    tokio::pin!(timeout);

    let status = loop {
        tokio::select! {
            result = &mut wait => break result.context("failed to wait for firmware download")?,
            () = &mut timeout => {
                anyhow::bail!("firmware download timed out after 30 minutes");
            }
            result = &mut content_length_task, if !content_length_resolved => {
                content_length_resolved = true;
                total = result.ok().flatten();
                anyhow::ensure!(
                    total.is_none_or(|bytes| bytes <= MAX_BUNDLE_BYTES),
                    "firmware bundle exceeds the 1 GiB download limit"
                );
                let done = tokio::fs::metadata(ONLINE_UPDATE_BUNDLE_PART_PATH)
                    .await
                    .map_or(0, |metadata| metadata.len());
                publish_download_progress(done, total).await?;
            }
            _ = ticker.tick() => {
                let done = tokio::fs::metadata(ONLINE_UPDATE_BUNDLE_PART_PATH)
                    .await
                    .map_or(0, |metadata| metadata.len());
                publish_download_progress(done, total).await?;
            }
        }
    };
    if !content_length_resolved {
        content_length_task.abort();
        let _ = content_length_task.await;
    }

    let mut error_output = String::new();
    if let Some(ref mut pipe) = stderr {
        let _ = pipe.read_to_string(&mut error_output).await;
    }
    if !status.success() {
        let _ = tokio::fs::remove_file(ONLINE_UPDATE_BUNDLE_PART_PATH).await;
        anyhow::bail!(
            "firmware download failed: {}",
            error_output.trim().trim_start_matches("curl:").trim()
        );
    }

    let final_size = tokio::fs::metadata(ONLINE_UPDATE_BUNDLE_PART_PATH)
        .await
        .context("failed to inspect downloaded firmware bundle")?
        .len();
    ensure_bundle_size(final_size)?;
    tokio::fs::rename(ONLINE_UPDATE_BUNDLE_PART_PATH, ONLINE_UPDATE_BUNDLE_PATH)
        .await
        .context("failed to stage downloaded firmware bundle")?;
    Ok(())
}

async fn remote_content_length(url: &str) -> Option<u64> {
    let output = tokio::process::Command::new("curl")
        .kill_on_drop(true)
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--max-time",
            "15",
            "--head",
            url,
        ])
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_content_length(&String::from_utf8_lossy(&output.stdout))
}

fn parse_content_length(headers: &str) -> Option<u64> {
    // `curl --location --head` emits one HTTP header block per redirect. Reset at
    // every status line so a proxy/redirect Content-Length can never masquerade as
    // the size of a final chunked response.
    let mut content_length = None;
    for line in headers.lines() {
        if line.starts_with("HTTP/") {
            content_length = None;
            continue;
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-length") {
            content_length = value.trim().parse::<u64>().ok().filter(|value| *value > 0);
        }
    }
    content_length
}

fn percentage(done: u64, total: Option<u64>) -> Option<u8> {
    let total = total.filter(|value| *value > 0)?;
    let scaled = u128::from(done) * 100 / u128::from(total);
    Some(u8::try_from(scaled.min(100)).unwrap_or(100))
}

fn ensure_bundle_size(bytes: u64) -> Result<()> {
    anyhow::ensure!(
        bytes <= MAX_BUNDLE_BYTES,
        "firmware bundle exceeds the 1 GiB download limit"
    );
    Ok(())
}

async fn publish_download_progress(done: u64, total: Option<u64>) -> Result<()> {
    ensure_bundle_size(done)?;
    publish(UpdateProgress {
        phase: UpdatePhase::Downloading,
        phase_progress: percentage(done, total),
        bytes_done: Some(done),
        bytes_total: total,
        detail: "Downloading firmware bundle".into(),
        ..UpdateProgress::default()
    })
    .await;
    Ok(())
}

async fn run_local(path: &str) -> Result<()> {
    let rauc = prepare_rauc_install().await?;
    run_local_with_rauc(path, rauc).await
}

async fn prepare_rauc_install() -> Result<Rauc> {
    let rauc = Rauc::connect().await?;
    let last_error_baseline = rauc.last_error().await?;
    begin_recovery(RecoveryState::Installing, last_error_baseline).await?;
    Ok(rauc)
}

async fn run_local_with_rauc(path: &str, rauc: Rauc) -> Result<()> {
    publish(UpdateProgress {
        phase: UpdatePhase::Verifying,
        detail: "Checking firmware bundle".into(),
        ..UpdateProgress::default()
    })
    .await;

    let (progress_tx, mut progress_rx) = tokio::sync::watch::channel(InstallProgress::default());
    let install = rauc.install_and_wait(path, progress_tx, INSTALL_WARNING_AFTER);
    tokio::pin!(install);
    let mut signature_verified = false;

    loop {
        tokio::select! {
            result = &mut install => {
                result?;
                if let Err(marker_error) = mark_recovery_ready().await {
                    tracing::warn!(%marker_error, "failed to persist completed firmware state");
                }
                publish(UpdateProgress {
                    phase: UpdatePhase::ReadyToReboot,
                    detail: "Firmware installed and verified".into(),
                    signature_verified: true,
                    ..UpdateProgress::default()
                }).await;
                return Ok(());
            }
            changed = progress_rx.changed() => {
                if changed.is_err() {
                    continue;
                }
                let normalized = normalize_rauc_progress(&progress_rx.borrow().clone());
                if matches!(normalized.phase, UpdatePhase::Writing | UpdatePhase::Finalizing) {
                    // RAUC only begins writing after the bundle signature and
                    // compatibility checks have succeeded.
                    signature_verified = true;
                }
                publish(UpdateProgress {
                    phase: normalized.phase,
                    phase_progress: normalized.phase_progress,
                    overall_progress: normalized.overall_progress,
                    detail: normalized.detail,
                    signature_verified,
                    ..UpdateProgress::default()
                }).await;
            }
        }
    }
}

#[must_use]
pub fn normalize_rauc_progress(progress: &InstallProgress) -> NormalizedRaucProgress {
    let message = progress.message.to_ascii_lowercase();
    let percentage = u8::try_from(progress.percentage.clamp(0, 100)).unwrap_or_default();

    let (phase, phase_progress, overall_progress) = if contains_any(
        &message,
        &["sync", "final", "activate", "marking", "cleanup", "unmount"],
    ) {
        (UpdatePhase::Finalizing, None, None)
    } else if contains_any(&message, &["download", "fetch"]) {
        // This is only a fallback for installs started outside the coordinator. Our
        // own online path reports exact bytes while staging the bundle.
        (UpdatePhase::Downloading, None, None)
    } else if contains_any(
        &message,
        &[
            "check",
            "verif",
            "signature",
            "bundle",
            "determin",
            "slot state",
            "prepar",
        ],
    ) || percentage < 20
    {
        (UpdatePhase::Verifying, None, None)
    } else if percentage >= 99 {
        (UpdatePhase::Finalizing, None, None)
    } else {
        // RAUC's percentage is cumulative across its nested install operation, not
        // a phase-local byte counter. Expose it explicitly as overall progress so
        // the UI never labels it "67% written". The final unmeasured sync remains
        // an indeterminate phase instead of parking a determinate bar at 99%.
        (UpdatePhase::Writing, None, Some(percentage.min(98)))
    };

    NormalizedRaucProgress {
        phase,
        phase_progress,
        overall_progress,
        detail: progress.message.clone(),
    }
}

fn recovery_lock() -> &'static Mutex<()> {
    RECOVERY_LOCK.get_or_init(|| Mutex::new(()))
}

async fn read_recovery_marker_unlocked() -> Result<Option<RecoveryMarker>> {
    match tokio::fs::read(RECOVERY_MARKER_PATH).await {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .context("failed to parse firmware recovery marker"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).context("failed to read firmware recovery marker"),
    }
}

async fn write_recovery_marker_unlocked(marker: &RecoveryMarker) -> Result<()> {
    let bytes =
        serde_json::to_vec(marker).context("failed to serialize firmware recovery marker")?;
    tokio::fs::write(RECOVERY_MARKER_PART_PATH, bytes)
        .await
        .context("failed to write firmware recovery marker")?;
    tokio::fs::rename(RECOVERY_MARKER_PART_PATH, RECOVERY_MARKER_PATH)
        .await
        .context("failed to publish firmware recovery marker")?;
    Ok(())
}

async fn begin_recovery(state: RecoveryState, last_error_baseline: String) -> Result<()> {
    let _lock = recovery_lock().lock().await;
    write_recovery_marker_unlocked(&RecoveryMarker {
        state,
        last_error_baseline,
        last_error: String::new(),
    })
    .await
}

async fn mark_recovery_ready() -> Result<()> {
    let _lock = recovery_lock().lock().await;
    let Some(mut marker) = read_recovery_marker_unlocked().await? else {
        return Ok(());
    };
    marker.state = RecoveryState::ReadyToReboot;
    marker.last_error.clear();
    write_recovery_marker_unlocked(&marker).await
}

async fn mark_recovery_failed(last_error: &str) -> Result<()> {
    let _lock = recovery_lock().lock().await;
    let Some(mut marker) = read_recovery_marker_unlocked().await? else {
        return Ok(());
    };
    marker.state = RecoveryState::Failed;
    marker.last_error = last_error.to_owned();
    write_recovery_marker_unlocked(&marker).await
}

async fn current_recovery_marker() -> Result<Option<RecoveryMarker>> {
    let _lock = recovery_lock().lock().await;
    read_recovery_marker_unlocked().await
}

/// Record and reconstruct an installation that is running in RAUC but no longer
/// has an in-process coordinator (for example after snapdog-ctrl restarted).
pub async fn observe_rauc_install(
    rauc: &Rauc,
    progress: Option<&InstallProgress>,
) -> UpdateProgress {
    let marker = current_recovery_marker().await.unwrap_or_else(|error| {
        tracing::warn!(%error, "failed to read firmware recovery state");
        None
    });
    if !marker.is_some_and(|marker| marker.state == RecoveryState::Installing) {
        match rauc.last_error().await {
            Ok(baseline) => {
                if let Err(error) = begin_recovery(RecoveryState::Installing, baseline).await {
                    tracing::warn!(%error, "failed to persist externally observed RAUC installation");
                }
            }
            Err(error) => {
                // Without an authoritative baseline a stale previous RAUC error
                // could be misclassified as this install's failure. Leave the
                // marker absent and retry on the next status poll.
                tracing::warn!(%error, "RAUC LastError unavailable; deferring recovery marker");
            }
        }
    }

    let normalized = progress.map_or_else(
        || NormalizedRaucProgress {
            phase: UpdatePhase::Verifying,
            phase_progress: None,
            overall_progress: None,
            detail: "RAUC installation in progress".into(),
        },
        normalize_rauc_progress,
    );
    let signature_verified = matches!(
        normalized.phase,
        UpdatePhase::Writing | UpdatePhase::Finalizing
    );
    UpdateProgress {
        phase: normalized.phase,
        phase_progress: normalized.phase_progress,
        overall_progress: normalized.overall_progress,
        detail: normalized.detail,
        signature_verified,
        ..UpdateProgress::default()
    }
}

fn terminal_recovery_progress(
    marker: &RecoveryMarker,
    current_last_error: &str,
    pending_boot_slot: Option<bool>,
) -> UpdateProgress {
    match marker.state {
        RecoveryState::ReadyToReboot => UpdateProgress {
            phase: UpdatePhase::ReadyToReboot,
            detail: "Firmware installed and verified".into(),
            signature_verified: true,
            ..UpdateProgress::default()
        },
        RecoveryState::Failed => UpdateProgress {
            phase: UpdatePhase::Failed,
            detail: "Firmware update failed".into(),
            last_error: if marker.last_error.is_empty() {
                "Firmware installation failed".into()
            } else {
                marker.last_error.clone()
            },
            ..UpdateProgress::default()
        },
        RecoveryState::Downloading => UpdateProgress {
            phase: UpdatePhase::Failed,
            detail: "Firmware update failed".into(),
            last_error: "Firmware download was interrupted".into(),
            ..UpdateProgress::default()
        },
        RecoveryState::Installing => {
            if !current_last_error.trim().is_empty()
                && current_last_error != marker.last_error_baseline
            {
                UpdateProgress {
                    phase: UpdatePhase::Failed,
                    detail: "Firmware update failed".into(),
                    last_error: current_last_error.to_owned(),
                    ..UpdateProgress::default()
                }
            } else if pending_boot_slot == Some(true) {
                UpdateProgress {
                    phase: UpdatePhase::ReadyToReboot,
                    detail: "Firmware installed and verified".into(),
                    signature_verified: true,
                    ..UpdateProgress::default()
                }
            } else if pending_boot_slot == Some(false) {
                UpdateProgress {
                    phase: UpdatePhase::Failed,
                    detail: "Firmware update failed".into(),
                    last_error: "Firmware installation ended without activating a boot slot".into(),
                    ..UpdateProgress::default()
                }
            } else {
                UpdateProgress {
                    phase: UpdatePhase::Finalizing,
                    detail: "Waiting for RAUC slot state".into(),
                    ..UpdateProgress::default()
                }
            }
        }
    }
}

/// Resolve a retained or externally observed update once RAUC is idle. A marker
/// that was still active can never collapse back to `idle`: it becomes either a
/// verified reboot target or an explicit failure.
pub async fn recover_rauc_terminal(
    current_last_error: &str,
    pending_boot_slot: Option<bool>,
) -> Option<UpdateProgress> {
    let marker = match current_recovery_marker().await {
        Ok(marker) => marker,
        Err(error) => {
            tracing::warn!(%error, "failed to read firmware recovery state");
            return None;
        }
    };

    let Some(marker) = marker else {
        return (pending_boot_slot == Some(true)).then(|| UpdateProgress {
            phase: UpdatePhase::ReadyToReboot,
            detail: "Firmware installed and verified".into(),
            signature_verified: true,
            ..UpdateProgress::default()
        });
    };
    let progress = terminal_recovery_progress(&marker, current_last_error, pending_boot_slot);
    let persist_result = match progress.phase {
        UpdatePhase::ReadyToReboot => mark_recovery_ready().await,
        UpdatePhase::Failed => mark_recovery_failed(&progress.last_error).await,
        _ => Ok(()),
    };
    if let Err(error) = persist_result {
        tracing::warn!(%error, "failed to persist recovered firmware terminal state");
    }
    if matches!(
        progress.phase,
        UpdatePhase::ReadyToReboot | UpdatePhase::Failed
    ) {
        for path in [
            UPDATE_BUNDLE_PATH,
            UPDATE_BUNDLE_PART_PATH,
            ONLINE_UPDATE_BUNDLE_PATH,
            ONLINE_UPDATE_BUNDLE_PART_PATH,
        ] {
            if let Err(error) = tokio::fs::remove_file(path).await
                && error.kind() != std::io::ErrorKind::NotFound
            {
                tracing::warn!(%error, %path, "failed to clean recovered firmware staging file");
            }
        }
    }
    Some(progress)
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_guard_serializes_uploads_and_installations() {
        assert!(!is_busy());
        assert!(!is_active());

        let installation = BusyGuard::acquire(true).expect("installation should acquire guard");
        assert!(is_busy());
        assert!(is_active());
        assert!(reserve_upload().is_err());
        drop(installation);

        let upload = reserve_upload().expect("upload should acquire released guard");
        assert!(is_busy());
        assert!(!is_active());
        assert!(BusyGuard::acquire(true).is_err());
        drop(upload);

        assert!(!is_busy());
        assert!(!is_active());
    }

    #[test]
    fn download_percentage_is_bounded_and_unknown_without_total() {
        assert_eq!(percentage(50, Some(100)), Some(50));
        assert_eq!(percentage(150, Some(100)), Some(100));
        assert_eq!(percentage(u64::MAX, Some(u64::MAX)), Some(100));
        assert_eq!(percentage(50, None), None);
        assert_eq!(percentage(50, Some(0)), None);
    }

    #[test]
    fn bundle_size_limit_is_inclusive_and_hard_bounded() {
        assert!(ensure_bundle_size(MAX_BUNDLE_BYTES).is_ok());
        assert!(ensure_bundle_size(MAX_BUNDLE_BYTES + 1).is_err());
        assert!(ensure_bundle_size(u64::MAX).is_err());
    }

    #[test]
    fn recovery_marker_serializes_the_last_error_baseline() {
        let marker = RecoveryMarker {
            state: RecoveryState::Installing,
            last_error_baseline: "old failure".into(),
            last_error: String::new(),
        };

        assert_eq!(
            serde_json::to_value(marker).expect("recovery marker should serialize"),
            serde_json::json!({
                "state": "installing",
                "last_error_baseline": "old failure",
                "last_error": "",
            })
        );
    }

    #[test]
    fn restart_recovery_surfaces_a_new_rauc_error() {
        let marker = RecoveryMarker {
            state: RecoveryState::Installing,
            last_error_baseline: "stale error".into(),
            last_error: String::new(),
        };

        let recovered = terminal_recovery_progress(&marker, "signature rejected", Some(false));
        assert_eq!(recovered.phase, UpdatePhase::Failed);
        assert_eq!(recovered.last_error, "signature rejected");
    }

    #[test]
    fn restart_recovery_never_treats_unactivated_install_as_success() {
        let marker = RecoveryMarker {
            state: RecoveryState::Installing,
            last_error_baseline: "stale error".into(),
            last_error: String::new(),
        };

        let interrupted = terminal_recovery_progress(&marker, "stale error", Some(false));
        assert_eq!(interrupted.phase, UpdatePhase::Failed);
        assert!(!interrupted.last_error.is_empty());

        let staged = terminal_recovery_progress(&marker, "stale error", Some(true));
        assert_eq!(staged.phase, UpdatePhase::ReadyToReboot);
        assert!(staged.signature_verified);

        let unavailable = terminal_recovery_progress(&marker, "stale error", None);
        assert_eq!(unavailable.phase, UpdatePhase::Finalizing);
        assert!(unavailable.last_error.is_empty());
    }

    #[test]
    fn interrupted_download_recovers_as_failure() {
        let marker = RecoveryMarker {
            state: RecoveryState::Downloading,
            last_error_baseline: String::new(),
            last_error: String::new(),
        };

        let recovered = terminal_recovery_progress(&marker, "stale RAUC error", None);
        assert_eq!(recovered.phase, UpdatePhase::Failed);
        assert_eq!(recovered.last_error, "Firmware download was interrupted");
    }

    #[test]
    fn content_length_comes_only_from_the_final_http_response() {
        let redirected = "HTTP/1.1 301 Moved Permanently\r\n\
            Content-Length: 42\r\n\r\n\
            HTTP/2 200\r\n\
            content-length: 83886080\r\n\r\n";
        assert_eq!(parse_content_length(redirected), Some(83_886_080));

        let final_response_is_chunked = "HTTP/1.1 302 Found\n\
            Content-Length: 42\n\n\
            HTTP/2 200\n\
            transfer-encoding: chunked\n\n";
        assert_eq!(parse_content_length(final_response_is_chunked), None);
        assert_eq!(
            parse_content_length("HTTP/2 200\ncontent-length: 0\n"),
            None
        );
        assert_eq!(
            parse_content_length("HTTP/2 200\ncontent-length: nope\n"),
            None
        );
    }

    #[test]
    fn verification_and_finalization_never_claim_precise_progress() {
        let verifying = normalize_rauc_progress(&InstallProgress {
            percentage: 10,
            message: "Checking bundle".into(),
            depth: 2,
        });
        assert_eq!(verifying.phase, UpdatePhase::Verifying);
        assert_eq!(verifying.phase_progress, None);
        assert_eq!(verifying.overall_progress, None);

        let finalizing = normalize_rauc_progress(&InstallProgress {
            percentage: 99,
            message: "Copying image".into(),
            depth: 3,
        });
        assert_eq!(finalizing.phase, UpdatePhase::Finalizing);
        assert_eq!(finalizing.phase_progress, None);
        assert_eq!(finalizing.overall_progress, None);
    }

    #[test]
    fn image_write_exposes_rauc_progress_only_as_overall() {
        let writing = normalize_rauc_progress(&InstallProgress {
            percentage: 67,
            message: "Copying image to rootfs.1".into(),
            depth: 3,
        });
        assert_eq!(writing.phase, UpdatePhase::Writing);
        assert_eq!(writing.phase_progress, None);
        assert_eq!(writing.overall_progress, Some(67));
    }

    #[test]
    fn normalization_handles_preflight_case_and_out_of_range_values() {
        let preflight = normalize_rauc_progress(&InstallProgress {
            percentage: 20,
            message: "Determining slot states done.".into(),
            depth: 2,
        });
        assert_eq!(preflight.phase, UpdatePhase::Verifying);
        assert_eq!(preflight.phase_progress, None);

        let uppercase_write = normalize_rauc_progress(&InstallProgress {
            percentage: 67,
            message: "COPYING IMAGE TO ROOTFS.1".into(),
            depth: 3,
        });
        assert_eq!(uppercase_write.phase, UpdatePhase::Writing);
        assert_eq!(uppercase_write.phase_progress, None);
        assert_eq!(uppercase_write.overall_progress, Some(67));

        let negative = normalize_rauc_progress(&InstallProgress {
            percentage: -50,
            message: "Checking bundle".into(),
            depth: 2,
        });
        assert_eq!(negative.phase, UpdatePhase::Verifying);
        assert_eq!(negative.phase_progress, None);

        let over_complete = normalize_rauc_progress(&InstallProgress {
            percentage: 150,
            message: "Copying image to rootfs.1".into(),
            depth: 3,
        });
        assert_eq!(over_complete.phase, UpdatePhase::Finalizing);
        assert_eq!(over_complete.phase_progress, None);
    }

    #[test]
    fn external_download_fallback_never_invents_byte_or_phase_progress() {
        let downloading = normalize_rauc_progress(&InstallProgress {
            percentage: 42,
            message: "Fetching bundle".into(),
            depth: 2,
        });

        assert_eq!(downloading.phase, UpdatePhase::Downloading);
        assert_eq!(downloading.phase_progress, None);
        assert_eq!(downloading.overall_progress, None);
    }

    #[test]
    fn terminal_progress_serializes_with_the_stable_api_names() {
        let ready = UpdateProgress {
            phase: UpdatePhase::ReadyToReboot,
            detail: "Firmware installed and verified".into(),
            signature_verified: true,
            ..UpdateProgress::default()
        };
        assert_eq!(
            serde_json::to_value(ready).expect("ready progress should serialize"),
            serde_json::json!({
                "phase": "ready_to_reboot",
                "phase_progress": null,
                "overall_progress": null,
                "bytes_done": null,
                "bytes_total": null,
                "detail": "Firmware installed and verified",
                "last_error": "",
                "signature_verified": true,
            })
        );

        let failed = UpdateProgress {
            phase: UpdatePhase::Failed,
            detail: "Firmware update failed".into(),
            last_error: "RAUC rejected the bundle".into(),
            ..UpdateProgress::default()
        };
        let serialized = serde_json::to_value(failed).expect("failed progress should serialize");
        assert_eq!(serialized["phase"], "failed");
        assert_eq!(serialized["last_error"], "RAUC rejected the bundle");
        assert_eq!(serialized["phase_progress"], serde_json::Value::Null);
        assert_eq!(serialized["overall_progress"], serde_json::Value::Null);
    }
}
