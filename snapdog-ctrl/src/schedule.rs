//! Pure scheduling logic for the auto-updater.
//!
//! Kept in its own always-compiled module (the `auto_update` module itself is
//! release-only, `#[cfg(not(debug_assertions))]`, so its logic would otherwise never
//! be unit-tested in CI, which runs the debug test profile). These functions are pure
//! — no clock, no I/O — so they are deterministically testable.

use chrono::NaiveDate;

/// Interpretation of RAUC's asynchronous operation state after `InstallBundle`.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum InstallPoll {
    Waiting,
    Installing,
    Completed,
    Failed,
}

/// `InstallBundle` returns before RAUC necessarily changes `operation` from idle.
/// Therefore idle is completion only after installing has been observed. A reported
/// error always wins, including when RAUC has already returned to idle.
#[must_use]
pub fn classify_install_status(
    operation: &str,
    last_error: &str,
    saw_installing: bool,
) -> InstallPoll {
    if !last_error.trim().is_empty() {
        return InstallPoll::Failed;
    }
    match operation {
        "installing" => InstallPoll::Installing,
        "idle" if saw_installing => InstallPoll::Completed,
        _ => InstallPoll::Waiting,
    }
}

/// Parse an `HH:MM` string into `(hour, minute)`, clamping out-of-range / malformed
/// input to the 04:00 default rather than a nonsense time.
pub fn parse_time(s: &str) -> (u32, u32) {
    let mut it = s.split(':');
    let h = it
        .next()
        .and_then(|v| v.trim().parse().ok())
        .filter(|h| *h < 24)
        .unwrap_or(4);
    let m = it
        .next()
        .and_then(|v| v.trim().parse().ok())
        .filter(|m| *m < 60)
        .unwrap_or(0);
    (h, m)
}

/// True if a full configured interval has elapsed since `last_run` (or it never ran).
/// `daily` → 1 day, `weekly` → 7, `monthly` → 30; anything else is treated as daily.
pub fn interval_elapsed(interval: &str, last_run: Option<NaiveDate>, today: NaiveDate) -> bool {
    let min_days = match interval {
        "weekly" => 7,
        "monthly" => 30,
        _ => 1, // daily
    };
    last_run.is_none_or(|lr| (today - lr).num_days() >= min_days)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_time_valid_and_defaults() {
        assert_eq!(parse_time("04:30"), (4, 30));
        assert_eq!(parse_time("23:59"), (23, 59));
        assert_eq!(parse_time("00:00"), (0, 0));
        assert_eq!(parse_time(" 5:07 "), (5, 7));
        // malformed / out of range → 04:00 default
        assert_eq!(parse_time(""), (4, 0));
        assert_eq!(parse_time("24:00"), (4, 0));
        assert_eq!(parse_time("9:61"), (9, 0));
        assert_eq!(parse_time("nonsense"), (4, 0));
    }

    #[test]
    fn interval_gate() {
        let today = NaiveDate::from_ymd_opt(2026, 7, 11).unwrap();
        let days = chrono::Duration::days;
        // never run → always due
        assert!(interval_elapsed("daily", None, today));
        // ran today → not due for any interval
        assert!(!interval_elapsed("daily", Some(today), today));
        assert!(!interval_elapsed("weekly", Some(today), today));
        assert!(!interval_elapsed("monthly", Some(today), today));
        // daily: yesterday is enough
        assert!(interval_elapsed("daily", Some(today - days(1)), today));
        // weekly: 6 days no, 7 days yes
        assert!(!interval_elapsed("weekly", Some(today - days(6)), today));
        assert!(interval_elapsed("weekly", Some(today - days(7)), today));
        // monthly: 29 no, 30 yes
        assert!(!interval_elapsed("monthly", Some(today - days(29)), today));
        assert!(interval_elapsed("monthly", Some(today - days(30)), today));
        // unknown interval falls back to daily
        assert!(interval_elapsed("hourly", Some(today - days(1)), today));
    }

    #[test]
    fn asynchronous_rauc_install_requires_a_real_install_transition() {
        assert_eq!(
            classify_install_status("idle", "", false),
            InstallPoll::Waiting
        );
        assert_eq!(
            classify_install_status("installing", "", false),
            InstallPoll::Installing
        );
        assert_eq!(
            classify_install_status("idle", "", true),
            InstallPoll::Completed
        );
        assert_eq!(
            classify_install_status("idle", "signature rejected", true),
            InstallPoll::Failed
        );
        assert_eq!(
            classify_install_status("installing", "write failed", true),
            InstallPoll::Failed
        );
        assert_eq!(
            classify_install_status("unknown", "", true),
            InstallPoll::Waiting
        );
    }
}
