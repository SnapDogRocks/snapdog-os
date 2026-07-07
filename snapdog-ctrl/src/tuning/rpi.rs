// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

use super::TuningConfig;
use crate::config_txt;
use anyhow::{Context, Result};

const CMDLINE_PATH: &str = "/boot/cmdline.txt";
const CMDLINE_BACKUP_PATH: &str = "/boot/cmdline.txt.bak";
const SYSTEMD_OVERRIDE_DIR: &str = "/etc/systemd/system/snapdog-client.service.d";
const SYSTEMD_OVERRIDE_PATH: &str = "/etc/systemd/system/snapdog-client.service.d/affinity.conf";

pub struct RpiTuningDriver;

impl RpiTuningDriver {
    pub const fn new() -> Self {
        Self
    }

    async fn read_cmdline(&self) -> Result<String> {
        tokio::fs::read_to_string(CMDLINE_PATH)
            .await
            .context("failed to read /boot/cmdline.txt")
    }

    async fn write_cmdline(&self, content: &str) -> Result<()> {
        #[cfg(target_os = "linux")]
        {
            let _ = tokio::process::Command::new("mount")
                .args(["-o", "remount,rw", "/boot"])
                .output()
                .await;
        }

        let write_res = async {
            if tokio::fs::metadata(CMDLINE_PATH).await.is_ok() {
                tokio::fs::copy(CMDLINE_PATH, CMDLINE_BACKUP_PATH).await?;
            }
            tokio::fs::write(CMDLINE_PATH, content).await?;
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

    pub async fn get_config(&self) -> Result<TuningConfig> {
        let config_txt_content = config_txt::read().await.unwrap_or_default();
        let cmdline_content = self.read_cmdline().await.unwrap_or_default();

        let rf_kill_wifi = config_txt::has_dtoverlay(&config_txt_content, "disable-wifi");
        let rf_kill_bluetooth = config_txt::has_dtoverlay(&config_txt_content, "disable-bt");

        let disable_onboard_audio = config_txt::find_value(&config_txt_content, "dtparam=audio")
            .is_some_and(|val| val == "off");

        let exclusive_audio_core = cmdline_content
            .split_whitespace()
            .any(|arg| arg == "isolcpus=3");

        Ok(TuningConfig {
            rf_kill_wifi,
            rf_kill_bluetooth,
            disable_onboard_audio,
            exclusive_audio_core,
        })
    }

    pub async fn set_config(&self, config: &TuningConfig) -> Result<()> {
        // 1. Update config.txt
        let mut config_txt_content = config_txt::read().await.unwrap_or_default();

        if config.rf_kill_wifi {
            config_txt_content = config_txt::add_dtoverlay(&config_txt_content, "disable-wifi");
        } else {
            config_txt_content = config_txt::remove_dtoverlay(&config_txt_content, "disable-wifi");
        }

        if config.rf_kill_bluetooth {
            config_txt_content = config_txt::add_dtoverlay(&config_txt_content, "disable-bt");
        } else {
            config_txt_content = config_txt::remove_dtoverlay(&config_txt_content, "disable-bt");
        }

        if config.disable_onboard_audio {
            config_txt_content =
                config_txt::upsert_value(&config_txt_content, "dtparam=audio", "off");
        } else {
            config_txt_content =
                config_txt::upsert_value(&config_txt_content, "dtparam=audio", "on");
        }

        if config.exclusive_audio_core {
            config_txt_content = config_txt::upsert_value(&config_txt_content, "force_turbo", "1");
        } else {
            config_txt_content = config_txt::upsert_value(&config_txt_content, "force_turbo", "0");
        }

        config_txt::write(&config_txt_content).await?;

        // 2. Update cmdline.txt
        let mut cmdline_content = self.read_cmdline().await.unwrap_or_default();
        if config.exclusive_audio_core {
            cmdline_content = add_cmdline_arg(&cmdline_content, "isolcpus=3");
        } else {
            cmdline_content = remove_cmdline_arg(&cmdline_content, "isolcpus=3");
        }
        self.write_cmdline(&cmdline_content).await?;

        // 3. Update the systemd CPUAffinity drop-in. SYSTEMD_OVERRIDE_DIR is a
        // symlink to /data (writable) seeded by post-build.sh + snapdog-data-init:
        // the rootfs is read-only, so writing under /etc directly would EROFS. Via
        // /data it persists across reboot AND survives an OS update (which replaces
        // the rootfs slot).
        if config.exclusive_audio_core {
            tokio::fs::create_dir_all(SYSTEMD_OVERRIDE_DIR).await?;
            let affinity_override = "[Service]\n\
                                     CPUAffinity=3\n\
                                     CPUSchedulingPolicy=rr\n\
                                     CPUSchedulingPriority=99\n\
                                     LimitRTPRIO=99\n\
                                     LimitMEMLOCK=infinity\n";
            tokio::fs::write(SYSTEMD_OVERRIDE_PATH, affinity_override).await?;
        } else if tokio::fs::metadata(SYSTEMD_OVERRIDE_PATH).await.is_ok() {
            tokio::fs::remove_file(SYSTEMD_OVERRIDE_PATH).await?;
        }
        // Re-read unit drop-ins so the affinity takes effect on the next (re)start.
        let _ = tokio::process::Command::new("systemctl")
            .arg("daemon-reload")
            .status()
            .await;

        Ok(())
    }
}

fn add_cmdline_arg(content: &str, arg: &str) -> String {
    let trimmed = content.trim();
    if trimmed.split_whitespace().any(|x| x == arg) {
        return content.to_string();
    }
    if trimmed.is_empty() {
        format!("{arg}\n")
    } else {
        format!("{trimmed} {arg}\n")
    }
}

fn remove_cmdline_arg(content: &str, arg: &str) -> String {
    let trimmed = content.trim();
    let parts: Vec<&str> = trimmed.split_whitespace().filter(|&x| x != arg).collect();
    if parts.is_empty() {
        String::new()
    } else {
        parts.join(" ") + "\n"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_cmdline_arg() {
        let init = "console=tty1 root=/dev/mmcblk0p2";
        assert_eq!(
            add_cmdline_arg(init, "isolcpus=3"),
            "console=tty1 root=/dev/mmcblk0p2 isolcpus=3\n"
        );
        // Already exists
        let init_with = "console=tty1 isolcpus=3 root=/dev/mmcblk0p2";
        assert_eq!(
            add_cmdline_arg(init_with, "isolcpus=3"),
            "console=tty1 isolcpus=3 root=/dev/mmcblk0p2"
        );
    }

    #[test]
    fn test_remove_cmdline_arg() {
        let init = "console=tty1 isolcpus=3 root=/dev/mmcblk0p2\n";
        assert_eq!(
            remove_cmdline_arg(init, "isolcpus=3"),
            "console=tty1 root=/dev/mmcblk0p2\n"
        );
        // Doesn't exist
        let init_without = "console=tty1 root=/dev/mmcblk0p2";
        assert_eq!(
            remove_cmdline_arg(init_without, "isolcpus=3"),
            "console=tty1 root=/dev/mmcblk0p2\n"
        );
    }
}
