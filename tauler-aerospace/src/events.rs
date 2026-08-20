//! The two messages this module reads on stdin.

use serde_json::Value;

/// Returns the workspace name from a `switchWorkspace` intent, or None for any
/// other event.
pub fn parse_switch_workspace(val: &Value) -> Option<String> {
    if val["type"].as_str() != Some("switchWorkspace") {
        return None;
    }
    val["workspace"].as_str().map(str::to_string)
}

/// True once tauler has sent the init event. Nothing in it is needed —
/// AeroSpace gaps are static config, so there is no reservation to apply — but
/// the module still waits for it before its first refresh, so that a layout
/// reload restarts the stream rather than leaving a stale strip on screen.
pub fn is_init_event(json: &str) -> bool {
    serde_json::from_str::<Value>(json)
        .map(|v| v["type"] == "init")
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_the_workspace_name_out_of_a_switch_intent() {
        let got = parse_switch_workspace(&json!({"type": "switchWorkspace", "workspace": "3"}));
        assert_eq!(got.as_deref(), Some("3"));
    }

    #[test]
    fn ignores_an_intent_of_another_type() {
        assert!(parse_switch_workspace(&json!({"type": "dismiss", "workspace": "3"})).is_none());
    }

    #[test]
    fn ignores_a_switch_intent_with_no_workspace() {
        assert!(parse_switch_workspace(&json!({"type": "switchWorkspace"})).is_none());
    }

    #[test]
    fn recognises_the_init_event_and_nothing_else() {
        assert!(is_init_event(
            r#"{"type":"init","output":"Built-in Retina Display"}"#
        ));
        assert!(!is_init_event(
            r#"{"type":"switchWorkspace","workspace":"1"}"#
        ));
        assert!(!is_init_event("not json"));
    }
}
