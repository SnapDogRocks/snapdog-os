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

#[derive(Debug, Clone, Eq, PartialEq)]
enum RebootExpectation {
    Rauc {
        previous_version: String,
        previous_booted_slot: Option<String>,
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
            if status.operation == "installing" {
                if let Some(prog) = status.progress {
                    ui_poll.update_message(format!(
                        "Installing: {:.1}% ({})",
                        prog.percent, prog.message
                    ));
                }
            } else if status.operation == "idle" {
                if !status.last_error.is_empty() {
                    ui_poll.finish_failure("Installation failed!");
                    return Err(UpgradeError::Failed(status.last_error));
                }
                // The install is done but the device does not reboot on its own —
                // trigger it so it boots into the freshly-installed slot. The
                // connection drops as it goes down, so ignore the transport result.
                let _ = self.client.reboot().await;
                ui_poll.finish_success("Installation complete! System is rebooting...");
                break;
            } else {
                ui_poll.update_message(format!("Operation status: {}", status.operation));
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

    async fn booted_slot(&self, deadline: Instant) -> Result<Option<String>> {
        let status = run_with_deadline(
            deadline,
            "fetch RAUC slot status",
            self.client.update_status(),
        )
        .await?;
        Ok(booted_slot_name(&status.slots))
    }

    async fn wait_for_reboot(
        &self,
        expectation: &RebootExpectation,
        deadline: Instant,
    ) -> Result<()> {
        let ui = self
            .reporter
            .spinner("reboot", "Waiting for device to go offline...");

        while remaining_duration(deadline).is_some() {
            if sleep_until_deadline(deadline, Duration::from_secs(2), "device reboot")
                .await
                .is_err()
            {
                break;
            }
            if self.client.system_info().await.is_err() {
                ui.update_message("Device went offline. Waiting for boot...".to_string());
                break;
            }
        }

        let mut backoff = Duration::from_secs(2);
        while remaining_duration(deadline).is_some() {
            if sleep_until_deadline(deadline, backoff, "device reboot")
                .await
                .is_err()
            {
                break;
            }
            if let Ok(info) = self.client.system_info().await {
                let current_booted_slot = if expectation.needs_booted_slot() {
                    run_with_deadline(
                        deadline,
                        "fetch RAUC slot status",
                        self.client.update_status(),
                    )
                    .await
                    .ok()
                    .and_then(|status| booted_slot_name(&status.slots))
                } else {
                    None
                };

                if let Some(reason) =
                    expectation.success_reason(&info, current_booted_slot.as_deref())
                {
                    ui.finish_success(&format!(
                        "Success! Device is back online ({reason}; uptime: {}s, version: v{})",
                        info.uptime_seconds, info.version
                    ));
                    return Ok(());
                }
                ui.update_message(
                    expectation.pending_message(&info, current_booted_slot.as_deref()),
                );
            }
            backoff = (backoff * 2).min(Duration::from_secs(10));
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
        current_booted_slot: Option<&str>,
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
                    (previous_booted_slot.as_deref(), current_booted_slot)
                {
                    if previous != current {
                        return Some(format!("booted slot changed from {previous} to {current}"));
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
                } else if info.uptime_seconds < 180 {
                    Some("device rebooted".to_string())
                } else {
                    None
                }
            }
        }
    }

    fn pending_message(&self, info: &SystemInfo, current_booted_slot: Option<&str>) -> String {
        match self {
            Self::Rauc {
                previous_version,
                previous_booted_slot,
            } => format!(
                "Device is online, but update is not verified yet (version: v{}, previous: v{}, booted slot: {}, previous slot: {}). Waiting...",
                info.version,
                previous_version,
                current_booted_slot.unwrap_or("unknown"),
                previous_booted_slot.as_deref().unwrap_or("unknown")
            ),
            Self::RawFlash { previous_version } => format!(
                "Device is online, but reboot is not verified yet (version: v{}, previous: v{}, uptime: {}s). Waiting...",
                info.version, previous_version, info.uptime_seconds
            ),
        }
    }
}

fn booted_slot_name(slots: &[SlotStatus]) -> Option<String> {
    slots
        .iter()
        .find(|slot| slot.booted)
        .map(|slot| slot.name.clone())
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
            booted_slot_name(&[slot("rootfs.0", false), slot("rootfs.1", true)]),
            Some("rootfs.1".to_string())
        );
    }

    #[test]
    fn rauc_reboot_success_accepts_version_change() {
        let expectation = RebootExpectation::Rauc {
            previous_version: "1.0.0".to_string(),
            previous_booted_slot: Some("rootfs.0".to_string()),
        };

        let reason = expectation.success_reason(&system_info("1.1.0", 42), Some("rootfs.0"));
        assert!(reason.is_some_and(|r| r.contains("version changed")));
    }

    #[test]
    fn rauc_reboot_success_accepts_slot_change_with_same_version() {
        let expectation = RebootExpectation::Rauc {
            previous_version: "1.0.0".to_string(),
            previous_booted_slot: Some("rootfs.0".to_string()),
        };

        let reason = expectation.success_reason(&system_info("1.0.0", 42), Some("rootfs.1"));
        assert!(reason.is_some_and(|r| r.contains("booted slot changed")));
    }

    #[test]
    fn rauc_reboot_does_not_accept_recent_uptime_alone() {
        let expectation = RebootExpectation::Rauc {
            previous_version: "1.0.0".to_string(),
            previous_booted_slot: Some("rootfs.0".to_string()),
        };

        assert_eq!(
            expectation.success_reason(&system_info("1.0.0", 42), Some("rootfs.0")),
            None
        );
    }

    #[test]
    fn raw_flash_reboot_accepts_recent_uptime() {
        let expectation = RebootExpectation::RawFlash {
            previous_version: "1.0.0".to_string(),
        };

        let reason = expectation.success_reason(&system_info("1.0.0", 42), None);
        assert!(reason.is_some_and(|r| r == "device rebooted"));
    }
}
