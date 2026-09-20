//! One intent in flight per channel, and the newest of the rest.
//!
//! A Module is a subprocess reading its stdin one line at a time, and nothing
//! about that says how fast it reads. A volume module that forks `wpctl` per
//! intent gets through a few dozen a second; the pipeline can produce one per
//! Pass. Handing it every intent as fast as they are produced means an unbounded
//! queue, and the bar then shows values from however far back the queue reaches
//! — a slider that keeps moving for seconds after the pointer stopped.
//!
//! So a channel takes one intent at a time. While that one is unanswered, newer
//! intents for the same channel replace each other in a slot rather than
//! queueing, exactly as repaints do for a Render target. When the module emits a
//! line, whatever is in the slot goes next.
//!
//! What is deliberately *not* assumed is that a module answers at all. Plenty of
//! them only act — a "play a sound" module has nothing to say back — and waiting
//! forever for an answer that is not coming would take a channel out of service
//! after one intent. So an unanswered intent stops blocking after
//! [`REPLY_GRACE`], and the channel falls back to sending at that rate.
//!
//! Each intent carries its [`Trace`] with it, in flight and in the slot alike, so
//! that the answer a module gives can be attributed to the intent it answers
//! rather than to whatever was held behind it (ADR 0042).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::trace::Trace;

/// How long an unanswered intent holds its channel.
///
/// A module that answers releases its channel far sooner than this, so for the
/// ones this exists for — the silent ones — it is what sets the rate: 25 intents
/// a second, which is more than a person dragging can perceive and far more than
/// a subprocess forking per message can absorb.
const REPLY_GRACE: Duration = Duration::from_millis(40);

/// What an answer on a channel resolves to.
pub struct Answered {
    /// The Trace of the intent that was in flight — the one the answer is for.
    pub trace: Option<Trace>,
    /// The intent that was waiting, now in flight in its turn, with its Trace.
    pub next: Option<(serde_json::Value, Option<Trace>)>,
}

#[derive(Default)]
pub struct Outbox {
    /// When the intent currently occupying each channel was sent, and its Trace.
    in_flight: HashMap<String, (Instant, Option<Trace>)>,
    /// The newest intent waiting for each occupied channel, with its Trace.
    waiting: HashMap<String, (serde_json::Value, Option<Trace>)>,
}

impl Outbox {
    pub fn new() -> Self {
        Self::default()
    }

    /// Offer an intent for `channel`. Returns it, with its Trace, if it should
    /// be written now.
    ///
    /// If the channel is busy the intent is kept instead, replacing whatever was
    /// already waiting — a superseded intent describes a state the newer one has
    /// already moved past.
    pub fn offer(
        &mut self,
        now: Instant,
        channel: &str,
        intent: serde_json::Value,
        trace: Option<Trace>,
    ) -> Option<(serde_json::Value, Option<Trace>)> {
        if self.busy(now, channel) {
            self.waiting.insert(channel.to_string(), (intent, trace));
            return None;
        }
        self.in_flight
            .insert(channel.to_string(), (now, trace.clone()));
        Some((intent, trace))
    }

    /// A module emitted a line, so its channel is free. Returns the Trace of
    /// the intent that was in flight, and whatever was waiting for the channel,
    /// which is then in flight in its turn.
    pub fn answered(&mut self, now: Instant, channel: &str) -> Answered {
        let trace = self.in_flight.remove(channel).and_then(|(_, t)| t);
        let next = self.waiting.remove(channel).map(|(intent, trace)| {
            self.in_flight
                .insert(channel.to_string(), (now, trace.clone()));
            (intent, trace)
        });
        Answered { trace, next }
    }

    /// Intents whose channel's grace has run out with no answer, so they are no
    /// longer worth holding. Each goes into flight with its Trace. Call once a
    /// Pass.
    pub fn released(&mut self, now: Instant) -> Vec<(String, serde_json::Value, Option<Trace>)> {
        let stale: Vec<String> = self
            .waiting
            .keys()
            .filter(|c| !self.busy(now, c))
            .cloned()
            .collect();
        stale
            .into_iter()
            .filter_map(|c| {
                let (intent, trace) = self.waiting.remove(&c)?;
                self.in_flight.insert(c.clone(), (now, trace.clone()));
                Some((c, intent, trace))
            })
            .collect()
    }

    /// Take the channel for an intent that must not be held or replaced.
    ///
    /// A press is not a position. A drag's intents describe where the pointer is
    /// and the newest is the only one worth sending; a click describes something
    /// that happened, and dropping it because a later drag arrived would lose the
    /// event outright. So clicks go straight out — and take the channel with
    /// them, so the drag that follows waits its turn rather than racing.
    pub fn urgent(&mut self, now: Instant, channel: &str, trace: Option<Trace>) {
        self.in_flight.insert(channel.to_string(), (now, trace));
    }

    fn busy(&self, now: Instant, channel: &str) -> bool {
        match self.in_flight.get(channel) {
            Some((sent, _)) => now.duration_since(*sent) < REPLY_GRACE,
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace::Trace;
    use serde_json::json;

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    /// ADR 0042: a stream value that answers a channel belongs to the intent
    /// that was in flight on it — not to whatever was held behind it. And the
    /// held intent takes the channel with its own Trace, not the answered one's.
    #[test]
    fn an_answer_is_attributed_to_the_intent_in_flight() {
        let mut ob = Outbox::new();
        let base = Instant::now();

        let sent = ob.offer(
            at(base, 0),
            "vol",
            json!({ "set": 1 }),
            Some(Trace::started_at(at(base, 0))),
        );
        assert!(sent.is_some(), "an idle channel sends the intent");

        let held = ob.offer(
            at(base, 1),
            "vol",
            json!({ "set": 2 }),
            Some(Trace::started_at(at(base, 1))),
        );
        assert!(held.is_none(), "a busy channel holds the intent");

        let a = ob.answered(at(base, 5), "vol");

        let trace = a
            .trace
            .expect("the answer carries the in-flight intent's Trace");
        assert_eq!(
            trace.report(at(base, 5)),
            "presented=5.0ms total=5.0ms superseded=0",
            "origin base+0: the FIRST intent's Trace, not the held one's"
        );

        let (next, next_trace) = a.next.expect("the held intent now takes the channel");
        assert_eq!(next, json!({ "set": 2 }));
        let next_trace = next_trace.expect("the held intent carries its own Trace into flight");
        assert_eq!(
            next_trace.report(at(base, 5)),
            "presented=4.0ms total=4.0ms superseded=0",
            "origin base+1: the held intent's own Trace"
        );
    }

    #[test]
    fn an_idle_channel_takes_an_intent_straight_away() {
        let mut out = Outbox::new();
        let t = Instant::now();
        assert_eq!(
            out.offer(t, "vol", json!({ "v": 1 }), None).map(|(v, _)| v),
            Some(json!({ "v": 1 }))
        );
    }

    /// The property the whole module exists for: however many intents arrive
    /// while one is unanswered, the module is handed none of them.
    #[test]
    fn a_busy_channel_takes_nothing_however_many_arrive() {
        let mut out = Outbox::new();
        let t = Instant::now();
        out.offer(t, "vol", json!({ "v": 1 }), None);
        for v in 2..50 {
            assert!(out
                .offer(at(t, 1), "vol", json!({ "v": v }), None)
                .is_none());
        }
    }

    /// And what it hands over next is the newest of them, not the oldest — the
    /// ones in between describe positions the pointer has already left.
    #[test]
    fn the_answer_releases_the_newest_waiting_intent() {
        let mut out = Outbox::new();
        let t = Instant::now();
        out.offer(t, "vol", json!({ "v": 1 }), None);
        out.offer(at(t, 1), "vol", json!({ "v": 2 }), None);
        out.offer(at(t, 2), "vol", json!({ "v": 3 }), None);
        assert_eq!(
            out.answered(at(t, 5), "vol").next.map(|(v, _)| v),
            Some(json!({ "v": 3 })),
            "the intents between 1 and 3 were superseded, not queued"
        );
    }

    #[test]
    fn an_answer_with_nothing_waiting_releases_nothing() {
        let mut out = Outbox::new();
        let t = Instant::now();
        out.offer(t, "vol", json!({ "v": 1 }), None);
        assert!(out.answered(at(t, 5), "vol").next.is_none());
    }

    /// A module that never answers must not be silenced after one intent.
    #[test]
    fn a_silent_channel_recovers_after_the_grace_period() {
        let mut out = Outbox::new();
        let t = Instant::now();
        out.offer(t, "beep", json!({ "n": 1 }), None);
        assert!(out
            .offer(at(t, 5), "beep", json!({ "n": 2 }), None)
            .is_none());

        let released = out.released(at(t, 41));
        let released: Vec<_> = released.into_iter().map(|(c, v, _)| (c, v)).collect();
        assert_eq!(released, vec![("beep".to_string(), json!({ "n": 2 }))]);
    }

    #[test]
    fn nothing_is_released_while_the_grace_period_holds() {
        let mut out = Outbox::new();
        let t = Instant::now();
        out.offer(t, "beep", json!({ "n": 1 }), None);
        out.offer(at(t, 5), "beep", json!({ "n": 2 }), None);
        assert!(out.released(at(t, 20)).is_empty());
    }

    /// One slow module must not hold up a fast one.
    #[test]
    fn channels_do_not_block_each_other() {
        let mut out = Outbox::new();
        let t = Instant::now();
        out.offer(t, "slow", json!({ "a": 1 }), None);
        assert_eq!(
            out.offer(t, "fast", json!({ "b": 1 }), None)
                .map(|(v, _)| v),
            Some(json!({ "b": 1 })),
            "a busy channel says nothing about any other"
        );
        assert!(out
            .offer(at(t, 1), "slow", json!({ "a": 2 }), None)
            .is_none());
    }

    /// A click is not a position, so it is never held back — and it claims the
    /// channel, so the drag that follows it queues behind rather than racing it.
    #[test]
    fn an_urgent_intent_takes_the_channel() {
        let mut out = Outbox::new();
        let t = Instant::now();
        out.urgent(t, "vol", None);
        assert!(out
            .offer(at(t, 1), "vol", json!({ "v": 9 }), None)
            .is_none());
        assert_eq!(
            out.answered(at(t, 5), "vol").next.map(|(v, _)| v),
            Some(json!({ "v": 9 })),
            "the drag intent held behind a click still goes once the module answers"
        );
    }

    /// After an answer the channel is genuinely free again, not merely drained.
    #[test]
    fn a_channel_answered_with_nothing_waiting_accepts_the_next_intent() {
        let mut out = Outbox::new();
        let t = Instant::now();
        out.offer(t, "vol", json!({ "v": 1 }), None);
        out.answered(at(t, 5), "vol");
        assert_eq!(
            out.offer(at(t, 6), "vol", json!({ "v": 2 }), None)
                .map(|(v, _)| v),
            Some(json!({ "v": 2 }))
        );
    }
}
