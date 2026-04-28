use std::collections::HashMap;
use std::sync::mpsc;

use crate::modules::hit_test;
use crate::render::measure_layout_frame;

fn dispatch_click(
    module_event_txs: &HashMap<String, mpsc::Sender<serde_json::Value>>,
    hit_path: &str,
    on_click: &serde_json::Value,
) {
    if let Some(channel) = on_click.get("__channel__").and_then(|v| v.as_str()) {
        if let Some(tx) = module_event_txs.get(channel) {
            let mut payload = on_click.clone();
            if let Some(obj) = payload.as_object_mut() { obj.remove("__channel__"); }
            let result = tx.send(serde_json::json!({"event": "click", "data": payload}));
            tracing::debug!(channel, ok = result.is_ok(), "click dispatched via __channel__");
        } else {
            tracing::debug!(channel, known_channels = ?module_event_txs.keys().collect::<Vec<_>>(), "click __channel__ not found");
        }
        return;
    }
    let mut path = hit_path.to_string();
    loop {
        if let Some(tx) = module_event_txs.get(&path) {
            let _ = tx.send(serde_json::json!({"event": "click", "data": on_click}));
            tracing::debug!(path, "click dispatched via path");
            return;
        }
        match path.rfind('/') {
            Some(pos) => path.truncate(pos),
            None => {
                tracing::debug!(hit_path, "click: no channel matched");
                return;
            }
        }
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
    let Some(layout_json) = raw_layout.as_ref() else { return; };
    let measured = measure_layout_frame(layout_json, phys_width, phys_height, dpr);

    tracing::debug!(click_x, click_y, phys_width, phys_height, "hit test");
    let Some((hit_path, on_click)) = hit_test(&measured, layout_json, click_x, click_y) else {
        tracing::debug!(click_x, click_y, "hit test: no clickable node found");
        return;
    };

    dispatch_click(module_event_txs, &hit_path, &on_click);
}

#[cfg(test)]
mod tests {
    use super::dispatch_click;
    use std::collections::HashMap;
    use std::sync::mpsc;

    fn make_txs(names: &[&str]) -> (HashMap<String, mpsc::Sender<serde_json::Value>>, Vec<mpsc::Receiver<serde_json::Value>>) {
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
    fn channel_key_routes_to_named_channel_not_path() {
        let (txs, rxs) = make_txs(&["my-module", "some/path/module"]);
        let on_click = serde_json::json!({
            "__channel__": "my-module",
            "action": "do-thing"
        });
        dispatch_click(&txs, "some/path/module", &on_click);
        assert!(rxs[0].try_recv().is_ok(), "named channel should receive a message");
        assert!(rxs[1].try_recv().is_err(), "path channel should NOT receive a message");
    }

    #[test]
    fn channel_key_is_stripped_from_payload() {
        let (txs, rxs) = make_txs(&["my-module"]);
        let on_click = serde_json::json!({
            "__channel__": "my-module",
            "action": "do-thing"
        });
        dispatch_click(&txs, "irrelevant/path", &on_click);
        let msg = rxs[0].try_recv().expect("should receive a message");
        let data = &msg["data"];
        assert!(data.get("__channel__").is_none(), "__channel__ should be stripped from data");
        assert_eq!(data["action"], "do-thing");
    }

    #[test]
    fn unknown_channel_key_sends_nothing() {
        let (txs, rxs) = make_txs(&["known-module"]);
        let on_click = serde_json::json!({
            "__channel__": "unknown-module",
            "action": "do-thing"
        });
        dispatch_click(&txs, "known-module", &on_click);
        assert!(rxs[0].try_recv().is_err(), "no message should be sent when __channel__ is unknown");
    }

    #[test]
    fn no_channel_key_walks_path_to_find_sender() {
        let (txs, rxs) = make_txs(&["some/path"]);
        let on_click = serde_json::json!({"action": "click"});
        dispatch_click(&txs, "some/path/module", &on_click);
        let msg = rxs[0].try_recv().expect("parent path should receive a message");
        assert_eq!(msg["event"], "click");
        assert_eq!(msg["data"]["action"], "click");
    }

    #[test]
    fn no_channel_key_and_no_path_match_sends_nothing() {
        let (txs, rxs) = make_txs(&["unrelated-module"]);
        let on_click = serde_json::json!({"action": "click"});
        dispatch_click(&txs, "some/path/module", &on_click);
        assert!(rxs[0].try_recv().is_err(), "no message should be sent when no path matches");
    }
}
