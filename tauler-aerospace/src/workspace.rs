//! Turns AeroSpace's two list queries into the payload a layout file reads.
//!
//! `aerospace list-workspaces` knows which workspaces exist and which is
//! focused; `aerospace list-windows` knows what is on them. Neither alone is
//! enough to draw a workspace strip, so they are joined here.

use serde_json::Value;

/// One workspace, as a layout file sees it.
///
/// Field names match `tauler-i3`'s so a workspace strip can be moved between
/// the two with no edit. `urgent` is always false: AeroSpace has no urgency
/// hint, and macOS has nothing to derive one from.
#[derive(Debug, PartialEq, Eq)]
pub struct Workspace {
    pub name: String,
    pub focused: bool,
    pub visible: bool,
    pub urgent: bool,
    pub focused_windows: Vec<String>,
    pub apps: Vec<String>,
}

impl Workspace {
    fn to_json(&self) -> Value {
        serde_json::json!({
            "name": self.name,
            "focused": self.focused,
            "visible": self.visible,
            "urgent": self.urgent,
            "focused_windows": self.focused_windows,
            "apps": self.apps,
        })
    }
}

/// The one line this module writes to stdout per refresh.
pub fn payload(workspaces: &[Workspace]) -> Value {
    serde_json::json!({
        "workspaces": workspaces.iter().map(Workspace::to_json).collect::<Vec<_>>(),
    })
}

fn str_field(v: &Value, key: &str) -> Option<String> {
    v.get(key)?.as_str().map(str::to_string)
}

/// Join `list-workspaces` and `list-windows` output into the workspace strip.
///
/// Workspace order follows `list-workspaces`; window order follows
/// `list-windows`. Unknown or malformed entries are skipped rather than
/// failing the whole refresh — a single unparseable window should not blank
/// the bar.
pub fn build(workspaces_json: &Value, windows_json: &Value) -> Vec<Workspace> {
    let empty = Vec::new();
    let windows = windows_json.as_array().unwrap_or(&empty);

    workspaces_json
        .as_array()
        .unwrap_or(&empty)
        .iter()
        .filter_map(|ws| {
            let name = str_field(ws, "workspace")?;
            let mine = windows
                .iter()
                .filter(|w| str_field(w, "workspace").as_deref() == Some(name.as_str()));
            let mut focused_windows = Vec::new();
            let mut apps = Vec::new();
            for w in mine {
                if let Some(title) = str_field(w, "window-title") {
                    focused_windows.push(title);
                }
                if let Some(app) = str_field(w, "app-name")
                    && !apps.contains(&app)
                {
                    apps.push(app);
                }
            }
            Some(Workspace {
                focused: ws["workspace-is-focused"].as_bool().unwrap_or(false),
                visible: ws["workspace-is-visible"].as_bool().unwrap_or(false),
                urgent: false,
                name,
                focused_windows,
                apps,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ws(name: &str, focused: bool, visible: bool) -> Value {
        json!({
            "workspace": name,
            "workspace-is-focused": focused,
            "workspace-is-visible": visible,
            "monitor-id": 1,
        })
    }

    fn win(workspace: &str, app: &str, title: &str) -> Value {
        json!({
            "workspace": workspace,
            "app-name": app,
            "app-bundle-id": "com.example.app",
            "window-id": 1,
            "window-title": title,
            "window-is-fullscreen": false,
        })
    }

    #[test]
    fn keeps_the_order_and_flags_reported_by_list_workspaces() {
        let got = build(
            &json!([ws("1", true, true), ws("2", false, false)]),
            &json!([]),
        );
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].name, "1");
        assert!(got[0].focused && got[0].visible);
        assert_eq!(got[1].name, "2");
        assert!(!got[1].focused && !got[1].visible);
    }

    #[test]
    fn attaches_each_window_to_the_workspace_that_holds_it() {
        let got = build(
            &json!([ws("1", true, true), ws("2", false, false)]),
            &json!([
                win("1", "Claude", "Claude"),
                win("2", "Finder", "Downloads")
            ]),
        );
        assert_eq!(got[0].focused_windows, vec!["Claude"]);
        assert_eq!(got[1].focused_windows, vec!["Downloads"]);
    }

    #[test]
    fn reports_an_empty_workspace_as_having_no_windows() {
        let got = build(
            &json!([ws("3", false, false)]),
            &json!([win("1", "A", "a")]),
        );
        assert!(got[0].focused_windows.is_empty());
        assert!(got[0].apps.is_empty());
    }

    #[test]
    fn lists_each_app_once_however_many_windows_it_has() {
        let got = build(
            &json!([ws("1", true, true)]),
            &json!([
                win("1", "Finder", "Downloads"),
                win("1", "Finder", "Documents"),
                win("1", "Claude", "Claude"),
            ]),
        );
        assert_eq!(got[0].apps, vec!["Finder", "Claude"]);
        assert_eq!(got[0].focused_windows.len(), 3);
    }

    #[test]
    fn skips_a_workspace_entry_that_has_no_name() {
        let got = build(
            &json!([json!({"monitor-id": 1}), ws("2", false, false)]),
            &json!([]),
        );
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "2");
    }

    #[test]
    fn payload_wraps_the_strip_under_a_workspaces_key() {
        let got = payload(&build(&json!([ws("1", true, true)]), &json!([])));
        assert_eq!(got["workspaces"][0]["name"], "1");
        assert_eq!(got["workspaces"][0]["focused"], true);
        assert_eq!(got["workspaces"][0]["urgent"], false);
    }
}
