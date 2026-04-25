//! Dev-channel auto-update scheduler.
//!
//! Wakes at the next UTC 00:00 boundary and runs the same update path that
//! `rove update` uses from the CLI. Stable builds are a no-op. Failures are
//! logged and the next nightly window retries; no retries are attempted on
//! failure within the same day.
//!
//! Post-update behavior is to **leave the running daemon in place**. The
//! on-disk binary is swapped via `self_replace`; operators pick up the new
//! bits on the next manual restart. This matches the behavior of the manual
//! `rove update` CLI and avoids interrupting long-running tasks.

use std::time::Duration;

use chrono::{Datelike, TimeZone, Utc};
use tokio::time::sleep;
use tracing::{error, info};

use crate::cli::output::OutputFormat;
use crate::cli::update::handle_update;
use crate::config::channel::Channel;

/// Spawn the dev-channel auto-update loop. No-op on stable channel.
pub fn spawn_auto_update_scheduler() {
    if Channel::current() != Channel::Dev {
        info!(
            channel = Channel::current().as_str(),
            "auto-update scheduler disabled (not on dev channel)"
        );
        return;
    }

    tokio::spawn(async move {
        info!("dev-channel auto-update scheduler starting");
        run_loop().await;
    });
}

async fn run_loop() {
    loop {
        let wait = duration_until_next_utc_midnight(Utc::now());
        info!(
            next_fire_in_secs = wait.as_secs(),
            "sleeping until next UTC 00:00"
        );
        sleep(wait).await;

        info!("dev-channel auto-update: fetching and applying latest build");
        match handle_update(false, OutputFormat::Text).await {
            Ok(()) => info!("dev-channel auto-update succeeded"),
            Err(error) => error!(%error, "dev-channel auto-update failed; will retry next UTC 00:00"),
        }

        // Belt-and-suspenders: sleep at least one second so the loop cannot
        // busy-fire if the clock didn't advance past midnight for any reason.
        sleep(Duration::from_secs(1)).await;
    }
}

/// Duration from `now` until the next UTC 00:00 boundary. Always strictly
/// positive — if we are *at* midnight, returns 24h.
pub(crate) fn duration_until_next_utc_midnight(now: chrono::DateTime<Utc>) -> Duration {
    let next = Utc
        .with_ymd_and_hms(now.year(), now.month(), now.day(), 0, 0, 0)
        .unwrap()
        + chrono::Duration::days(1);

    let delta = next - now;
    let secs = delta.num_seconds().max(1) as u64;
    let nanos = delta
        .subsec_nanos()
        .clamp(0, 999_999_999) as u32;
    Duration::new(secs, nanos)
}

#[cfg(test)]
mod tests {
    use super::{duration_until_next_utc_midnight, spawn_auto_update_scheduler};
    use crate::config::channel::Channel;
    use chrono::{TimeZone, Utc};

    #[test]
    fn duration_is_positive_and_under_24h() {
        let now = Utc.with_ymd_and_hms(2026, 4, 25, 12, 0, 0).unwrap();
        let d = duration_until_next_utc_midnight(now);
        assert!(d.as_secs() > 0);
        assert!(d.as_secs() <= 24 * 60 * 60);
    }

    #[test]
    fn duration_wraps_past_midnight_to_next_day() {
        let just_after_midnight = Utc.with_ymd_and_hms(2026, 4, 25, 0, 0, 1).unwrap();
        let d = duration_until_next_utc_midnight(just_after_midnight);
        // 23h 59m 59s ≈ 86399s
        assert!(d.as_secs() >= 86_399);
        assert!(d.as_secs() <= 86_400);
    }

    #[test]
    fn scheduler_is_noop_on_stable_channel() {
        let previous = std::env::var_os("ROVE_CHANNEL");
        std::env::set_var("ROVE_CHANNEL", "stable");
        assert_eq!(Channel::current(), Channel::Stable);
        // Should not panic and should return immediately (no tokio task
        // actually spawned when the channel check fails).
        spawn_auto_update_scheduler();
        match previous {
            Some(value) => std::env::set_var("ROVE_CHANNEL", value),
            None => std::env::remove_var("ROVE_CHANNEL"),
        }
    }
}
