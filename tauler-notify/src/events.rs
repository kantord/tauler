/// Returns the notification id from a `dismiss` intent, or None for any other
/// event.
pub fn parse_dismiss(val: &serde_json::Value) -> Option<u32> {
    if val["type"].as_str() != Some("dismiss") {
        return None;
    }
    val["id"].as_u64().map(|id| id as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_dismiss_extracts_notification_id() {
        let json = serde_json::json!({"type": "dismiss", "id": 42});
        assert_eq!(parse_dismiss(&json), Some(42));
    }

    #[test]
    fn parse_dismiss_returns_none_for_different_type() {
        let json = serde_json::json!({"type": "switchWorkspace", "id": 42});
        assert!(parse_dismiss(&json).is_none());
    }

    #[test]
    fn parse_dismiss_returns_none_for_init_event() {
        let json = serde_json::json!({"type": "init", "output": "DP-1"});
        assert!(parse_dismiss(&json).is_none());
    }

    #[test]
    fn parse_dismiss_returns_none_when_id_missing() {
        let json = serde_json::json!({"type": "dismiss"});
        assert!(parse_dismiss(&json).is_none());
    }

    #[test]
    fn parse_dismiss_returns_none_when_id_is_not_a_number() {
        let json = serde_json::json!({"type": "dismiss", "id": "42"});
        assert!(parse_dismiss(&json).is_none());
    }
}
