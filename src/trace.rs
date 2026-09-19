//! The Trace: the recorded timings of one input's journey to pixels, one entry per Hop
//! (see CONTEXT.md and ADR 0042).
//!
//! A report is one line of space-separated `key=value` pairs: one `hop=<ms>ms` per mark
//! (its duration since the previous mark, or since the origin for the first), then
//! `presented=<ms>ms` (since the last mark), `total=<ms>ms` (origin to presented) and
//! `superseded=<n>`. Durations are milliseconds with one decimal.
//!
//! On X11 the origin is the event's server timestamp: Xorg stamps input events from
//! CLOCK_MONOTONIC in milliseconds, the clock `Instant` uses on Linux, so `socket=` is
//! the time the event sat between the X server and the presenter. A server clock that
//! disagrees with ours by more than ten seconds is not ours (a remote X server, XWayland
//! with a different base) and the origin falls back to receipt. Wayland pointer
//! timestamps have an unspecified base, so Wayland and macOS keep the receipt origin.

use std::time::{Duration, Instant};

/// How far behind our clock a server timestamp may read and still be trusted as ours.
const SAME_CLOCK_TOLERANCE_MS: u32 = 10_000;

#[derive(Debug, Clone)]
pub struct Trace {
    origin: Instant,
    marks: Vec<(&'static str, Instant)>,
    superseded: u32,
}

impl Trace {
    /// A Trace whose origin is `origin` — the instant the input was received.
    pub fn started_at(origin: Instant) -> Trace {
        Trace {
            origin,
            marks: Vec::new(),
            superseded: 0,
        }
    }

    /// A Trace whose origin is now — but only while someone is listening for the
    /// `tauler::trace` target at DEBUG. Otherwise `None`, which is what keeps the
    /// Trace off by default: every Hop clones and marks an `Option` that is empty.
    pub fn begin() -> Option<Trace> {
        if tracing::enabled!(target: "tauler::trace", tracing::Level::DEBUG) {
            Some(Trace::started_at(Instant::now()))
        } else {
            None
        }
    }

    /// A Trace whose origin is the X server's `event_ms` timestamp and whose first mark,
    /// `socket`, is now — the event's receipt. Gated like [`Trace::begin`].
    #[cfg(target_os = "linux")]
    pub fn begin_at_monotonic_ms(event_ms: u32) -> Option<Trace> {
        if !tracing::enabled!(target: "tauler::trace", tracing::Level::DEBUG) {
            return None;
        }
        // Truncated the way Xorg's GetTimeInMillis does: tv_sec*1000 + tv_nsec/1000000 as CARD32.
        let ts = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
        let now_ms = (ts.tv_sec * 1000 + ts.tv_nsec / 1_000_000) as u32;
        let now = Instant::now();
        let mut trace = Trace::started_at(Trace::origin_from_monotonic_ms(event_ms, now_ms, now));
        trace.mark("socket", now);
        Some(trace)
    }

    /// The instant `event_ms` names, given that the same 32-bit monotonic millisecond
    /// clock read `now_ms` at `now`. Wrap-safe: the clock wrapping between the event
    /// and now is handled. An `event_ms` more than [`SAME_CLOCK_TOLERANCE_MS`] behind
    /// `now_ms` is not on our clock (a remote X server, XWayland with a different base),
    /// and the origin falls back to `now` rather than inventing latency.
    pub fn origin_from_monotonic_ms(event_ms: u32, now_ms: u32, now: Instant) -> Instant {
        let elapsed_ms = now_ms.wrapping_sub(event_ms);
        if elapsed_ms > SAME_CLOCK_TOLERANCE_MS {
            return now;
        }
        now - Duration::from_millis(u64::from(elapsed_ms))
    }

    /// Record that the journey reached `hop` at `at`.
    pub fn mark(&mut self, hop: &'static str, at: Instant) {
        self.marks.push((hop, at));
    }

    /// Record that the journey reached `hop` now.
    pub fn mark_now(&mut self, hop: &'static str) {
        self.mark(hop, Instant::now());
    }

    /// Add `n` superseded render requests to the count this Trace reports.
    pub fn superseded(&mut self, n: u32) {
        self.superseded += n;
    }

    /// This Trace's request replaced `older` in a slot: count it and everything it had
    /// already superseded.
    pub fn supersedes(&mut self, older: &Trace) {
        self.superseded += older.superseded + 1;
    }

    /// The one-line report: each Hop's duration since the previous mark, then the total
    /// from origin to `presented`, then the supersede count.
    pub fn report(&self, presented: Instant) -> String {
        let mut parts = Vec::with_capacity(self.marks.len() + 3);
        let mut previous = self.origin;
        for &(hop, at) in &self.marks {
            parts.push(format!("{hop}={}ms", millis(at - previous)));
            previous = at;
        }
        parts.push(format!("presented={}ms", millis(presented - previous)));
        parts.push(format!("total={}ms", millis(presented - self.origin)));
        parts.push(format!("superseded={}", self.superseded));
        parts.join(" ")
    }
}

fn millis(d: Duration) -> String {
    format!("{:.1}", d.as_secs_f64() * 1000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_report_names_each_hop_with_its_duration() {
        let t0 = Instant::now();
        let mut trace = Trace::started_at(t0);
        trace.mark("pass", t0 + Duration::from_millis(2));
        trace.mark("intent", t0 + Duration::from_millis(3));
        trace.superseded(1);

        assert_eq!(
            trace.report(t0 + Duration::from_millis(10)),
            "pass=2.0ms intent=1.0ms presented=7.0ms total=10.0ms superseded=1"
        );
    }

    #[test]
    fn an_origin_is_derived_from_the_server_clock_across_the_wrap() {
        let now = Instant::now();

        let origin = Trace::origin_from_monotonic_ms(995, 1000, now);
        assert_eq!(
            Trace::started_at(origin).report(now),
            "presented=5.0ms total=5.0ms superseded=0",
            "an event stamped 5 ms before the clock read now_ms happened 5 ms before now"
        );

        let origin = Trace::origin_from_monotonic_ms(u32::MAX - 2, 2, now);
        assert_eq!(
            Trace::started_at(origin).report(now),
            "presented=5.0ms total=5.0ms superseded=0",
            "the clock wrapping 3 ms after the event and reading 2 now is still 5 ms elapsed, not 49.7 days backwards"
        );
    }

    #[test]
    fn a_server_clock_that_is_not_ours_yields_no_origin_before_now() {
        let now = Instant::now();

        let origin = Trace::origin_from_monotonic_ms(0, 60_000, now);
        assert_eq!(
            Trace::started_at(origin).report(now),
            "presented=0.0ms total=0.0ms superseded=0",
            "a server timestamp more than ten seconds behind our monotonic clock is not on our clock (a remote X server, XWayland with a different base), so the origin falls back to now rather than inventing a minute of latency"
        );
    }
}
