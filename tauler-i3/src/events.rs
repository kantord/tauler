pub struct InitEvent {
    pub output: String,
    pub bar_width: u32,
    pub dpi: f32,
    pub outer_gap: u32,
}

pub fn parse_init_event(json: &str) -> Option<InitEvent> {
    let val: serde_json::Value = serde_json::from_str(json).ok()?;
    if val["type"].as_str() != Some("init") {
        return None;
    }
    Some(InitEvent {
        output: val["output"].as_str()?.to_string(),
        bar_width: val["config"]["width"].as_u64()? as u32,
        dpi: val["dpi"].as_f64().unwrap_or(96.0) as f32,
        outer_gap: val["config"]["outer_gap"].as_u64().unwrap_or(0) as u32,
    })
}

/// Returns the workspace name from a click event, or None if not a workspace click.
pub fn parse_click_event(val: &serde_json::Value) -> Option<String> {
    if val["event"].as_str() != Some("click") {
        return None;
    }
    val["data"]["workspace"].as_str().map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_init_event_extracts_output_and_config() {
        let json =
            r#"{"type":"init","output":"DP-1","config":{"width":200,"outer_gap":8},"dpi":96.0}"#;
        let ev = parse_init_event(json).unwrap();
        assert_eq!(ev.output, "DP-1");
        assert_eq!(ev.bar_width, 200);
        assert_eq!(ev.outer_gap, 8);
        assert!((ev.dpi - 96.0).abs() < 0.01);
    }

    #[test]
    fn parse_init_event_defaults_outer_gap_to_zero() {
        let json = r#"{"type":"init","output":"DP-1","config":{"width":200},"dpi":96.0}"#;
        let ev = parse_init_event(json).unwrap();
        assert_eq!(ev.outer_gap, 0);
    }

    #[test]
    fn parse_init_event_defaults_dpi_to_96() {
        let json = r#"{"type":"init","output":"DP-1","config":{"width":200}}"#;
        let ev = parse_init_event(json).unwrap();
        assert!((ev.dpi - 96.0).abs() < 0.01);
    }

    #[test]
    fn parse_init_event_returns_none_for_wrong_type() {
        let json = r#"{"type":"ping","output":"DP-1","config":{"width":200}}"#;
        assert!(parse_init_event(json).is_none());
    }

    #[test]
    fn parse_init_event_returns_none_for_invalid_json() {
        assert!(parse_init_event("not json").is_none());
    }

    #[test]
    fn parse_click_event_extracts_workspace_name() {
        let json = serde_json::json!({"event": "click", "data": {"workspace": "1: web"}});
        assert_eq!(parse_click_event(&json).as_deref(), Some("1: web"));
    }

    #[test]
    fn parse_click_event_returns_none_for_non_click_event() {
        let json = serde_json::json!({"event": "hover", "data": {"workspace": "1: web"}});
        assert!(parse_click_event(&json).is_none());
    }

    #[test]
    fn parse_click_event_returns_none_when_no_workspace_data() {
        let json = serde_json::json!({"event": "click", "data": {}});
        assert!(parse_click_event(&json).is_none());
    }
}
