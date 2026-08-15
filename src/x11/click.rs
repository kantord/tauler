use std::collections::HashMap;
use std::sync::mpsc;

use crate::hit_test::hit_test;

/// Dispatches a click's `on_click`: an array of intents, each
/// `{"channel": "<bin>", "event": {...}}`. The `event` object goes to the
/// channel's sender verbatim. One bad intent never stops the others.
fn dispatch_click(
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

#[allow(clippy::too_many_arguments)]
pub fn do_hit_test(
    raw_layout: &Option<serde_json::Value>,
    module_event_txs: &HashMap<String, mpsc::Sender<serde_json::Value>>,
    phys_width: u32,
    phys_height: u32,
    dpr: f32,
    click_x: f32,
    click_y: f32,
) {
    let Some(layout_json) = raw_layout.as_ref() else {
        return;
    };
    tracing::debug!(click_x, click_y, phys_width, phys_height, "hit test");
    let Some(on_click) = hit_test(layout_json, phys_width, phys_height, dpr, click_x, click_y)
    else {
        tracing::debug!(click_x, click_y, "hit test: no clickable node found");
        return;
    };

    dispatch_click(module_event_txs, &on_click);
}

#[cfg(test)]
mod tests {
    use super::dispatch_click;
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
        dispatch_click(&txs, &on_click);
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
        dispatch_click(&txs, &on_click);
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
        dispatch_click(&txs, &on_click);
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
        dispatch_click(&txs, &on_click);
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
