//! The Trace: the recorded timings of one input's journey to pixels, one entry per Hop
//! (see CONTEXT.md and ADR 0042).
//!
//! A report is one line of space-separated `key=value` pairs: one `hop=<ms>ms` per mark
//! (its duration since the previous mark, or since the origin for the first), then
//! `presented=<ms>ms` (since the last mark), `total=<ms>ms` (origin to presented) and
//! `superseded=<n>`. Durations are milliseconds with one decimal.

use std::time::{Duration, Instant};

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
}
