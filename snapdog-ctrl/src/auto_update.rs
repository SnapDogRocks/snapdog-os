//! Auto-update scheduler.
//!
//! A lightweight 1-minute tick. Once the system clock is trustworthy, each tick asks:
//! for the configured **local** time-of-day and interval, is an update due and not yet
//! run this interval? If so, install a strictly-newer bundle via RAUC and tryboot-reboot.
//!
//! This replaces an earlier design that blocked on an exact UTC-minute match. The
//! problems it fixes:
//!   * the configured time is now **local** (honours `/etc/localtime` → `/data/localtime`),
//!     not UTC — a user who sets 04:00 gets 04:00 device-local, with correct DST;
//!   * a persisted last-run date gives **catch-up** (a device powered off at the target
//!     time still updates on its next boot) and dedup across process restarts;
//!   * a `>=` window instead of exact-minute equality survives NTP clock steps and
//!     runtime stalls that used to skip the single target minute (→ a 24 h miss);
//!   * config changes (enable / time / channel / interval) are picked up within a minute
//!     instead of after a full day/week/month sleep;
//!   * transient failures (RAUC busy or unreadable, manifest unreachable) retry on the
//!     next tick instead of costing a whole interval.

use crate::schedule::{interval_elapsed, parse_time};
use crate::system::{
    UpdateDecision, bundle_url, current_os_version, decide_update, get_auto_update,
    last_auto_update_date, last_failed_update, rauc_operation, reboot, record_auto_update_date,
    record_pending_update, remote_channel_version, update_auto_update_status,
};
use chrono::{Local, Timelike};

const TICK: std::time::Duration = std::time::Duration::from_secs(60);
/// Epoch of 2025-01-01Z. A Raspberry Pi has no RTC; before the first NTP sync the
/// clock sits at (or near) the epoch, so anything earlier than this is "not synced".
const SANE_EPOCH: i64 = 1_735_689_600;

/// Spawn the auto-update background loop.
pub fn spawn() {
    tokio::spawn(async {
        wait_for_trustworthy_clock().await;
        loop {
            if let Err(e) = tick().await {
                tracing::warn!("auto-update: cycle error: {e}");
            }
            tokio::time::sleep(TICK).await;
        }
    });
}

/// Block until the wall clock looks NTP-synced, so we never compare the configured
/// time against a pre-sync epoch value. Bounded so we never hang forever; the unit is
/// also ordered `After=time-sync.target`, so this is normally already satisfied.
async fn wait_for_trustworthy_clock() {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(300);
    loop {
        if Local::now().timestamp() >= SANE_EPOCH {
            return;
        }
        if std::time::Instant::now() > deadline {
            tracing::warn!("auto-update: clock still unsynced after 5 min; proceeding anyway");
            return;
        }
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
    }
}

async fn tick() -> anyhow::Result<()> {
    let config = get_auto_update().await;
    if !config.enabled {
        return Ok(());
    }

    // A WebUI/local update may still be uploading while RAUC itself is idle.
    // Treat the coordinator as busy too, otherwise the scheduler could consume the
    // interval with a spurious "already in progress" failure.
    if crate::update::is_busy() {
        tracing::info!("auto-update: another firmware operation is active, retrying next tick");
        return Ok(());
    }

    let now = Local::now();
    let today = now.date_naive();
    let (target_h, target_m) = parse_time(&config.time);

    // Time-of-day gate: only once we've reached the configured LOCAL time today.
    if (now.hour(), now.minute()) < (target_h, target_m) {
        return Ok(());
    }

    // Interval + dedup gate: a full interval must have elapsed since the last run.
    // The persisted date makes this survive restarts and gives catch-up when the
    // device was off at the target minute.
    if !interval_elapsed(&config.interval, last_auto_update_date().await, today) {
        return Ok(());
    }

    // RAUC must be idle. Distinguish "genuinely busy" from "status unreadable": on an
    // error we retry next tick rather than conflating it with busy forever.
    match rauc_operation().await {
        Ok(op) if op != "idle" => {
            tracing::info!("auto-update: RAUC busy ({op}), retrying next tick");
            return Ok(());
        }
        Err(e) => {
            tracing::warn!("auto-update: RAUC status unreadable ({e}), retrying next tick");
            return Ok(());
        }
        Ok(_) => {}
    }

    // Fetch the channel's target version. If the manifest is unreachable we do NOT
    // record a run — the next tick retries, so a transient outage at the target time
    // no longer costs a whole interval.
    let current = current_os_version().await;
    update_auto_update_status("checking", true, false, false, None).await;
    let Some(remote) = remote_channel_version(&config.channel).await else {
        tracing::info!("auto-update: update manifest unreachable, retrying next tick");
        update_auto_update_status(
            "error",
            false,
            false,
            false,
            Some("update manifest unreachable"),
        )
        .await;
        return Ok(());
    };

    // Reaching the server and making a decision counts as today's run, so we don't
    // re-check every minute for the rest of the interval. A subsequent install failure
    // then also waits for the next interval rather than retrying immediately — deliberate,
    // so a persistently bad bundle can't hammer the eMMC/SD with a re-download+install
    // every minute. (An unreachable manifest, above, is NOT recorded, so a transient
    // network outage at the target time still retries on the next tick.)
    record_auto_update_date(today).await;

    let last_failed = last_failed_update().await;
    let version = match decide_update(Some(remote.as_str()), &current, last_failed.as_deref()) {
        UpdateDecision::Install(v) => v,
        UpdateDecision::Skip(reason) => {
            tracing::info!(
                "auto-update: skipping (running {current}, {} channel offers {remote}): {reason}",
                config.channel
            );
            update_auto_update_status("up_to_date", false, false, false, None).await;
            return Ok(());
        }
    };

    update_auto_update_status("installing", false, true, false, None).await;
    match install_and_reboot(&version, &config.channel).await {
        Ok(()) => {
            update_auto_update_status("rebooting", false, false, true, None).await;
            Ok(())
        }
        Err(e) => {
            update_auto_update_status("error", false, false, false, Some(&e.to_string())).await;
            Err(e)
        }
    }
}

/// Download, install via RAUC, and tryboot-reboot into `version`. A plain
/// `systemctl reboot` would boot the committed slot instead of the trial slot the
/// install just armed (RESTART2), so reconcile would then mark the bundle failed.
async fn install_and_reboot(version: &str, channel: &str) -> anyhow::Result<()> {
    // Bundle URL: <board>-<channel>.raucb (channel is "release" or "beta").
    let url = bundle_url(channel).await;
    tracing::info!("auto-update: installing {version} from {url}");
    let update_guard = crate::update::install_online(&url).await?;

    // Record the version we are about to boot into so the next boot can confirm it took —
    // or mark it bad if the bootloader rolls back to the previous slot.
    record_pending_update(version).await?;
    tracing::info!("auto-update: install complete, rebooting");
    reboot().await?;
    // `systemctl reboot` can return after systemd accepted the request but before
    // this process is stopped. Keep every firmware entry point locked throughout
    // that window; releasing the guard could admit a second write during shutdown.
    std::mem::forget(update_guard);
    Ok(())
}
