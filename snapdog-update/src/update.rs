// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

use crate::client::{SlotStatus, SystemInfo, UpdateClient};
use crate::error::{Result, UpgradeError};
use crate::output::Reporter;
use std::future::Future;
use std::path::Path;
use std::time::{Duration, Instant};
use tokio::time::sleep;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum ImageType {
    RaucBundle,
    RawFlashPrepare,
    RawFlashConfirm,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum RunOutcome {
    Completed,
    RawFlashConfirmationRequired {
        challenge: String,
        expires_in_seconds: u64,
    },
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum BoardKind {
    Pi3,
    Pi4,
    Pi5,
    Zero2w,
}

/// Identity of a booted RAUC slot: its slot name (rootfs.0/rootfs.1) and the
/// installed bundle version. The bundle version is the authoritative post-reboot
/// signal — unlike the ctrl's `system_info.version` (a `git describe` string that
/// two local builds off the same commit share), the per-slot bundle version is
/// exactly what was flashed and always differs from the previously-booted bundle.
#[derive(Debug, Clone, Eq, PartialEq)]
struct BootedSlot {
    name: String,
    version: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
enum RebootExpectation {
    Rauc {
        previous_version: String,
        previous_booted_slot: Option<BootedSlot>,
    },
    RawFlash {
        previous_version: String,
    },
}

pub struct UpgradeManager {
    client: UpdateClient,
    image_path: Option<std::path::PathBuf>,
    image_type: ImageType,
    raw_confirmation: Option<String>,
    timeout: Duration,
    poll_interval: Duration,
    reporter: Reporter,
}

impl UpgradeManager {
    pub fn new(
        base_url: &str,
        image_path: Option<&Path>,
        force_raw: bool,
        raw_confirmation: Option<String>,
        timeout: Duration,
        poll_interval: Duration,
        reporter: Reporter,
    ) -> Result<Self> {
        if timeout.is_zero() {
            return Err(UpgradeError::InvalidArgument(
                "--timeout-mins must be greater than zero".to_string(),
            ));
        }
        if poll_interval.is_zero() {
            return Err(UpgradeError::InvalidArgument(
                "--poll-secs must be greater than zero".to_string(),
            ));
        }

        let image_type = resolve_image_type(image_path, force_raw, raw_confirmation.as_deref())?;

        Ok(Self {
            client: UpdateClient::new(base_url)?,
            image_path: image_path.map(Path::to_path_buf),
            image_type,
            raw_confirmation,
            timeout,
            poll_interval,
            reporter,
        })
    }

    pub async fn run(&mut self, password: Option<&str>) -> Result<RunOutcome> {
        self.reporter
            .status("preflight", "Starting preflight checks...");
        self.client
            .preflight_auth(password, self.reporter.interactive())
            .await?;

        let deadline = deadline_from_now(self.timeout)?;
        let info = run_with_deadline(
            deadline,
            "fetch system information",
            self.client.system_info(),
        )
        .await?;
        self.reporter.status(
            "target",
            format!(
                "Connected to '{}' (active version: '{}', board: '{}', uptime: {}s)",
                info.hostname, info.version, info.board_model, info.uptime_seconds
            ),
        );

        if let Some(path) = &self.image_path {
            validate_board_compatibility(path, &info.board_model)?;
        }

        self.check_health(deadline).await?;

        match self.image_type {
            ImageType::RaucBundle => {
                let previous_booted_slot = self.booted_slot(deadline).await?;
                self.run_rauc_flow(deadline).await?;
                self.wait_for_reboot(
                    &RebootExpectation::Rauc {
                        previous_version: info.version,
                        previous_booted_slot,
                    },
                    deadline,
                )
                .await?;
                Ok(RunOutcome::Completed)
            }
            ImageType::RawFlashPrepare => self.run_raw_prepare_flow(&info.version, deadline).await,
            ImageType::RawFlashConfirm => {
                self.run_raw_confirm_flow(&info.version, deadline).await?;
                Ok(RunOutcome::Completed)
            }
        }
    }

    async fn check_health(&self, deadline: Instant) -> Result<()> {
        let health =
            run_with_deadline(deadline, "fetch system health", self.client.system_health()).await?;
        if !health.ok {
            let criticals: Vec<String> = health
                .warnings
                .iter()
                .filter(|w| w.severity == "critical")
                .map(|w| w.id.clone())
                .collect();
            if !criticals.is_empty() {
                return Err(UpgradeError::SystemUnhealthy(criticals));
            }
        }
        Ok(())
    }

    async fn run_rauc_flow(&self, deadline: Instant) -> Result<()> {
        let image_path = self.image_path.as_ref().ok_or_else(|| {
            UpgradeError::InvalidArgument("RAUC update requires --file".to_string())
        })?;

        let metadata = tokio::fs::metadata(image_path).await?;
        let ui = std::sync::Arc::new(
            self.reporter
                .upload_progress(metadata.len(), "Uploading RAUC bundle..."),
        );
        let ui_clone = ui.clone();
        run_with_deadline(
            deadline,
            "upload RAUC bundle",
            self.client
                .upload_image(image_path, "/api/system/update/upload", move |sent| {
                    ui_clone.set_position(sent);
                }),
        )
        .await?;
        ui.finish_success("Upload completed successfully!");

        self.reporter
            .status("install", "Triggering RAUC installation...");
        run_with_deadline(
            deadline,
            "trigger RAUC installation",
            self.client.trigger_install(),
        )
        .await?;

        let ui_poll = self
            .reporter
            .spinner("install", "Starting RAUC installation...");

        // RAUC's InstallBundle is asynchronous, so "idle" is ambiguous: it is both
        // the pre-start state (right after trigger_install(), before the installer
        // thread spins up) and the post-finish state. Reading that early "idle" as
        // completion would reboot before anything was written to the slot. So we
        // only accept "idle" as done once we have actually observed "installing".
        let mut saw_installing = false;

        loop {
            if sleep_until_deadline(deadline, self.poll_interval, "RAUC installation")
                .await
                .is_err()
            {
                ui_poll.finish_failure("Installation timed out!");
                return Err(UpgradeError::Timeout(
                    "RAUC installation took too long".into(),
                ));
            }

            // The status endpoint can hiccup while rauc is busy writing the slot
            // (a slow or momentarily-malformed response). A transient poll error
            // must NOT abort the update — keep polling; the deadline is enforced
            // by sleep_until_deadline at the top of the loop.
            let status = match run_with_deadline(
                deadline,
                "fetch update status",
                self.client.update_status(),
            )
            .await
            {
                Ok(status) => status,
                Err(e) => {
                    ui_poll.update_message(format!("status poll failed (retrying): {e}"));
                    continue;
                }
            };

            match classify_install_status(&status.operation, &status.last_error, saw_installing) {
                InstallPoll::Installing => {
                    saw_installing = true;
                    if let Some(prog) = status.progress {
                        ui_poll.update_message(format!(
                            "Installing: {}% ({})",
                            prog.percentage, prog.message
                        ));
                    }
                }
                InstallPoll::Failed => {
                    ui_poll.finish_failure("Installation failed!");
                    return Err(UpgradeError::Failed(status.last_error));
                }
                InstallPoll::Completed => {
                    // The install is done but the device does not reboot on its own
                    // — trigger it so it boots into the freshly-installed slot. The
                    // connection drops as it goes down, so ignore the transport
                    // result.
                    let _ = self.client.reboot().await;
                    ui_poll.finish_success("Installation complete! System is rebooting...");
                    break;
                }
                InstallPoll::Waiting => {
                    ui_poll.update_message(if status.operation == "idle" {
                        "Waiting for installation to start...".to_string()
                    } else {
                        format!("Operation status: {}", status.operation)
                    });
                }
            }
        }
        Ok(())
    }

    async fn run_raw_prepare_flow(
        &self,
        old_version: &str,
        deadline: Instant,
    ) -> Result<RunOutcome> {
        let image_path = self.image_path.as_ref().ok_or_else(|| {
            UpgradeError::InvalidArgument("raw flash upload requires --file".to_string())
        })?;

        let metadata = tokio::fs::metadata(image_path).await?;
        let ui = std::sync::Arc::new(
            self.reporter
                .upload_progress(metadata.len(), "Uploading raw system image..."),
        );
        let ui_clone = ui.clone();
        let challenge = run_with_deadline(
            deadline,
            "upload raw system image",
            self.client.trigger_flash_raw(image_path, move |sent| {
                ui_clone.set_position(sent);
            }),
        )
        .await?;
        ui.finish_success("Upload completed successfully!");

        self.reporter
            .raw_flash_challenge(&challenge.challenge, challenge.expires_in_seconds);

        if let Some(typed) = self
            .reporter
            .prompt_raw_flash_confirmation(&challenge.challenge)
            .await?
        {
            if typed != challenge.challenge {
                return Err(UpgradeError::RawFlashChallengeMismatch);
            }
            self.confirm_raw_flash(&challenge.challenge, old_version, deadline)
                .await?;
            return Ok(RunOutcome::Completed);
        }

        Ok(RunOutcome::RawFlashConfirmationRequired {
            challenge: challenge.challenge,
            expires_in_seconds: challenge.expires_in_seconds,
        })
    }

    async fn run_raw_confirm_flow(&self, old_version: &str, deadline: Instant) -> Result<()> {
        let challenge = self.raw_confirmation.as_deref().ok_or_else(|| {
            UpgradeError::InvalidArgument(
                "raw flash confirmation requires --confirm-raw-flash".to_string(),
            )
        })?;
        self.confirm_raw_flash(challenge, old_version, deadline)
            .await
    }

    async fn confirm_raw_flash(
        &self,
        challenge: &str,
        old_version: &str,
        deadline: Instant,
    ) -> Result<()> {
        self.reporter
            .status("raw_flash", "Confirming pending raw flash challenge...");
        run_with_deadline(
            deadline,
            "confirm raw flash",
            self.client.confirm_flash_raw(challenge),
        )
        .await?;
        self.reporter.status(
            "raw_flash",
            "Confirmation accepted. Waiting for device reboot...",
        );
        self.wait_for_reboot(
            &RebootExpectation::RawFlash {
                previous_version: old_version.to_string(),
            },
            deadline,
        )
        .await
    }

    async fn booted_slot(&self, deadline: Instant) -> Result<Option<BootedSlot>> {
        let status = run_with_deadline(
            deadline,
            "fetch RAUC slot status",
            self.client.update_status(),
        )
        .await?;
        Ok(booted_slot(&status.slots))
    }

    async fn wait_for_reboot(
        &self,
        expectation: &RebootExpectation,
        deadline: Instant,
    ) -> Result<()> {
        let ui = self
            .reporter
            .spinner("reboot", "Waiting for the device to reboot...");

        // Single self-gating poll loop. The RAUC success signals (the booted slot's
        // bundle version or slot name changing) can only become true AFTER the new
        // slot has actually booted, so we check on every reachable poll instead of
        // first requiring a clean offline observation. That fixes two failure modes
        // of the old two-phase design: (1) if the brief offline window was ever
        // missed, the "wait for offline" phase would burn the whole deadline and
        // then report a false timeout even though the device had already come back
        // on the new slot; (2) detection no longer depends on the ctrl's
        // `git describe` version differing, which two local builds share.
        //
        // `observed_offline` is still tracked because the RawFlash path (a same-
        // version reflash) has no slot/version signal and must confirm a real reboot
        // via a low uptime — which is only trustworthy once we've seen it drop.
        let mut observed_offline = false;
        let mut backoff = Duration::from_secs(2);
        while remaining_duration(deadline).is_some() {
            if sleep_until_deadline(deadline, backoff, "device reboot")
                .await
                .is_err()
            {
                break;
            }
            match self.client.system_info().await {
                Err(_) => {
                    if !observed_offline {
                        observed_offline = true;
                        ui.update_message(
                            "Device went offline. Waiting for it to come back...".to_string(),
                        );
                    }
                    // Poll faster while it is down so we catch the recovery promptly.
                    backoff = Duration::from_secs(2);
                }
                Ok(info) => {
                    let current_booted_slot = if expectation.needs_booted_slot() {
                        run_with_deadline(
                            deadline,
                            "fetch RAUC slot status",
                            self.client.update_status(),
                        )
                        .await
                        .ok()
                        .and_then(|status| booted_slot(&status.slots))
                    } else {
                        None
                    };

                    if let Some(reason) = expectation.success_reason(
                        &info,
                        current_booted_slot.as_ref(),
                        observed_offline,
                    ) {
                        ui.finish_success(&format!(
                            "Success! Device is back online ({reason}; uptime: {}s, version: v{})",
                            info.uptime_seconds, info.version
                        ));
                        return Ok(());
                    }
                    ui.update_message(
                        expectation.pending_message(&info, current_booted_slot.as_ref()),
                    );
                    backoff = (backoff * 2).min(Duration::from_secs(10));
                }
            }
        }

        ui.finish_failure("Reboot check timed out!");
        Err(UpgradeError::Timeout(
            "Device did not recover in time".into(),
        ))
    }
}

impl RebootExpectation {
    const fn needs_booted_slot(&self) -> bool {
        matches!(self, Self::Rauc { .. })
    }

    fn success_reason(
        &self,
        info: &SystemInfo,
        current_booted_slot: Option<&BootedSlot>,
        observed_offline: bool,
    ) -> Option<String> {
        match self {
            Self::Rauc {
                previous_version,
                previous_booted_slot,
            } => {
                if info.version != *previous_version {
                    return Some(format!(
                        "version changed from v{previous_version} to v{}",
                        info.version
                    ));
                }

                if let (Some(previous), Some(current)) =
                    (previous_booted_slot.as_ref(), current_booted_slot)
                {
                    // Authoritative signal: the booted slot now runs a different
                    // bundle than before the install. Robust to identical
                    // git-describe `info.version` strings, and correctly false on a
                    // reverted trial (which lands back on the old slot + bundle).
                    if previous.version != current.version {
                        return Some(format!(
                            "installed bundle version changed from {} to {}",
                            previous.version, current.version
                        ));
                    }
                    // Fallback for a same-version reinstall to the other slot.
                    if previous.name != current.name {
                        return Some(format!(
                            "booted slot changed from {} to {}",
                            previous.name, current.name
                        ));
                    }
                }

                None
            }
            Self::RawFlash { previous_version } => {
                if info.version != *previous_version {
                    Some(format!(
                        "version changed from v{previous_version} to v{}",
                        info.version
                    ))
                } else if observed_offline && info.uptime_seconds < 180 {
                    // A raw reflash of the same version has no slot/version signal;
                    // trust a low uptime only after we actually saw it go offline, so
                    // an already-recently-booted device can't be a false positive.
                    Some("device rebooted".to_string())
                } else {
                    None
                }
            }
        }
    }

    fn pending_message(
        &self,
        info: &SystemInfo,
        current_booted_slot: Option<&BootedSlot>,
    ) -> String {
        match self {
            Self::Rauc {
                previous_version,
                previous_booted_slot,
            } => format!(
                "Device is online, but update is not verified yet (version: v{}, previous: v{}, booted bundle: {}, previous bundle: {}). Waiting...",
                info.version,
                previous_version,
                current_booted_slot.map_or("unknown", |s| s.version.as_str()),
                previous_booted_slot
                    .as_ref()
                    .map_or("unknown", |s| s.version.as_str())
            ),
            Self::RawFlash { previous_version } => format!(
                "Device is online, but reboot is not verified yet (version: v{}, previous: v{}, uptime: {}s). Waiting...",
                info.version, previous_version, info.uptime_seconds
            ),
        }
    }
}

fn booted_slot(slots: &[SlotStatus]) -> Option<BootedSlot> {
    slots
        .iter()
        .find(|slot| slot.booted)
        .map(|slot| BootedSlot {
            name: slot.name.clone(),
            version: slot.version.clone(),
        })
}

/// What a single RAUC status poll means during installation.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum InstallPoll {
    Installing,
    Completed,
    Failed,
    Waiting,
}

/// Interpret a RAUC status poll. Because `InstallBundle` is asynchronous, "idle"
/// is ambiguous — it is both the pre-start and the post-finish state. We only
/// treat "idle" as completion once "installing" has been observed at least once;
/// before that, an "idle" (or any other transient operation) means the installer
/// simply has not started yet. A non-empty `last_error` on "idle" is always a
/// failure, matching RAUC's terminal-error reporting.
fn classify_install_status(operation: &str, last_error: &str, saw_installing: bool) -> InstallPoll {
    match operation {
        "installing" => InstallPoll::Installing,
        "idle" if !last_error.is_empty() => InstallPoll::Failed,
        "idle" if saw_installing => InstallPoll::Completed,
        _ => InstallPoll::Waiting,
    }
}

async fn run_with_deadline<T, Fut>(
    deadline: Instant,
    operation: &'static str,
    future: Fut,
) -> Result<T>
where
    Fut: Future<Output = Result<T>>,
{
    let remaining = remaining_duration(deadline).ok_or_else(|| timeout_error(operation))?;
    tokio::time::timeout(remaining, future)
        .await
        .unwrap_or_else(|_| Err(timeout_error(operation)))
}

async fn sleep_until_deadline(
    deadline: Instant,
    duration: Duration,
    operation: &'static str,
) -> Result<()> {
    let remaining = remaining_duration(deadline).ok_or_else(|| timeout_error(operation))?;
    sleep(duration.min(remaining)).await;
    Ok(())
}

fn deadline_from_now(timeout: Duration) -> Result<Instant> {
    Instant::now()
        .checked_add(timeout)
        .ok_or_else(|| UpgradeError::InvalidArgument("--timeout-mins is too large".to_string()))
}

fn remaining_duration(deadline: Instant) -> Option<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|duration| !duration.is_zero())
}

fn timeout_error(operation: &str) -> UpgradeError {
    UpgradeError::Timeout(format!("{operation} timed out"))
}

fn resolve_image_type(
    image_path: Option<&Path>,
    force_raw: bool,
    raw_confirmation: Option<&str>,
) -> Result<ImageType> {
    if raw_confirmation.is_some() {
        return Ok(ImageType::RawFlashConfirm);
    }

    let image_path = image_path.ok_or_else(|| {
        UpgradeError::InvalidArgument(
            "--file is required unless --confirm-raw-flash is used".into(),
        )
    })?;

    if force_raw {
        return Ok(ImageType::RawFlashPrepare);
    }

    let ext = image_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    if ext.eq_ignore_ascii_case("raucb") {
        return Ok(ImageType::RaucBundle);
    }

    Err(UpgradeError::UnsupportedImage {
        path: image_path.display().to_string(),
    })
}

fn validate_board_compatibility(image_path: &Path, board_model: &str) -> Result<()> {
    let Some(image_board) = image_path
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(board_from_image_name)
    else {
        return Ok(());
    };

    let Some(target_board) = board_from_model(board_model) else {
        return Ok(());
    };

    if image_board == target_board {
        return Ok(());
    }

    Err(UpgradeError::IncompatibleBoard {
        target: target_board.as_str().to_string(),
        image: image_board.as_str().to_string(),
    })
}

fn board_from_image_name(file_name: &str) -> Option<BoardKind> {
    let file_name = file_name.to_lowercase();
    if file_name.contains("zero2w") || file_name.contains("zero-2-w") {
        Some(BoardKind::Zero2w)
    } else if file_name.contains("pi5") || file_name.contains("pi-5") || file_name.contains("rpi5")
    {
        Some(BoardKind::Pi5)
    } else if file_name.contains("pi4") || file_name.contains("pi-4") || file_name.contains("rpi4")
    {
        Some(BoardKind::Pi4)
    } else if file_name.contains("pi3") || file_name.contains("pi-3") || file_name.contains("rpi3")
    {
        Some(BoardKind::Pi3)
    } else {
        None
    }
}

fn board_from_model(board_model: &str) -> Option<BoardKind> {
    let board_model = board_model.to_lowercase();
    if board_model.contains("zero 2") || board_model.contains("zero2") {
        Some(BoardKind::Zero2w)
    } else if board_model.contains("pi 5") || board_model.contains("pi5") {
        Some(BoardKind::Pi5)
    } else if board_model.contains("pi 4") || board_model.contains("pi4") {
        Some(BoardKind::Pi4)
    } else if board_model.contains("pi 3") || board_model.contains("pi3") {
        Some(BoardKind::Pi3)
    } else {
        None
    }
}

impl BoardKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Pi3 => "pi3",
            Self::Pi4 => "pi4",
            Self::Pi5 => "pi5",
            Self::Zero2w => "zero2w",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::{OutputFormat, Reporter};

    fn reporter() -> Reporter {
        Reporter::new(OutputFormat::Json, true, true)
    }

    fn system_info(version: &str, uptime_seconds: u64) -> SystemInfo {
        SystemInfo {
            hostname: "snapdog-test".to_string(),
            version: version.to_string(),
            board_model: "Raspberry Pi 4 Model B".to_string(),
            uptime_seconds,
        }
    }

    fn slot(name: &str, booted: bool) -> SlotStatus {
        SlotStatus {
            name: name.to_string(),
            class: "rootfs".to_string(),
            device: format!("/dev/{name}"),
            state: "good".to_string(),
            version: "1.0.0".to_string(),
            booted,
        }
    }

    fn booted(name: &str, version: &str) -> BootedSlot {
        BootedSlot {
            name: name.to_string(),
            version: version.to_string(),
        }
    }

    #[test]
    fn resolves_rauc_bundle_without_raw_flag() {
        let manager = UpgradeManager::new(
            "http://127.0.0.1",
            Some(Path::new("snapdog-os-pi4.raucb")),
            false,
            None,
            Duration::from_secs(60),
            Duration::from_secs(1),
            reporter(),
        )
        .unwrap();
        assert_eq!(manager.image_type, ImageType::RaucBundle);
    }

    #[test]
    fn resolves_rauc_bundle_case_insensitively() {
        let manager = UpgradeManager::new(
            "http://127.0.0.1",
            Some(Path::new("snapdog-os-pi4.RAUCB")),
            false,
            None,
            Duration::from_secs(60),
            Duration::from_secs(1),
            reporter(),
        )
        .unwrap();
        assert_eq!(manager.image_type, ImageType::RaucBundle);
    }

    #[test]
    fn rejects_raw_image_without_raw_flag() {
        let result = UpgradeManager::new(
            "http://127.0.0.1",
            Some(Path::new("snapdog-os-pi4.img.gz")),
            false,
            None,
            Duration::from_secs(60),
            Duration::from_secs(1),
            reporter(),
        );
        let Err(err) = result else {
            panic!("raw image without --raw should be rejected");
        };
        assert!(matches!(err, UpgradeError::UnsupportedImage { .. }));
    }

    #[test]
    fn resolves_raw_confirmation_without_file() {
        let manager = UpgradeManager::new(
            "http://127.0.0.1",
            None,
            true,
            Some("ABC123".to_string()),
            Duration::from_secs(60),
            Duration::from_secs(1),
            reporter(),
        )
        .unwrap();
        assert_eq!(manager.image_type, ImageType::RawFlashConfirm);
    }

    #[test]
    fn detects_board_mismatch() {
        let err = validate_board_compatibility(
            Path::new("snapdog-os-pi5-0.3.0.raucb"),
            "Raspberry Pi 4 Model B",
        )
        .unwrap_err();
        assert!(matches!(
            err,
            UpgradeError::IncompatibleBoard {
                target,
                image
            } if target == "pi4" && image == "pi5"
        ));
    }

    #[test]
    fn accepts_matching_zero2w_board() {
        validate_board_compatibility(
            Path::new("snapdog-os-zero2w-0.3.0.raucb"),
            "Raspberry Pi Zero 2 W Rev 1.0",
        )
        .unwrap();
    }

    #[test]
    fn detects_rpi_filename_board_mismatch() {
        let err = validate_board_compatibility(
            Path::new("snapdog-os-rpi3-0.3.0.raucb"),
            "Raspberry Pi 5 Model B",
        )
        .unwrap_err();
        assert!(matches!(
            err,
            UpgradeError::IncompatibleBoard {
                target,
                image
            } if target == "pi5" && image == "pi3"
        ));
    }

    #[test]
    fn finds_booted_slot() {
        assert_eq!(
            booted_slot(&[slot("rootfs.0", false), slot("rootfs.1", true)]),
            Some(booted("rootfs.1", "1.0.0"))
        );
    }

    #[test]
    fn rauc_reboot_success_accepts_version_change() {
        let expectation = RebootExpectation::Rauc {
            previous_version: "1.0.0".to_string(),
            previous_booted_slot: Some(booted("rootfs.0", "0.2.3-wifi")),
        };

        let reason = expectation.success_reason(
            &system_info("1.1.0", 42),
            Some(&booted("rootfs.0", "0.2.3-wifi")),
            true,
        );
        assert!(reason.is_some_and(|r| r.contains("version changed")));
    }

    #[test]
    fn rauc_reboot_success_accepts_slot_change_with_same_version() {
        // Identical ctrl version AND bundle version, different slot: a same-version
        // reinstall to the other slot still counts as a booted update.
        let expectation = RebootExpectation::Rauc {
            previous_version: "1.0.0".to_string(),
            previous_booted_slot: Some(booted("rootfs.0", "0.3.0")),
        };

        let reason = expectation.success_reason(
            &system_info("1.0.0", 42),
            Some(&booted("rootfs.1", "0.3.0")),
            true,
        );
        assert!(reason.is_some_and(|r| r.contains("booted slot changed")));
    }

    #[test]
    fn rauc_reboot_success_accepts_bundle_version_change_despite_identical_ctrl_version() {
        // The exact false-timeout this fixes: two local builds off the same commit
        // share a `git describe` `system_info.version`, so ONLY the per-slot bundle
        // version distinguishes old from new.
        let expectation = RebootExpectation::Rauc {
            previous_version: "v0.6.1-32-g980bd32-dirty".to_string(),
            previous_booted_slot: Some(booted("rootfs.1", "0.2.3-wifi")),
        };

        let reason = expectation.success_reason(
            &system_info("v0.6.1-32-g980bd32-dirty", 30),
            Some(&booted("rootfs.0", "0.2.4-wifi")),
            true,
        );
        assert!(reason.is_some_and(|r| {
            r.contains("installed bundle version changed from 0.2.3-wifi to 0.2.4-wifi")
        }));
    }

    #[test]
    fn rauc_reboot_reverted_trial_is_not_success() {
        // Trial booted the new slot, failed health, and auto-reverted: back on the
        // OLD slot + bundle with an identical ctrl version. Must NOT be reported as
        // success even though we saw the device go offline during the trial.
        let expectation = RebootExpectation::Rauc {
            previous_version: "v0.6.1-32-g980bd32-dirty".to_string(),
            previous_booted_slot: Some(booted("rootfs.1", "0.2.3-wifi")),
        };

        assert_eq!(
            expectation.success_reason(
                &system_info("v0.6.1-32-g980bd32-dirty", 30),
                Some(&booted("rootfs.1", "0.2.3-wifi")),
                true,
            ),
            None
        );
    }

    #[test]
    fn rauc_reboot_does_not_accept_recent_uptime_alone() {
        let expectation = RebootExpectation::Rauc {
            previous_version: "1.0.0".to_string(),
            previous_booted_slot: Some(booted("rootfs.0", "0.3.0")),
        };

        assert_eq!(
            expectation.success_reason(
                &system_info("1.0.0", 42),
                Some(&booted("rootfs.0", "0.3.0")),
                true,
            ),
            None
        );
    }

    #[test]
    fn idle_before_installing_is_not_completion() {
        // The bug this guards: the pre-start "idle" must not be read as "done".
        assert_eq!(
            classify_install_status("idle", "", false),
            InstallPoll::Waiting
        );
    }

    #[test]
    fn idle_after_installing_is_completion() {
        assert_eq!(
            classify_install_status("idle", "", true),
            InstallPoll::Completed
        );
    }

    #[test]
    fn installing_is_reported_and_arms_completion() {
        assert_eq!(
            classify_install_status("installing", "", false),
            InstallPoll::Installing
        );
    }

    #[test]
    fn idle_with_error_is_failure_regardless_of_progress() {
        assert_eq!(
            classify_install_status("idle", "signature invalid", false),
            InstallPoll::Failed
        );
        assert_eq!(
            classify_install_status("idle", "verity hash mismatch", true),
            InstallPoll::Failed
        );
    }

    #[test]
    fn unknown_operation_keeps_waiting() {
        assert_eq!(
            classify_install_status("starting", "", false),
            InstallPoll::Waiting
        );
    }

    #[test]
    fn raw_flash_reboot_accepts_recent_uptime() {
        let expectation = RebootExpectation::RawFlash {
            previous_version: "1.0.0".to_string(),
        };

        let reason = expectation.success_reason(&system_info("1.0.0", 42), None, true);
        assert!(reason.is_some_and(|r| r == "device rebooted"));
    }

    #[test]
    fn raw_flash_reboot_requires_observed_offline_for_uptime_signal() {
        // A device with a coincidentally-low uptime that we never saw drop must not
        // be mistaken for a completed reflash.
        let expectation = RebootExpectation::RawFlash {
            previous_version: "1.0.0".to_string(),
        };

        assert_eq!(
            expectation.success_reason(&system_info("1.0.0", 42), None, false),
            None
        );
    }

    #[test]
    fn installing_status_with_percentage_progress_decodes() {
        // Regression: the device populates progress as {"percentage": <int>, ...}
        // ONLY while installing. A field mismatch (percent vs percentage) failed the
        // whole status decode on every poll during the install window, so
        // `saw_installing` was never set and the client waited forever for an install
        // that had already finished. This is the exact body observed on-device.
        let body = r#"{"operation":"installing","progress":{"percentage":40,"message":"Determining target install group done."},"last_error":"","slots":[]}"#;
        let status: crate::client::UpdateStatus =
            serde_json::from_str(body).expect("installing status must decode");
        assert_eq!(status.operation, "installing");
        let prog = status.progress.expect("progress present while installing");
        assert_eq!(prog.percentage, 40);
        assert_eq!(
            classify_install_status(&status.operation, &status.last_error, false),
            InstallPoll::Installing
        );
    }

    #[test]
    fn malformed_progress_never_blocks_operation() {
        // Defense-in-depth: a display-only progress field of an unexpected shape must
        // degrade to None, never failing the decode of the whole status — otherwise
        // `operation` (which drives the state machine) becomes unreadable.
        let body =
            r#"{"operation":"installing","progress":{"percent":99.5},"last_error":"","slots":[]}"#;
        let status: crate::client::UpdateStatus =
            serde_json::from_str(body).expect("a mismatched progress shape must not fail decode");
        assert_eq!(status.operation, "installing");
        assert!(status.progress.is_none());
    }
}
