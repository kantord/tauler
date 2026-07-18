//! Pure, deterministic scheduling logic for deciding when the bar module
//! should next refresh, given a heartbeat ceiling and a debounced pending
//! change. No I/O, no real clock reads — callers supply `now`.

use std::time::{Duration, Instant};

const HEARTBEAT: Duration = Duration::from_secs(1);
const DEBOUNCE: Duration = Duration::from_millis(50);

/// What the scheduler decided to do given the current time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeDecision {
    /// Refresh immediately — a deadline has been reached or passed.
    RefreshNow,
    /// Sleep until this instant, then re-evaluate.
    WaitUntil(Instant),
}

/// Decide the next wakeup given:
/// - `now`: the current instant
/// - `last_refresh`: when the last refresh happened (heartbeat ceiling is
///   `last_refresh + 1s`)
/// - `pending_since`: `Some(t)` if a change arrived at `t` and is still
///   debouncing (debounce deadline is `t + 50ms`), `None` if there is no
///   pending change
pub fn next_wakeup(
    now: Instant,
    last_refresh: Instant,
    pending_since: Option<Instant>,
) -> WakeDecision {
    let heartbeat_ceiling = last_refresh + HEARTBEAT;

    let deadline = match pending_since {
        Some(t) => (t + DEBOUNCE).min(heartbeat_ceiling),
        None => heartbeat_ceiling,
    };

    if now >= deadline {
        WakeDecision::RefreshNow
    } else {
        WakeDecision::WaitUntil(deadline)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs `next_wakeup` with `last_refresh` fixed at a base instant, `now`
    /// and `pending_since` given as millisecond offsets from that base, and
    /// asserts the resulting decision. `expect` is a function from the base
    /// instant to the expected `WakeDecision`, since `WaitUntil` deadlines
    /// are themselves offsets from `base`.
    fn check(
        pending_since_ms: Option<u64>,
        now_ms: u64,
        expect: impl FnOnce(Instant) -> WakeDecision,
    ) {
        let base = Instant::now();
        let last_refresh = base;
        let pending_since = pending_since_ms.map(|ms| base + Duration::from_millis(ms));
        let now = base + Duration::from_millis(now_ms);

        let decision = next_wakeup(now, last_refresh, pending_since);

        assert_eq!(decision, expect(base));
    }

    #[test]
    fn no_pending_change_waits_until_heartbeat_ceiling() {
        check(None, 0, |base| WakeDecision::WaitUntil(base + HEARTBEAT));
    }

    #[test]
    fn no_pending_change_refreshes_now_once_heartbeat_ceiling_reached() {
        // "Reached" is interpreted inclusively: now == deadline counts as
        // having reached it, so we expect RefreshNow, not WaitUntil(now).
        check(None, 1000, |_| WakeDecision::RefreshNow);
    }

    #[test]
    fn no_pending_change_refreshes_now_once_heartbeat_ceiling_passed() {
        check(None, 1500, |_| WakeDecision::RefreshNow);
    }

    #[test]
    fn pending_change_debounce_deadline_before_heartbeat_waits_for_debounce() {
        // Change arrives almost immediately after refresh: debounce deadline
        // (base + 60ms) is well before the heartbeat ceiling (base + 1s).
        check(Some(10), 0, |base| {
            WakeDecision::WaitUntil(base + Duration::from_millis(10) + DEBOUNCE)
        });
    }

    #[test]
    fn pending_change_refreshes_now_once_debounce_deadline_reached() {
        // now == debounce deadline (base + 10ms + 50ms = base + 60ms), which
        // is still well before the heartbeat ceiling.
        check(Some(10), 60, |_| WakeDecision::RefreshNow);
    }

    #[test]
    fn pending_change_debounce_deadline_after_heartbeat_is_capped_by_heartbeat() {
        // Change arrives just after a refresh, so its 50ms debounce deadline
        // (base + 970ms + 50ms = base + 1020ms) would land after the
        // heartbeat ceiling (base + 1000ms). The heartbeat ceiling must win.
        check(Some(970), 0, |base| {
            WakeDecision::WaitUntil(base + HEARTBEAT)
        });
    }

    #[test]
    fn pending_change_refreshes_now_once_heartbeat_ceiling_reached_while_still_debouncing() {
        // now reaches the heartbeat ceiling before the (later) debounce
        // deadline would fire.
        check(Some(970), 1000, |_| WakeDecision::RefreshNow);
    }

    #[test]
    fn debounce_deadline_exactly_equal_to_heartbeat_ceiling_is_capped_by_heartbeat() {
        // Debounce deadline lands exactly on the heartbeat ceiling:
        // base + 950ms + 50ms == base + 1000ms. We treat "capped at" as
        // inclusive equality still resolving to the heartbeat ceiling value
        // (the two coincide, so either interpretation yields the same
        // instant here, but we assert the heartbeat-ceiling framing).
        check(Some(950), 0, |base| {
            WakeDecision::WaitUntil(base + HEARTBEAT)
        });
    }
}
