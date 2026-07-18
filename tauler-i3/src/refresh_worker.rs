//! Refresh-worker thread: owns one dedicated GET_TREE connection and runs
//! the debounce-with-max-wait scheduler (see `scheduler::next_wakeup`).
//! Becomes the sole stdout writer for workspace updates — the very first
//! scheduling deadline is essentially "now", so there's no separate
//! one-time startup emit.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::ipc::I3Query;
use crate::scheduler::{WakeDecision, next_wakeup};
use crate::tree_cache::TreeCache;
use crate::workspace::{Workspace, build_workspace_data, fetch_workspaces};

/// Run the refresh-worker loop: fetch and publish the workspace tree on the
/// schedule `next_wakeup` decides, until `refresh_rx`'s senders are all
/// dropped.
pub fn run(
    mut query: I3Query,
    output: String,
    refresh_rx: mpsc::Receiver<()>,
    cache: TreeCache<Vec<Workspace>>,
) {
    // Force an immediate first refresh: a last_refresh exactly one
    // heartbeat in the past makes the heartbeat ceiling equal to "now",
    // which next_wakeup treats as already-reached.
    let mut last_refresh = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .unwrap_or_else(Instant::now);
    let mut pending_since: Option<Instant> = None;

    loop {
        match next_wakeup(Instant::now(), last_refresh, pending_since) {
            WakeDecision::RefreshNow => {
                match fetch_workspaces(&mut query, &output) {
                    Ok(ws) => {
                        println!("{}", build_workspace_data(&ws));
                        cache.publish(ws);
                    }
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            "fetch_workspaces failed, skipping refresh"
                        );
                    }
                }
                // Success or failure both reset the schedule — no
                // extra retry layer beyond I3Query's own.
                last_refresh = Instant::now();
                pending_since = None;
            }
            WakeDecision::WaitUntil(deadline) => {
                let wait = deadline.saturating_duration_since(Instant::now());
                match refresh_rx.recv_timeout(wait) {
                    Ok(()) => {
                        if pending_since.is_none() {
                            pending_since = Some(Instant::now());
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Unique socket path under the system temp dir.
    fn temp_sock(name: &str) -> String {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir()
            .join(format!(
                "tauler-i3-refresh-worker-{name}-{}-{nanos}.sock",
                std::process::id()
            ))
            .to_string_lossy()
            .into_owned()
    }

    /// Regression test for plan.md's Testing section: a failed GET_TREE
    /// attempt must still reset the schedule (`last_refresh`/`pending_since`)
    /// exactly as a successful one would — no extra retry layer beyond
    /// `I3Query`'s own internal reconnect+retry-once. We can't observe
    /// `last_refresh`/`pending_since` directly (they're local to `run`), so
    /// we observe the property that actually matters operationally: with a
    /// server that always fails GET_TREE, the worker's heartbeat-driven
    /// retries stay at roughly the ~1s heartbeat cadence — not a hot loop.
    ///
    /// The fake server accepts and immediately drops every connection
    /// without reading or replying, so each GET_TREE attempt fails fast
    /// (broken pipe / EOF) rather than waiting out the query timeout.
    #[test]
    fn failed_fetch_still_resets_schedule_without_hot_looping() {
        let path = temp_sock("failed-fetch");
        let listener = UnixListener::bind(&path).unwrap();
        let accepts = Arc::new(AtomicUsize::new(0));

        let a = Arc::clone(&accepts);
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(s) = stream else { break };
                a.fetch_add(1, Ordering::SeqCst);
                // Never read or reply: the client sees this as a failed
                // GET_TREE (broken pipe / EOF) almost immediately.
                drop(s);
            }
        });

        // Short, test-friendly timeout (not the 5s production
        // I3_IPC_TIMEOUT) so a genuine hang would still fail fast.
        let query = I3Query::new(path, Duration::from_millis(200));
        let (refresh_tx, refresh_rx) = mpsc::channel::<()>();
        let cache = TreeCache::new(Vec::new());

        thread::spawn(move || {
            run(query, "DP-1".to_string(), refresh_rx, cache);
        });

        // Observe accepts over a window spanning roughly one heartbeat
        // cycle beyond the first (immediate) attempt.
        thread::sleep(Duration::from_millis(1200));
        // Let the worker's next recv_timeout notice there are no senders
        // left and exit its loop instead of running indefinitely.
        drop(refresh_tx);

        let n = accepts.load(Ordering::SeqCst);
        assert!(
            n >= 1,
            "expected at least the initial refresh attempt to reach the server, got {n}"
        );
        assert!(
            n <= 8,
            "expected roughly heartbeat-cadence retries (a couple of \
             accepts per attempt via I3Query's own reconnect+retry) within \
             ~1.2s, got {n} accepts — looks like a hot loop instead of the \
             schedule resetting normally after each failed attempt"
        );
    }
}
