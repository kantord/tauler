//! Turning a pointer event into intents on a module's stdin.
//!
//! Not display-server code: X11 and Wayland both feed the same three phases in here
//! (`docs/adr/0010`), and everything below is about what a press or a drag *means*,
//! not how it arrived.

use std::collections::HashMap;
use std::sync::mpsc;

/// Dispatches a handler's intents: an array, each
/// `{"channel": "<bin>", "event": {...}}`. The `event` object goes to the
/// channel's sender verbatim. One bad intent never stops the others.
pub fn dispatch(
    module_event_txs: &HashMap<String, mpsc::Sender<serde_json::Value>>,
    on_click: &serde_json::Value,
) {
    let Some(intents) = on_click.as_array() else {
        tracing::warn!(on_click = %on_click, "on_click is not an array of intents");
        return;
    };
    for intent in intents {
        let Some(channel) = intent.get("channel").and_then(|v| v.as_str()) else {
            tracing::warn!(intent = %intent, "click intent has no channel");
            continue;
        };
        let Some(tx) = module_event_txs.get(channel) else {
            tracing::warn!(channel, known_channels = ?module_event_txs.keys().collect::<Vec<_>>(), "click intent channel not found");
            continue;
        };
        let Some(event) = intent.get("event") else {
            tracing::warn!(channel, "click intent has no event");
            continue;
        };
        let result = tx.send(event.clone());
        tracing::debug!(channel, ok = result.is_ok(), "click intent dispatched");
    }
}

/// A handler as the layout wrote it: intents to send as-is, or a function to call.
///
/// Two shapes because both are legal everywhere (`docs/adr/0021`). The id form is what
/// the node flattener leaves behind when it moves a function into the JS registry.
pub enum Handler {
    Intents(serde_json::Value),
    Function(i64),
}

/// Read a handler value, whichever shape it came in.
///
/// A value that is neither is warned about here rather than dropped: it is the one
/// place that sees the malformed shape, since nothing downstream is reached.
pub fn read_handler(value: &serde_json::Value) -> Option<Handler> {
    if let Some(id) = value.get("$handler").and_then(serde_json::Value::as_i64) {
        return Some(Handler::Function(id));
    }
    if value.is_array() {
        return Some(Handler::Intents(value.clone()));
    }
    tracing::warn!(
        handler = %value,
        "a handler must be an array of intents or a function; this one is neither"
    );
    None
}

/// An in-progress drag: the geometry and the panel it was pressed in, plus what was
/// last sent so a motion producing the same intents can be skipped.
///
/// The handler itself lives in the JS capture slot — it has to outlive the tick it was
/// registered in, and there is no node identity to find it by again (`docs/adr/0020`).
pub struct Capture {
    pub panel_id: String,
    pub rect: crate::hit_test::Rect,
    pub dpr: f32,
    last: Option<serde_json::Value>,
}

impl Capture {
    pub fn new(panel_id: String, rect: crate::hit_test::Rect, dpr: f32) -> Self {
        Self {
            panel_id,
            rect,
            dpr,
            last: None,
        }
    }

    /// Whether these intents are new, recording them if so.
    ///
    /// The skip is what keeps a drag to one message per distinct value instead of one
    /// per motion event (`docs/adr/0020`). Lives here because the previous dispatch is
    /// the capture's own business — nothing else has a reason to know it.
    pub fn is_new(&mut self, intents: &serde_json::Value) -> bool {
        if self.last.as_ref() == Some(intents) {
            return false;
        }
        self.last = Some(intents.clone());
        true
    }

    /// Record a press's intents without skipping. A press always fires, so it only
    /// seeds what the drag that follows will compare against.
    pub fn seed(&mut self, intents: serde_json::Value) {
        self.last = Some(intents);
    }
}

#[cfg(test)]
mod handler_shapes {
    use super::{read_handler, Handler};
    use serde_json::json;

    #[test]
    fn an_array_is_intents_to_send_as_they_are() {
        let v = json!([{"channel": "t", "event": {"type": "x"}}]);
        assert!(matches!(read_handler(&v), Some(Handler::Intents(_))));
    }

    #[test]
    fn a_handler_id_is_a_function_to_call() {
        assert!(matches!(
            read_handler(&json!({"$handler": 7})),
            Some(Handler::Function(7))
        ));
    }

    /// A handler written as a bare object is neither, and must not be mistaken for
    /// intents. It is diagnosed here because nothing downstream is reached to do it.
    #[test]
    #[tracing_test::traced_test]
    fn a_bare_object_is_neither_and_says_so() {
        assert!(read_handler(&json!({"channel": "t"})).is_none());
        assert!(logs_contain("WARN"), "a malformed handler warns");
        assert!(
            logs_contain("neither"),
            "the warning says what the two legal shapes are"
        );
    }
}

#[cfg(test)]
mod capture_dedup {
    use super::Capture;
    use crate::hit_test::Rect;
    use serde_json::json;

    fn capture() -> Capture {
        Capture::new(
            "bar".into(),
            Rect {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 16.0,
            },
            1.0,
        )
    }

    /// What keeps a drag to one message per distinct value instead of one per pixel.
    #[test]
    fn a_motion_producing_what_was_just_sent_is_not_new() {
        let mut c = capture();
        let same = json!([{"channel": "m", "event": {"v": 1}}]);
        assert!(c.is_new(&same), "the first one goes");
        assert!(!c.is_new(&same), "the repeat does not");
    }

    #[test]
    fn a_motion_producing_a_different_value_is_new() {
        let mut c = capture();
        assert!(c.is_new(&json!([{"channel": "m", "event": {"v": 1}}])));
        assert!(c.is_new(&json!([{"channel": "m", "event": {"v": 2}}])));
    }

    /// A press seeds the comparison rather than being compared: it always fires, and
    /// the drag that follows is what needs something to differ from.
    #[test]
    fn a_seeded_press_is_what_the_first_motion_is_compared_against() {
        let mut c = capture();
        let pressed = json!([{"channel": "m", "event": {"v": 1}}]);
        c.seed(pressed.clone());
        assert!(!c.is_new(&pressed), "a motion that has not left the value");
    }
}

#[cfg(test)]
mod tests {
    use super::dispatch;
    use std::collections::HashMap;
    use std::sync::mpsc;

    fn make_txs(
        names: &[&str],
    ) -> (
        HashMap<String, mpsc::Sender<serde_json::Value>>,
        Vec<mpsc::Receiver<serde_json::Value>>,
    ) {
        let mut txs = HashMap::new();
        let mut rxs = Vec::new();
        for &name in names {
            let (tx, rx) = mpsc::channel();
            txs.insert(name.to_string(), tx);
            rxs.push(rx);
        }
        (txs, rxs)
    }

    #[test]
    fn single_intent_delivers_event_object_verbatim() {
        let (txs, rxs) = make_txs(&["my-module"]);
        let event = serde_json::json!({
            "type": "switchWorkspace",
            "workspace": "1: web"
        });
        let on_click = serde_json::json!([
            {"channel": "my-module", "event": event.clone()}
        ]);
        dispatch(&txs, &on_click);
        let msg = rxs[0].try_recv().expect("should receive a message");
        assert_eq!(
            msg, event,
            "the event object must be delivered verbatim: no envelope, no channel key"
        );
    }

    #[test]
    fn two_intents_each_reach_their_own_channel() {
        let (txs, rxs) = make_txs(&["module-a", "module-b"]);
        let event_a = serde_json::json!({"type": "a-thing", "n": 1});
        let event_b = serde_json::json!({"type": "b-thing", "n": 2});
        let on_click = serde_json::json!([
            {"channel": "module-a", "event": event_a.clone()},
            {"channel": "module-b", "event": event_b.clone()},
        ]);
        dispatch(&txs, &on_click);
        assert_eq!(rxs[0].try_recv().expect("module-a should receive"), event_a);
        assert_eq!(rxs[1].try_recv().expect("module-b should receive"), event_b);
    }

    #[test]
    #[tracing_test::traced_test]
    fn unknown_channel_is_skipped_but_other_intents_still_dispatch() {
        let (txs, rxs) = make_txs(&["known-module"]);
        let event = serde_json::json!({"type": "do-thing"});
        let on_click = serde_json::json!([
            {"channel": "ghost-module", "event": {"type": "vanishes"}},
            {"channel": "known-module", "event": event.clone()},
        ]);
        dispatch(&txs, &on_click);
        assert_eq!(
            rxs[0]
                .try_recv()
                .expect("known-module should still receive its intent"),
            event
        );
        assert!(logs_contain("WARN"), "unknown channel should log at WARN");
        assert!(
            logs_contain("ghost-module"),
            "warn should name the unknown channel"
        );
        assert!(
            logs_contain("known-module"),
            "warn should list the known channels"
        );
    }

    #[test]
    #[tracing_test::traced_test]
    fn non_array_on_click_dispatches_nothing() {
        let (txs, rxs) = make_txs(&["my-module"]);
        let on_click = serde_json::json!({"channel": "my-module", "event": {"type": "x"}});
        dispatch(&txs, &on_click);
        assert!(
            rxs[0].try_recv().is_err(),
            "a non-array on_click must dispatch nothing"
        );
        assert!(
            logs_contain("WARN"),
            "a non-array on_click should log at WARN"
        );
    }
}
