use costae::has_fullscreen_on_output;

fn make_window(id: u64, fullscreen_mode: u64) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "fullscreen_mode": fullscreen_mode,
        "nodes": [],
        "floating_nodes": []
    })
}

fn make_workspace(id: u64, windows: Vec<serde_json::Value>) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "type": "workspace",
        "nodes": windows,
        "floating_nodes": []
    })
}

fn make_tree(
    output_name: &str,
    focused_ws_id: u64,
    workspaces: Vec<serde_json::Value>,
) -> serde_json::Value {
    serde_json::json!({
        "nodes": [{
            "name": output_name,
            "focus": [focused_ws_id],
            "nodes": workspaces
        }]
    })
}

#[test]
fn no_fullscreen_returns_false() {
    let tree = make_tree(
        "DP-1",
        10,
        vec![make_workspace(10, vec![make_window(20, 0)])],
    );
    assert!(!has_fullscreen_on_output(&tree, "DP-1"));
}

#[test]
fn fullscreen_on_focused_workspace_returns_true() {
    let tree = make_tree(
        "DP-1",
        10,
        vec![make_workspace(10, vec![make_window(20, 1)])],
    );
    assert!(has_fullscreen_on_output(&tree, "DP-1"));
}

#[test]
fn fullscreen_on_unfocused_workspace_returns_false() {
    let tree = make_tree(
        "DP-1",
        10,
        vec![
            make_workspace(10, vec![make_window(20, 0)]),
            make_workspace(11, vec![make_window(21, 1)]),
        ],
    );
    assert!(!has_fullscreen_on_output(&tree, "DP-1"));
}

#[test]
fn fullscreen_on_different_output_returns_false() {
    let tree = serde_json::json!({
        "nodes": [
            {
                "name": "DP-1",
                "focus": [10],
                "nodes": [make_workspace(10, vec![make_window(20, 0)])]
            },
            {
                "name": "HDMI-1",
                "focus": [11],
                "nodes": [make_workspace(11, vec![make_window(21, 1)])]
            }
        ]
    });
    assert!(!has_fullscreen_on_output(&tree, "DP-1"));
    assert!(has_fullscreen_on_output(&tree, "HDMI-1"));
}

#[test]
fn nested_fullscreen_window_is_detected() {
    let inner = serde_json::json!({
        "id": 30,
        "fullscreen_mode": 1,
        "nodes": [],
        "floating_nodes": []
    });
    let outer = serde_json::json!({
        "id": 20,
        "fullscreen_mode": 0,
        "nodes": [inner],
        "floating_nodes": []
    });
    let tree = make_tree("DP-1", 10, vec![make_workspace(10, vec![outer])]);
    assert!(has_fullscreen_on_output(&tree, "DP-1"));
}

#[test]
fn empty_tree_returns_false() {
    assert!(!has_fullscreen_on_output(
        &serde_json::json!({"nodes": []}),
        "DP-1"
    ));
}

#[test]
fn real_i3_tree_structure_with_content_container() {
    // Real i3 tree: output → content_container → workspace → window
    // The output's focus points to the content container, not the workspace directly.
    let tree = serde_json::json!({
        "nodes": [{
            "name": "DP-1",
            "focus": [100],
            "nodes": [{
                "id": 100,
                "type": "con",
                "focus": [10],
                "nodes": [make_workspace(10, vec![make_window(20, 1)])]
            }]
        }]
    });
    assert!(has_fullscreen_on_output(&tree, "DP-1"));
}

#[test]
fn real_i3_tree_no_fullscreen_with_content_container() {
    let tree = serde_json::json!({
        "nodes": [{
            "name": "DP-1",
            "focus": [100],
            "nodes": [{
                "id": 100,
                "type": "con",
                "focus": [10],
                "nodes": [make_workspace(10, vec![make_window(20, 0)])]
            }]
        }]
    });
    assert!(!has_fullscreen_on_output(&tree, "DP-1"));
}
