use crate::ipc::{BarConfig, GapOverrides};

/// The environment tauler reports at startup.
///
/// Deliberately just facts: which output the bar is on, and whatever gaps the
/// layout declared. tauler used to also send derived `config.left`/`right`/
/// `outer_gap` — its guess at what the gaps should be — which this module then
/// had to allow overriding. Panel geometry is tauler's to know; what i3 should
/// reserve is this module's to decide, and the layout file's to state.
pub struct InitEvent {
    pub output: String,
    pub gaps: GapOverrides,
}

impl InitEvent {
    pub fn bar_config(&self) -> BarConfig {
        BarConfig {
            output: self.output.clone(),
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
        gaps: GapOverrides {
            left: side("left"),
            right: side("right"),
            top: side("top"),
            bottom: side("bottom"),
        },
    })
}

/// Returns the workspace name from a `switchWorkspace` intent, or None for any
/// other event.
pub fn parse_switch_workspace(val: &serde_json::Value) -> Option<String> {
    if val["type"].as_str() != Some("switchWorkspace") {
        return None;
    }
    val["workspace"].as_str().map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_init_event_extracts_the_output() {
        let json = r#"{"type":"init","output":"DP-1"}"#;
        let ev = parse_init_event(json).unwrap();
        assert_eq!(ev.output, "DP-1");
    }

    /// tauler no longer sends derived gap widths, so the payload must parse
    /// without them. The old parser took `config.left` with `?` and would
    /// reject the whole init.
    #[test]
    fn parse_init_event_needs_no_derived_config_block() {
        assert!(parse_init_event(r#"{"type":"init","output":"DP-1"}"#).is_some());
    }

    /// Anything tauler adds for other modules must not confuse this one.
    #[test]
    fn parse_init_event_ignores_unknown_fields() {
        let json = r#"{"type":"init","output":"DP-1","dpi":140.0,"screen_width":2633}"#;
        assert_eq!(parse_init_event(json).unwrap().output, "DP-1");
    }

    #[test]
    fn parse_init_event_reads_declared_gaps() {
        let json = r#"{"type":"init","output":"DP-1","gaps":{"left":300,"top":0}}"#;
        let ev = parse_init_event(json).unwrap();
        assert_eq!(ev.gaps.left, Some(300));
        assert_eq!(ev.gaps.top, Some(0), "an explicit 0 is a declaration");
        assert_eq!(ev.gaps.right, None, "an absent side means no gap");
        assert_eq!(ev.gaps.bottom, None);
    }

    #[test]
    fn parse_init_event_defaults_gaps_to_absent() {
        let json = r#"{"type":"init","output":"DP-1"}"#;
        let ev = parse_init_event(json).unwrap();
        assert_eq!(ev.gaps, crate::ipc::GapOverrides::default());
    }

    #[test]
    fn parse_init_event_returns_none_for_wrong_type() {
        let json = r#"{"type":"ping","output":"DP-1"}"#;
        assert!(parse_init_event(json).is_none());
    }

    #[test]
    fn parse_init_event_returns_none_for_invalid_json() {
        assert!(parse_init_event("not json").is_none());
    }

    #[test]
    fn parse_switch_workspace_extracts_workspace_name() {
        let json = serde_json::json!({"type": "switchWorkspace", "workspace": "1: web"});
        assert_eq!(parse_switch_workspace(&json).as_deref(), Some("1: web"));
    }

    #[test]
    fn parse_switch_workspace_returns_none_for_old_envelope_shape() {
        let json = serde_json::json!({"event": "click", "data": {"workspace": "1: web"}});
        assert!(parse_switch_workspace(&json).is_none());
    }

    #[test]
    fn parse_switch_workspace_returns_none_for_different_type() {
        let json = serde_json::json!({"type": "focusWindow", "workspace": "1: web"});
        assert!(parse_switch_workspace(&json).is_none());
    }

    #[test]
    fn parse_switch_workspace_returns_none_when_workspace_missing() {
        let json = serde_json::json!({"type": "switchWorkspace"});
        assert!(parse_switch_workspace(&json).is_none());
    }
}
