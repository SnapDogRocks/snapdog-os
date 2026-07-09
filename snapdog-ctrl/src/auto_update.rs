//! Auto-update scheduler.
//!
//! Checks daily at the configured time whether a newer bundle is available,
//! then installs it via RAUC and reboots.

use crate::system::{
    UpdateDecision, current_os_version, decide_update, get_auto_update, last_failed_update,
    rauc_install, rauc_operation, reboot, record_pending_update, remote_channel_version,
};

const UPDATE_BASE_URL: &str = "https://updates.snapdog.cc/os/bundles";
const SECS_PER_DAY: u64 = 24 * 3600;
const SECS_PER_WEEK: u64 = 7 * SECS_PER_DAY;
const SECS_PER_MONTH: u64 = 30 * SECS_PER_DAY;

/// Spawn the auto-update background loop.
pub fn spawn() {
    tokio::spawn(async {
        loop {
            if let Err(e) = run_cycle().await {
                tracing::warn!("auto-update cycle error: {e}");
            }
            // Sleep based on interval before next check
            let config = get_auto_update().await;
            let sleep_secs = match config.interval.as_str() {
                "weekly" => SECS_PER_WEEK,
                "monthly" => SECS_PER_MONTH,
                _ => SECS_PER_DAY,
            };
            tokio::time::sleep(std::time::Duration::from_secs(sleep_secs)).await;
        }
    });
}

async fn run_cycle() -> anyhow::Result<()> {
    let config = get_auto_update().await;
    if !config.enabled {
        return Ok(());
    }

    // Wait until configured time
    wait_until(&config.time).await;

    // Re-read config (user might have disabled in the meantime)
    let config = get_auto_update().await;
    if !config.enabled {
        return Ok(());
    }

    // Don't install if already installing
    if rauc_operation().await.unwrap_or_default() != "idle" {
        tracing::info!("auto-update: RAUC busy, skipping");
        return Ok(());
    }

    // Decide whether to install. Only apply a strictly newer bundle, and never
    // retry a version that already failed to boot. Without this gate an unbootable
    // bundle would install → roll back → reinstall on every cycle, rewriting the
    // eMMC/SD indefinitely; and even a healthy channel would be needlessly
    // reflashed every day because the pointer already matches the running version.
    let current = current_os_version().await;
    let remote = remote_channel_version(&config.channel).await;
    let last_failed = last_failed_update().await;
    let version = match decide_update(remote.as_deref(), &current, last_failed.as_deref()) {
        UpdateDecision::Install(version) => version,
        UpdateDecision::Skip(reason) => {
            tracing::info!(
                "auto-update: skipping (running {current}, {} channel offers {}): {reason}",
                config.channel,
                remote.as_deref().unwrap_or("unknown")
            );
            return Ok(());
        }
    };

    // Construct bundle URL: snapdog-os-<board>-<channel>.raucb (channel is
    // "release" or "beta", matching the CI/CDN naming).
    let board = crate::system::detect_board().await;
    let url = format!("{UPDATE_BASE_URL}/{board}-{}.raucb", config.channel);

    tracing::info!("auto-update: installing {version} from {url}");
    rauc_install(&url).await?;

    // Wait for RAUC to finish (max 30 minutes), then reboot
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1800);
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        if rauc_operation().await.unwrap_or_default() == "idle" {
            break;
        }
        if std::time::Instant::now() > deadline {
            tracing::error!("auto-update: RAUC stuck, aborting");
            return Ok(());
        }
    }

    // Record the version we are about to boot into so the next boot can confirm it
    // took — or mark it bad if the bootloader rolls back to the previous slot.
    record_pending_update(&version).await;

    tracing::info!("auto-update: install complete, rebooting");
    // Tryboot-aware reboot (as the manual / DAC-detect paths use): enters the trial
    // just armed by the install via RESTART2. A plain `systemctl reboot` would boot
    // the committed slot instead, so the install would never run and reconcile would
    // mark it failed on the next boot.
    reboot().await;

    Ok(())
}

async fn wait_until(time: &str) {
    let (target_h, target_m) = parse_time(time);

    loop {
        let now = chrono_now();
        let (h, m) = (now / 60 % 24, now % 60);

        if h == target_h && m == target_m {
            break;
        }

        // Sleep 30s and check again
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
    }
}

fn parse_time(s: &str) -> (u64, u64) {
    let parts: Vec<&str> = s.split(':').collect();
    let h = parts.first().and_then(|v| v.parse().ok()).unwrap_or(4);
    let m = parts.get(1).and_then(|v| v.parse().ok()).unwrap_or(0);
    (h, m)
}

/// Minutes since midnight (UTC) from system clock.
fn chrono_now() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    (secs / 60) % (24 * 60)
}
