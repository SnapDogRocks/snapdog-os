//! Auto-update scheduler.
//!
//! Checks daily at the configured time whether a newer bundle is available,
//! then installs it via RAUC and reboots.

use crate::system::{get_auto_update, rauc_install, rauc_operation};

const UPDATE_BASE_URL: &str = "https://update.snapdog.cc/os/bundles";

/// Spawn the auto-update background loop.
pub fn spawn() {
    tokio::spawn(async {
        loop {
            if let Err(e) = run_cycle().await {
                tracing::warn!("auto-update cycle error: {e}");
            }
            // Re-check config every hour
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
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

    // Construct bundle URL
    let board = detect_board();
    let suffix = if config.channel == "stable" {
        ""
    } else {
        "-beta"
    };
    let url = format!("{UPDATE_BASE_URL}/{board}{suffix}.raucb");

    tracing::info!("auto-update: installing from {url}");
    rauc_install(&url).await?;

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

fn detect_board() -> &'static str {
    // Read from /etc/rauc/system.conf compatible field
    let content = std::fs::read_to_string("/etc/rauc/system.conf").unwrap_or_default();
    if content.contains("pi5") {
        "pi5"
    } else if content.contains("pi3") {
        "pi3"
    } else {
        "pi4"
    }
}
