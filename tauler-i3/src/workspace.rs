use swayipc::{Node, NodeType};

use crate::ipc::I3Query;

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
pub fn collect_window_titles_in_focus_order(node: &Node, out: &mut Vec<String>) {
    // Leaf: actual X11/XCB window
    if node.window.is_some() {
        if let Some(title) = &node.name {
            out.push(title.clone());
        }
        return;
    }

    let children: Vec<&Node> = node
        .nodes
        .iter()
        .chain(node.floating_nodes.iter())
        .collect();

    let mut visited = std::collections::HashSet::new();

    for id in &node.focus {
        if let Some(child) = children.iter().find(|c| c.id == *id)
            && visited.insert(child.id)
        {
            collect_window_titles_in_focus_order(child, out);
        }
    }

    for child in &children {
        if visited.insert(child.id) {
            collect_window_titles_in_focus_order(child, out);
        }
    }
}

/// Query GET_TREE over the persistent i3 connection and build the workspace
/// list from the reply alone — one request covers names, focus, urgency and
/// window titles.
pub fn fetch_workspaces(query: &mut I3Query, output: &str) -> swayipc::Fallible<Vec<Workspace>> {
    let tree = query.get_tree()?;
    Ok(workspaces_from_tree(&tree, output))
}

/// Build the `Workspace` list from a GET_TREE reply alone, without a separate
/// GET_WORKSPACES query. Workspaces are `"type": "workspace"` nodes found under
/// `"type": "output"` nodes; internal (`__`-prefixed) workspaces are excluded;
/// `output_filter` follows `workspace_matches_output` semantics; the focused
/// workspace is derived from the root focus chain; window titles come in
/// most-recently-focused order.
pub fn workspaces_from_tree(tree: &Node, output_filter: &str) -> Vec<Workspace> {
    let focused_id = focused_workspace_id(tree);
    let mut out = Vec::new();
    collect_workspaces_from_tree(tree, "", output_filter, focused_id, &mut out);
    out
}

/// Follow the root focus chain (`focus[0]` at each level) down to the first
/// `"type": "workspace"` node and return its id. Returns `None` if the chain
/// is broken (missing/empty `focus`, dangling id) before reaching a workspace.
fn focused_workspace_id(tree: &Node) -> Option<i64> {
    let mut node = tree;
    loop {
        if node.node_type == NodeType::Workspace {
            return Some(node.id);
        }
        let target = *node.focus.first()?;
        node = node
            .nodes
            .iter()
            .chain(node.floating_nodes.iter())
            .find(|c| c.id == target)?;
    }
}

/// Recursive helper for `workspaces_from_tree`: walks the tree in document
/// order, remembering the nearest ancestor output name, and appends matching
/// non-internal workspace nodes to `out`.
fn collect_workspaces_from_tree(
    node: &Node,
    output_name: &str,
    output_filter: &str,
    focused_id: Option<i64>,
    out: &mut Vec<Workspace>,
) {
    if node.node_type == NodeType::Workspace {
        let name = node.name.clone().unwrap_or_default();
        if name.starts_with("__") || !workspace_matches_output(output_name, output_filter) {
            return;
        }
        let mut focused_windows = Vec::new();
        collect_window_titles_in_focus_order(node, &mut focused_windows);
        out.push(Workspace {
            name,
            focused: focused_id == Some(node.id),
            urgent: node.urgent,
            focused_windows,
        });
        return;
    }

    let output_name = if node.node_type == NodeType::Output {
        node.name.as_deref().unwrap_or("")
    } else {
        output_name
    };

    for child in node.nodes.iter().chain(node.floating_nodes.iter()) {
        collect_workspaces_from_tree(child, output_name, output_filter, focused_id, out);
    }
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

    /// Build a fully-valid `swayipc::Node` fixture, filling every field the
    /// tests don't care about with a reasonable placeholder. `Node` is
    /// `#[non_exhaustive]`, so struct-literal construction is impossible from
    /// this crate — deserializing from JSON is the only way in.
    fn node(
        id: i64,
        name: Option<&str>,
        node_type: &str,
        window: Option<i64>,
        urgent: bool,
        focus: &[i64],
        nodes: Vec<Node>,
    ) -> Node {
        // No test fixture needs non-empty floating_nodes; hardcoded below.
        let floating_nodes: Vec<Node> = vec![];
        let rect = json!({"x": 0, "y": 0, "width": 0, "height": 0});
        serde_json::from_value(json!({
            "id": id,
            "name": name,
            "type": node_type,
            "border": "normal",
            "current_border_width": 0,
            "layout": "none",
            "orientation": "none",
            "percent": null,
            "rect": rect,
            "window_rect": rect,
            "deco_rect": rect,
            "geometry": rect,
            "urgent": urgent,
            "focused": false,
            "focus": focus,
            "floating": null,
            "nodes": nodes,
            "floating_nodes": floating_nodes,
            "sticky": false,
            "representation": null,
            "fullscreen_mode": null,
            "scratchpad_state": null,
            "app_id": null,
            "pid": null,
            "window": window,
            "num": null,
            "window_properties": null,
            "marks": [],
            "inhibit_idle": null,
            "idle_inhibitors": null,
            "sandbox_engine": null,
            "sandbox_app_id": null,
            "sandbox_instance_id": null,
            "tag": null,
            "shell": null,
            "foreign_toplevel_identifier": null,
            "visible": null,
            "output": null,
        }))
        .expect("valid Node fixture")
    }

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
        let n = node(1, Some("nvim"), "con", Some(99), false, &[], vec![]);
        let mut out = vec![];
        collect_window_titles_in_focus_order(&n, &mut out);
        assert_eq!(out, vec!["nvim"]);
    }

    #[test]
    fn collect_titles_follows_focus_order() {
        let first = node(20, Some("first"), "con", Some(1), false, &[], vec![]);
        let second = node(30, Some("second"), "con", Some(2), false, &[], vec![]);
        let n = node(10, None, "con", None, false, &[20, 30], vec![first, second]);
        let mut out = vec![];
        collect_window_titles_in_focus_order(&n, &mut out);
        assert_eq!(out, vec!["first", "second"]);
    }

    #[test]
    fn collect_titles_collects_all_windows() {
        let a = node(20, Some("a"), "con", Some(1), false, &[], vec![]);
        let b = node(30, Some("b"), "con", Some(2), false, &[], vec![]);
        let c = node(40, Some("c"), "con", Some(3), false, &[], vec![]);
        let n = node(10, None, "con", None, false, &[20, 30, 40], vec![a, b, c]);
        let mut out = vec![];
        collect_window_titles_in_focus_order(&n, &mut out);
        assert_eq!(out, vec!["a", "b", "c"]);
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
    fn fetch_workspaces_with_timeout_errors_when_server_never_replies() {
        use std::os::unix::net::UnixListener;
        use std::sync::mpsc;
        use std::time::{Duration, Instant};

        let sock_path = std::env::temp_dir().join(format!(
            "tauler-i3-test-noreply-{}.sock",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&sock_path);
        let listener = UnixListener::bind(&sock_path).unwrap();

        // Server that accepts connections and reads the request, but never replies.
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { break };
                std::thread::spawn(move || {
                    let mut buf = [0u8; 64];
                    let _ = std::io::Read::read(&mut s, &mut buf);
                    // Hold the connection open without ever writing a reply.
                    std::thread::sleep(Duration::from_secs(60));
                });
            }
        });

        let socket = sock_path.to_str().unwrap().to_string();
        let (tx, rx) = mpsc::channel();
        let start = Instant::now();
        std::thread::spawn(move || {
            let mut query = I3Query::new(socket, Duration::from_millis(200));
            let res = fetch_workspaces(&mut query, "");
            let _ = tx.send(res.map(|_| ()));
        });

        // Guard: if the implementation blocks forever, fail instead of hanging.
        let result = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("fetch_workspaces hung: no result within 5s");
        let elapsed = start.elapsed();

        assert!(
            result.is_err(),
            "expected Err from a server that never replies, got Ok"
        );
        assert!(
            elapsed < Duration::from_secs(3),
            "expected bounded return, took {elapsed:?}"
        );

        let _ = std::fs::remove_file(&sock_path);
    }

    /// Minimal but realistic GET_TREE reply:
    /// root → outputs "DP-1" and "__i3".
    /// DP-1 → content con → workspaces "1: web" (globally focused via the
    /// root focus chain, two windows with kitty most recently focused) and
    /// "2: term" (urgent, no windows).
    /// __i3 → content con → workspace "__i3_scratch" (internal, must be hidden).
    fn sample_tree() -> Node {
        let firefox = node(6, Some("firefox"), "con", Some(601), false, &[], vec![]);
        let kitty = node(7, Some("kitty"), "con", Some(602), false, &[], vec![]);
        let ws_web = node(
            4,
            Some("1: web"),
            "workspace",
            None,
            false,
            &[7, 6],
            vec![firefox, kitty],
        );
        let ws_term = node(5, Some("2: term"), "workspace", None, true, &[], vec![]);
        let content_dp1 = node(
            3,
            Some("content"),
            "con",
            None,
            false,
            &[4],
            vec![ws_web, ws_term],
        );
        let output_dp1 = node(
            2,
            Some("DP-1"),
            "output",
            None,
            false,
            &[3],
            vec![content_dp1],
        );

        let ws_scratch = node(
            102,
            Some("__i3_scratch"),
            "workspace",
            None,
            false,
            &[],
            vec![],
        );
        let content_i3 = node(
            101,
            Some("content"),
            "con",
            None,
            false,
            &[102],
            vec![ws_scratch],
        );
        let output_i3 = node(
            100,
            Some("__i3"),
            "output",
            None,
            false,
            &[101],
            vec![content_i3],
        );

        node(
            1,
            Some("root"),
            "root",
            None,
            false,
            &[2],
            vec![output_dp1, output_i3],
        )
    }

    #[test]
    fn workspaces_from_tree_returns_names_in_tree_order_with_empty_filter() {
        let tree = sample_tree();
        let ws = workspaces_from_tree(&tree, "");
        let names: Vec<&str> = ws.iter().map(|w| w.name.as_str()).collect();
        assert_eq!(names, vec!["1: web", "2: term"]);
    }

    #[test]
    fn workspaces_from_tree_filters_by_output_name() {
        let tree = sample_tree();

        let matching = workspaces_from_tree(&tree, "DP-1");
        let names: Vec<&str> = matching.iter().map(|w| w.name.as_str()).collect();
        assert_eq!(names, vec!["1: web", "2: term"]);

        let non_matching = workspaces_from_tree(&tree, "HDMI-0");
        assert!(
            non_matching.is_empty(),
            "expected no workspaces for unknown output, got {:?}",
            non_matching.iter().map(|w| &w.name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn workspaces_from_tree_marks_only_focus_chain_workspace_focused() {
        let tree = sample_tree();
        let ws = workspaces_from_tree(&tree, "");
        let focused: Vec<&str> = ws
            .iter()
            .filter(|w| w.focused)
            .map(|w| w.name.as_str())
            .collect();
        assert_eq!(focused, vec!["1: web"]);
    }

    #[test]
    fn workspaces_from_tree_propagates_urgent_flag() {
        let tree = sample_tree();
        let ws = workspaces_from_tree(&tree, "");
        let urgent: Vec<(&str, bool)> = ws.iter().map(|w| (w.name.as_str(), w.urgent)).collect();
        assert_eq!(urgent, vec![("1: web", false), ("2: term", true)]);
    }

    #[test]
    fn workspaces_from_tree_collects_window_titles_in_focus_order() {
        let tree = sample_tree();
        let ws = workspaces_from_tree(&tree, "");
        let web = ws
            .iter()
            .find(|w| w.name == "1: web")
            .expect("workspace '1: web' missing");
        // Workspace focus chain is [7, 6] → kitty was focused more recently.
        assert_eq!(web.focused_windows, vec!["kitty", "firefox"]);
    }

    #[test]
    fn workspaces_from_tree_windowless_workspace_has_no_focused_windows() {
        let tree = sample_tree();
        let ws = workspaces_from_tree(&tree, "");
        let term = ws
            .iter()
            .find(|w| w.name == "2: term")
            .expect("workspace '2: term' missing");
        assert!(term.focused_windows.is_empty());
    }

    #[test]
    fn workspaces_from_tree_excludes_internal_workspaces() {
        let tree = sample_tree();

        // Internal workspaces never appear, even with a matching filter.
        let scoped = workspaces_from_tree(&tree, "__i3");
        assert!(
            scoped.is_empty(),
            "expected __i3_scratch to be hidden, got {:?}",
            scoped.iter().map(|w| &w.name).collect::<Vec<_>>()
        );

        // With an empty filter the regular workspaces are present but no
        // __-prefixed workspace leaks through.
        let all = workspaces_from_tree(&tree, "");
        assert!(
            !all.is_empty(),
            "expected non-internal workspaces to appear"
        );
        assert!(all.iter().all(|w| !w.name.starts_with("__")));
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
