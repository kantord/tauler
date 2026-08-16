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

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// How long an unanswered intent holds its channel.
///
/// A module that answers releases its channel far sooner than this, so for the
/// ones this exists for — the silent ones — it is what sets the rate: 25 intents
/// a second, which is more than a person dragging can perceive and far more than
/// a subprocess forking per message can absorb.
const REPLY_GRACE: Duration = Duration::from_millis(40);

#[derive(Default)]
pub struct Outbox {
    /// When the intent currently occupying each channel was sent.
    sent_at: HashMap<String, Instant>,
    /// The newest intent waiting for each occupied channel.
    waiting: HashMap<String, serde_json::Value>,
}

impl Outbox {
    pub fn new() -> Self {
        Self::default()
    }

    /// Offer an intent for `channel`. Returns it if it should be written now.
    ///
    /// If the channel is busy the intent is kept instead, replacing whatever was
    /// already waiting — a superseded intent describes a state the newer one has
    /// already moved past.
    pub fn offer(
        &mut self,
        now: Instant,
        channel: &str,
        intent: serde_json::Value,
    ) -> Option<serde_json::Value> {
        if self.busy(now, channel) {
            self.waiting.insert(channel.to_string(), intent);
            return None;
        }
        self.sent_at.insert(channel.to_string(), now);
        Some(intent)
    }

    /// A module emitted a line, so its channel is free. Returns whatever was
    /// waiting for it, which is then in flight in its turn.
    pub fn answered(&mut self, now: Instant, channel: &str) -> Option<serde_json::Value> {
        self.sent_at.remove(channel);
        let next = self.waiting.remove(channel)?;
        self.sent_at.insert(channel.to_string(), now);
        Some(next)
    }

    /// Intents whose channel's grace has run out with no answer, so they are no
    /// longer worth holding. Call once a Pass.
    pub fn released(&mut self, now: Instant) -> Vec<(String, serde_json::Value)> {
        let stale: Vec<String> = self
            .waiting
            .keys()
            .filter(|c| !self.busy(now, c))
            .cloned()
            .collect();
        stale
            .into_iter()
            .filter_map(|c| {
                let intent = self.waiting.remove(&c)?;
                self.sent_at.insert(c.clone(), now);
                Some((c, intent))
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
    pub fn urgent(&mut self, now: Instant, channel: &str) {
        self.sent_at.insert(channel.to_string(), now);
    }

    fn busy(&self, now: Instant, channel: &str) -> bool {
        match self.sent_at.get(channel) {
            Some(sent) => now.duration_since(*sent) < REPLY_GRACE,
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    #[test]
    fn an_idle_channel_takes_an_intent_straight_away() {
        let mut out = Outbox::new();
        let t = Instant::now();
        assert_eq!(
            out.offer(t, "vol", json!({ "v": 1 })),
            Some(json!({ "v": 1 }))
        );
    }

    /// The property the whole module exists for: however many intents arrive
    /// while one is unanswered, the module is handed none of them.
    #[test]
    fn a_busy_channel_takes_nothing_however_many_arrive() {
        let mut out = Outbox::new();
        let t = Instant::now();
        out.offer(t, "vol", json!({ "v": 1 }));
        for v in 2..50 {
            assert_eq!(out.offer(at(t, 1), "vol", json!({ "v": v })), None);
        }
    }

    /// And what it hands over next is the newest of them, not the oldest — the
    /// ones in between describe positions the pointer has already left.
    #[test]
    fn the_answer_releases_the_newest_waiting_intent() {
        let mut out = Outbox::new();
        let t = Instant::now();
        out.offer(t, "vol", json!({ "v": 1 }));
        out.offer(at(t, 1), "vol", json!({ "v": 2 }));
        out.offer(at(t, 2), "vol", json!({ "v": 3 }));
        assert_eq!(
            out.answered(at(t, 5), "vol"),
            Some(json!({ "v": 3 })),
            "the intents between 1 and 3 were superseded, not queued"
        );
    }

    #[test]
    fn an_answer_with_nothing_waiting_releases_nothing() {
        let mut out = Outbox::new();
        let t = Instant::now();
        out.offer(t, "vol", json!({ "v": 1 }));
        assert_eq!(out.answered(at(t, 5), "vol"), None);
    }

    /// A module that never answers must not be silenced after one intent.
    #[test]
    fn a_silent_channel_recovers_after_the_grace_period() {
        let mut out = Outbox::new();
        let t = Instant::now();
        out.offer(t, "beep", json!({ "n": 1 }));
        assert_eq!(out.offer(at(t, 5), "beep", json!({ "n": 2 })), None);

        let released = out.released(at(t, 41));
        assert_eq!(released, vec![("beep".to_string(), json!({ "n": 2 }))]);
    }

    #[test]
    fn nothing_is_released_while_the_grace_period_holds() {
        let mut out = Outbox::new();
        let t = Instant::now();
        out.offer(t, "beep", json!({ "n": 1 }));
        out.offer(at(t, 5), "beep", json!({ "n": 2 }));
        assert!(out.released(at(t, 20)).is_empty());
    }

    /// One slow module must not hold up a fast one.
    #[test]
    fn channels_do_not_block_each_other() {
        let mut out = Outbox::new();
        let t = Instant::now();
        out.offer(t, "slow", json!({ "a": 1 }));
        assert_eq!(
            out.offer(t, "fast", json!({ "b": 1 })),
            Some(json!({ "b": 1 })),
            "a busy channel says nothing about any other"
        );
        assert_eq!(out.offer(at(t, 1), "slow", json!({ "a": 2 })), None);
    }

    /// A click is not a position, so it is never held back — and it claims the
    /// channel, so the drag that follows it queues behind rather than racing it.
    #[test]
    fn an_urgent_intent_takes_the_channel() {
        let mut out = Outbox::new();
        let t = Instant::now();
        out.urgent(t, "vol");
        assert_eq!(out.offer(at(t, 1), "vol", json!({ "v": 9 })), None);
        assert_eq!(
            out.answered(at(t, 5), "vol"),
            Some(json!({ "v": 9 })),
            "the drag intent held behind a click still goes once the module answers"
        );
    }

    /// After an answer the channel is genuinely free again, not merely drained.
    #[test]
    fn a_channel_answered_with_nothing_waiting_accepts_the_next_intent() {
        let mut out = Outbox::new();
        let t = Instant::now();
        out.offer(t, "vol", json!({ "v": 1 }));
        out.answered(at(t, 5), "vol");
        assert_eq!(
            out.offer(at(t, 6), "vol", json!({ "v": 2 })),
            Some(json!({ "v": 2 }))
        );
    }
}
