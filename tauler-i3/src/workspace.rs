use std::collections::HashMap;
use std::os::unix::net::UnixStream;

use crate::ipc::{i3_recv, i3_send};

pub struct Workspace {
    pub name: String,
    pub focused: bool,
    pub urgent: bool,
    pub focused_windows: Vec<String>,
}

/// Returns true if `workspace_output` matches the given `filter`.
///
/// An empty `filter` means "show all outputs" (used by Wayland callers that
/// have no per-monitor filtering), so every `workspace_output` value passes.
/// A non-empty `filter` requires an exact match.
pub fn workspace_matches_output(workspace_output: &str, filter: &str) -> bool {
    filter.is_empty() || workspace_output == filter
}

/// Walk an i3 tree node following `focus` order, collecting leaf window titles
/// into `out` in most-recently-focused order.
pub fn collect_window_titles_in_focus_order(node: &serde_json::Value, out: &mut Vec<String>) {
    // Leaf: actual X11/XCB window
    if node["window"].is_i64() || node["window"].is_u64() {
        if let Some(title) = node["name"].as_str() {
            out.push(title.to_string());
        }
        return;
    }

    let focus_ids: Vec<i64> = node["focus"]
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_i64()).collect())
        .unwrap_or_default();

    let nodes_arr = node["nodes"].as_array();
    let floating_arr = node["floating_nodes"].as_array();
    let children: Vec<&serde_json::Value> = nodes_arr
        .iter()
        .flat_map(|a| a.iter())
        .chain(floating_arr.iter().flat_map(|a| a.iter()))
        .collect();

    let mut visited = std::collections::HashSet::new();

    for id in &focus_ids {
        if let Some(child) = children.iter().find(|c| c["id"].as_i64() == Some(*id)) {
            if let Some(child_id) = child["id"].as_i64() {
                if visited.insert(child_id) {
                    collect_window_titles_in_focus_order(child, out);
                }
            }
        }
    }

    for child in &children {
        if let Some(child_id) = child["id"].as_i64() {
            if visited.insert(child_id) {
                collect_window_titles_in_focus_order(child, out);
            }
        }
    }
}

/// Recursively collect workspace_name → window titles (focus order) from an i3 tree.
pub fn collect_workspace_windows(node: &serde_json::Value, map: &mut HashMap<String, Vec<String>>) {
    if node["type"].as_str() == Some("workspace") {
        let name = node["name"].as_str().unwrap_or("").to_string();
        let mut titles = Vec::new();
        collect_window_titles_in_focus_order(node, &mut titles);
        if !titles.is_empty() {
            map.insert(name, titles);
        }
        return;
    }
    for child in node["nodes"]
        .as_array()
        .iter()
        .flat_map(|a| a.iter())
        .chain(
            node["floating_nodes"]
                .as_array()
                .iter()
                .flat_map(|a| a.iter()),
        )
    {
        collect_workspace_windows(child, map);
    }
}

/// Query GET_TREE and return a map of workspace name → window titles in focus order.
pub fn fetch_tree_window_titles(socket: &str) -> std::io::Result<HashMap<String, Vec<String>>> {
    let mut s = UnixStream::connect(socket)?;
    i3_send(&mut s, 4, b"")?;
    let (_, payload) = i3_recv(&mut s)?;
    let tree: serde_json::Value = serde_json::from_slice(&payload).unwrap_or_default();
    let mut map = HashMap::new();
    collect_workspace_windows(&tree, &mut map);
    Ok(map)
}

pub fn fetch_workspaces(socket: &str, output: &str) -> std::io::Result<Vec<Workspace>> {
    let mut s = UnixStream::connect(socket)?;
    i3_send(&mut s, 1, b"")?;
    let (_, payload) = i3_recv(&mut s)?;
    let arr: Vec<serde_json::Value> = serde_json::from_slice(&payload).unwrap_or_default();

    let window_titles = fetch_tree_window_titles(socket).unwrap_or_default();

    Ok(arr
        .iter()
        .filter(|w| workspace_matches_output(w["output"].as_str().unwrap_or(""), output))
        .map(|w| {
            let name = w["name"].as_str().unwrap_or("?").to_string();
            let focused_windows = window_titles.get(&name).cloned().unwrap_or_default();
            Workspace {
                name,
                focused: w["focused"].as_bool().unwrap_or(false),
                urgent: w["urgent"].as_bool().unwrap_or(false),
                focused_windows,
            }
        })
        .collect())
}

pub fn build_workspace_data(workspaces: &[Workspace]) -> serde_json::Value {
    serde_json::json!({
        "workspaces": workspaces.iter().map(|ws| serde_json::json!({
            "name": ws.name,
            "focused": ws.focused,
            "urgent": ws.urgent,
            "focused_windows": ws.focused_windows,
        })).collect::<Vec<_>>()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn workspace_matches_output_empty_filter_passes_all() {
        assert!(workspace_matches_output("DP-1", ""));
        assert!(workspace_matches_output("HDMI-A-1", ""));
        assert!(workspace_matches_output("", ""));
    }

    #[test]
    fn workspace_matches_output_nonempty_filter_accepts_exact_match() {
        assert!(workspace_matches_output("DP-1", "DP-1"));
    }

    #[test]
    fn workspace_matches_output_nonempty_filter_rejects_different_output() {
        assert!(!workspace_matches_output("HDMI-A-1", "DP-1"));
    }

    #[test]
    fn workspace_matches_output_nonempty_filter_rejects_empty_workspace_output() {
        assert!(!workspace_matches_output("", "DP-1"));
    }

    #[test]
    fn collect_titles_returns_leaf_window() {
        let node = json!({
            "id": 1, "window": 99, "name": "nvim",
            "nodes": [], "floating_nodes": [], "focus": []
        });
        let mut out = vec![];
        collect_window_titles_in_focus_order(&node, &mut out);
        assert_eq!(out, vec!["nvim"]);
    }

    #[test]
    fn collect_titles_follows_focus_order() {
        let node = json!({
            "id": 10, "window": null, "focus": [20, 30],
            "nodes": [
                { "id": 20, "window": 1, "name": "first", "nodes": [], "floating_nodes": [], "focus": [] },
                { "id": 30, "window": 2, "name": "second", "nodes": [], "floating_nodes": [], "focus": [] }
            ],
            "floating_nodes": []
        });
        let mut out = vec![];
        collect_window_titles_in_focus_order(&node, &mut out);
        assert_eq!(out, vec!["first", "second"]);
    }

    #[test]
    fn collect_titles_collects_all_windows() {
        let node = json!({
            "id": 10, "window": null, "focus": [20, 30, 40],
            "nodes": [
                { "id": 20, "window": 1, "name": "a", "nodes": [], "floating_nodes": [], "focus": [] },
                { "id": 30, "window": 2, "name": "b", "nodes": [], "floating_nodes": [], "focus": [] },
                { "id": 40, "window": 3, "name": "c", "nodes": [], "floating_nodes": [], "focus": [] }
            ],
            "floating_nodes": []
        });
        let mut out = vec![];
        collect_window_titles_in_focus_order(&node, &mut out);
        assert_eq!(out, vec!["a", "b", "c"]);
    }

    #[test]
    fn collect_workspace_windows_extracts_all_titles_in_order() {
        let tree = json!({
            "id": 1, "type": "root", "window": null, "focus": [],
            "nodes": [{
                "id": 2, "type": "output", "window": null, "focus": [3],
                "nodes": [{
                    "id": 3, "type": "workspace", "name": "1: myenv",
                    "window": null, "focus": [4, 5],
                    "nodes": [
                        { "id": 4, "window": 99, "name": "nvim", "nodes": [], "floating_nodes": [], "focus": [] },
                        { "id": 5, "window": 100, "name": "kitty", "nodes": [], "floating_nodes": [], "focus": [] }
                    ],
                    "floating_nodes": []
                }],
                "floating_nodes": []
            }],
            "floating_nodes": []
        });
        let mut map = HashMap::new();
        collect_workspace_windows(&tree, &mut map);
        let titles = map.get("1: myenv").unwrap();
        assert_eq!(titles, &vec!["nvim", "kitty"]);
    }

    #[test]
    fn build_workspace_data_includes_focused_windows_array() {
        let ws = vec![Workspace {
            name: "1".into(),
            focused: true,
            urgent: false,
            focused_windows: vec!["nvim".into(), "kitty".into(), "firefox".into()],
        }];
        let data = build_workspace_data(&ws);
        let wins = data["workspaces"][0]["focused_windows"].as_array().unwrap();
        assert_eq!(wins[0], "nvim");
        assert_eq!(wins[1], "kitty");
        assert_eq!(wins[2], "firefox");
    }

    #[test]
    fn build_workspace_data_focused_windows_empty_when_no_windows() {
        let ws = vec![Workspace {
            name: "1".into(),
            focused: false,
            urgent: false,
            focused_windows: vec![],
        }];
        let data = build_workspace_data(&ws);
        assert_eq!(
            data["workspaces"][0]["focused_windows"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
    }
}
