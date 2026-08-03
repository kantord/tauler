use crate::ipc::{BarConfig, GapOverrides};

pub struct InitEvent {
    pub output: String,
    pub left_width: u32,
    pub right_width: u32,
    pub dpi: f32,
    pub outer_gap: u32,
    pub gaps: GapOverrides,
}

impl InitEvent {
    pub fn bar_config(&self) -> BarConfig {
        BarConfig {
            output: self.output.clone(),
            dpi: self.dpi,
            left: self.left_width,
            right: self.right_width,
            outer_gap: self.outer_gap,
            gaps: self.gaps,
        }
    }
}

pub fn parse_init_event(json: &str) -> Option<InitEvent> {
    let val: serde_json::Value = serde_json::from_str(json).ok()?;
    if val["type"].as_str() != Some("init") {
        return None;
    }
    let side = |name: &str| val["gaps"][name].as_u64().map(|v| v as u32);
    Some(InitEvent {
        output: val["output"].as_str()?.to_string(),
        left_width: val["config"]["left"].as_u64()? as u32,
        right_width: val["config"]["right"].as_u64().unwrap_or(0) as u32,
        dpi: val["dpi"].as_f64().unwrap_or(96.0) as f32,
        outer_gap: val["config"]["outer_gap"].as_u64().unwrap_or(0) as u32,
        gaps: GapOverrides {
            left: side("left"),
            right: side("right"),
            top: side("top"),
            bottom: side("bottom"),
        },
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
        let json = r#"{"type":"init","output":"DP-1","config":{"left":200,"right":87,"outer_gap":8},"dpi":96.0}"#;
        let ev = parse_init_event(json).unwrap();
        assert_eq!(ev.output, "DP-1");
        assert_eq!(ev.left_width, 200);
        assert_eq!(ev.right_width, 87);
        assert_eq!(ev.outer_gap, 8);
        assert!((ev.dpi - 96.0).abs() < 0.01);
    }

    #[test]
    fn parse_init_event_defaults_outer_gap_to_zero() {
        let json = r#"{"type":"init","output":"DP-1","config":{"left":200},"dpi":96.0}"#;
        let ev = parse_init_event(json).unwrap();
        assert_eq!(ev.outer_gap, 0);
    }

    #[test]
    fn parse_init_event_defaults_dpi_to_96() {
        let json = r#"{"type":"init","output":"DP-1","config":{"left":200}}"#;
        let ev = parse_init_event(json).unwrap();
        assert!((ev.dpi - 96.0).abs() < 0.01);
    }

    #[test]
    fn parse_init_event_defaults_right_width_to_zero() {
        let json = r#"{"type":"init","output":"DP-1","config":{"left":200}}"#;
        let ev = parse_init_event(json).unwrap();
        assert_eq!(ev.right_width, 0);
    }

    #[test]
    fn parse_init_event_reads_declared_gap_overrides() {
        let json =
            r#"{"type":"init","output":"DP-1","config":{"left":200},"gaps":{"left":300,"top":0}}"#;
        let ev = parse_init_event(json).unwrap();
        assert_eq!(ev.gaps.left, Some(300));
        assert_eq!(ev.gaps.top, Some(0), "an explicit 0 is a declaration");
        assert_eq!(ev.gaps.right, None, "absent sides stay derived");
        assert_eq!(ev.gaps.bottom, None);
    }

    #[test]
    fn parse_init_event_defaults_gap_overrides_to_absent() {
        let json = r#"{"type":"init","output":"DP-1","config":{"left":200}}"#;
        let ev = parse_init_event(json).unwrap();
        assert_eq!(ev.gaps, crate::ipc::GapOverrides::default());
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
