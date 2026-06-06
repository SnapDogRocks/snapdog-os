// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

use crate::client::UpdateClient;
use crate::error::{Result, UpgradeError};
use crate::progress::ProgressUi;
use std::path::Path;
use std::time::Duration;
use tokio::time::sleep;

#[derive(Clone, Copy)]
pub enum ImageType {
    RaucBundle,
    RawFlash,
}

pub struct UpgradeManager {
    client: UpdateClient,
    image_path: std::path::PathBuf,
    image_type: ImageType,
    timeout: Duration,
    poll_interval: Duration,
}

impl UpgradeManager {
    pub fn new(
        base_url: &str,
        image_path: &Path,
        force_raw: bool,
        timeout: Duration,
        poll_interval: Duration,
    ) -> Self {
        let image_type = if force_raw {
            ImageType::RawFlash
        } else {
            let ext = image_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");
            if ext == "raucb" {
                ImageType::RaucBundle
            } else {
                ImageType::RawFlash
            }
        };

        Self {
            client: UpdateClient::new(base_url),
            image_path: image_path.to_path_buf(),
            image_type,
            timeout,
            poll_interval,
        }
    }

    pub async fn run(&mut self, password: Option<&str>) -> Result<()> {
        tracing::info!("Starting preflight checks...");
        self.client.preflight_auth(password).await?;

        // 1. Fetch system details and check target architecture
        let info = self.client.system_info().await?;
        tracing::info!(
            "Connected to '{}' (active version: '{}', board: '{}', uptime: {}s)",
            info.hostname,
            info.version,
            info.board_model,
            info.uptime_seconds
        );

        // Preflight safety checks: prevent installing incompatible image types
        let file_name = self
            .image_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_lowercase();
        let board_compatible = info.board_model.to_lowercase();
        if board_compatible.contains("pi5") && file_name.contains("pi4") {
            return Err(UpgradeError::IncompatibleBoard {
                target: "pi5".to_string(),
                image: "pi4".to_string(),
            });
        } else if board_compatible.contains("pi4") && file_name.contains("pi5") {
            return Err(UpgradeError::IncompatibleBoard {
                target: "pi4".to_string(),
                image: "pi5".to_string(),
            });
        }

        // Check health status before upgrading
        let health = self.client.system_health().await?;
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

        // 2. Perform upload and installation
        match self.image_type {
            ImageType::RaucBundle => self.run_rauc_flow().await?,
            ImageType::RawFlash => self.run_raw_flow().await?,
        }

        // 3. Post-reboot checks
        self.wait_for_reboot(&info.version).await?;
        Ok(())
    }

    async fn run_rauc_flow(&self) -> Result<()> {
        // Upload
        let metadata = tokio::fs::metadata(&self.image_path).await?;
        let ui = std::sync::Arc::new(ProgressUi::new_upload(
            metadata.len(),
            "Uploading RAUC bundle...",
        ));
        let ui_clone = ui.clone();
        self.client
            .upload_image(&self.image_path, "/api/system/update/upload", move |sent| {
                ui_clone.set_position(sent);
            })
            .await?;
        ui.finish_success("Upload completed successfully!");

        // Trigger Install
        tracing::info!("Triggering installation...");
        self.client.trigger_install().await?;

        // Status Polling
        let ui_poll = ProgressUi::new_spinner("Starting installation...");
        let start_time = std::time::Instant::now();

        loop {
            if start_time.elapsed() > self.timeout {
                ui_poll.finish_failure("Installation timed out!");
                return Err(UpgradeError::Timeout(
                    "RAUC installation took too long".into(),
                ));
            }

            sleep(self.poll_interval).await;

            let status = self.client.update_status().await?;
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
                ui_poll.finish_success("Installation complete! System is rebooting...");
                break;
            } else {
                ui_poll.update_message(format!("Operation status: {}", status.operation));
            }
        }
        Ok(())
    }

    async fn run_raw_flow(&self) -> Result<()> {
        let metadata = tokio::fs::metadata(&self.image_path).await?;
        let ui = std::sync::Arc::new(ProgressUi::new_upload(
            metadata.len(),
            "Uploading raw system image...",
        ));
        let ui_clone = ui.clone();
        let challenge = self
            .client
            .trigger_flash_raw(&self.image_path, move |sent| {
                ui_clone.set_position(sent);
            })
            .await?;
        ui.finish_success("Upload completed successfully!");

        println!("\n=======================================================");
        println!("⚠️  WARNING: You are about to flash a raw system image.");
        println!("This will overwrite the inactive root partition completely.");
        println!("Challenge Code: {}", challenge.challenge);
        println!("=======================================================\n");

        tracing::info!("Confirming raw flash challenge...");
        self.client.confirm_flash_raw(&challenge.challenge).await?;
        tracing::info!("Confirmation accepted! System is writing image and rebooting...");

        Ok(())
    }

    async fn wait_for_reboot(&self, old_version: &str) -> Result<()> {
        let ui = ProgressUi::new_spinner("Waiting for device to go offline...");
        let start_time = std::time::Instant::now();
        let max_wait = Duration::from_secs(300);

        // Phase 1: Wait for device to go offline (port closed)
        while start_time.elapsed() < max_wait {
            sleep(Duration::from_secs(2)).await;
            if self.client.system_info().await.is_err() {
                ui.update_message("Device went offline. Waiting for boot...".to_string());
                break;
            }
        }

        // Phase 2: Wait for device to come back online
        let mut backoff = Duration::from_secs(2);
        while start_time.elapsed() < max_wait {
            sleep(backoff).await;
            if let Ok(info) = self.client.system_info().await {
                if info.version != old_version || info.uptime_seconds < 180 {
                    ui.finish_success(&format!(
                        "Success! Device is back online (Uptime: {}s, Version: v{})",
                        info.uptime_seconds, info.version
                    ));
                    return Ok(());
                }
                ui.update_message(
                    "Device is back online, but running old version. Waiting...".to_string(),
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
