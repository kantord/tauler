pub mod config;
pub mod data;
pub mod display_manager;
pub mod jsx;
pub mod layout;
pub mod managed_set;
pub mod modules;
pub mod panel;
pub mod presentation;
pub mod render;
pub mod theme;
pub mod ui;
pub mod windowing;
pub mod x11;

pub use takumi::rendering::MeasuredNode;
pub use takumi::GlobalContext;

// layout
pub use layout::{parse_layout, parse_root_node, OutputInfo, PanelAnchor, PanelSpecData};

// panel — generic PanelSpec<DM>
pub use panel::PanelSpec;

// managed_set
pub use managed_set::ManagedSet;

// panel module — unified context and lifecycle
pub use panel::X11PanelContext;

// render
pub use render::{
    init_global_ctx, measure_layout_frame, preload_layout_images, reload_font_config, render_frame,
    render_frame_rgba, with_global_ctx,
};

// modules
pub use modules::hit_test;

// x11
pub use x11::{solid_color_rgba, strut_partial_values_for_anchor, x11_bgrx_to_rgba};
// data spawn functions
pub use data::{
    spawn_bi_stream, spawn_module, spawn_string_stream, SpawnedBiStream, SpawnedModule,
};

// also re-export fullscreen helpers that were in lib.rs
/// Returns true if the focused workspace on the given output has any fullscreen window.
/// `tree` is the JSON from an i3 GET_TREE (type 4) response.
///
/// The real i3 tree nests workspaces inside a content container:
///   root → output → content_container → workspace → windows
/// We follow the `focus` array at each level until we reach a workspace node.
pub fn has_fullscreen_on_output(tree: &serde_json::Value, output_name: &str) -> bool {
    let Some(outputs) = tree["nodes"].as_array() else {
        return false;
    };
    for output in outputs {
        if output["name"].as_str() != Some(output_name) {
            continue;
        }
        return focused_workspace_has_fullscreen(output);
    }
    false
}

/// Follow the focus chain from `container` down to the focused workspace,
/// then check if that workspace has any fullscreen window.
fn focused_workspace_has_fullscreen(container: &serde_json::Value) -> bool {
    if container["type"].as_str() == Some("workspace") {
        return node_has_fullscreen(container);
    }
    let focused_id = container["focus"]
        .as_array()
        .and_then(|f| f.first())
        .and_then(|id| id.as_u64());
    if let (Some(fid), Some(nodes)) = (focused_id, container["nodes"].as_array()) {
        for child in nodes {
            if child["id"].as_u64() == Some(fid) {
                return focused_workspace_has_fullscreen(child);
            }
        }
    }
    false
}

fn node_has_fullscreen(node: &serde_json::Value) -> bool {
    if node["fullscreen_mode"].as_u64().unwrap_or(0) > 0 {
        return true;
    }
    for key in &["nodes", "floating_nodes"] {
        if let Some(children) = node[key].as_array() {
            if children.iter().any(node_has_fullscreen) {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_root_node_extracts_panel_specs() {
        let root = serde_json::json!({
            "type": "root",
            "children": [{
                "type": "panel",
                "id": "sidebar",
                "anchor": "left",
                "width": 250,
                "height": 2160,
                "outer_gap": 8,
                "children": [{ "type": "container" }]
            }]
        });
        let panels = parse_root_node(&root).unwrap();
        assert_eq!(panels.len(), 1);
        assert_eq!(panels[0].id, "sidebar");
        assert_eq!(panels[0].anchor, Some(PanelAnchor::Left));
        assert_eq!(panels[0].width, 250);
        assert_eq!(panels[0].height, 2160);
        assert_eq!(panels[0].outer_gap, 8);
    }

    #[test]
    fn parse_root_node_rejects_non_root_type() {
        let node = serde_json::json!({ "type": "container" });
        assert!(parse_root_node(&node).is_err());
    }

    #[test]
    fn parse_root_node_defaults_x_y_outer_gap_to_zero() {
        let root = serde_json::json!({
            "type": "root",
            "children": [{
                "type": "panel",
                "id": "sidebar",
                "width": 250,
                "height": 2160,
                "children": []
            }]
        });
        let panels = parse_root_node(&root).unwrap();
        assert_eq!(panels[0].x, 0);
        assert_eq!(panels[0].y, 0);
        assert_eq!(panels[0].outer_gap, 0);
        assert_eq!(panels[0].anchor, None);
    }

    #[test]
    fn strut_for_anchor_left_sets_left_strut() {
        let v = strut_partial_values_for_anchor(PanelAnchor::Left, 0, 0, 1920, 2160, 365, 2160);
        assert_eq!(v[0], 365); // left strut
        assert_eq!(v[1], 0); // right strut
        assert_eq!(v[2], 0); // top strut
        assert_eq!(v[3], 0); // bottom strut
        assert_eq!(v[4], 0); // left_start_y
        assert_eq!(v[5], 2159); // left_end_y
    }

    #[test]
    fn strut_for_anchor_top_sets_top_strut() {
        let v = strut_partial_values_for_anchor(PanelAnchor::Top, 0, 0, 1920, 2160, 1920, 32);
        assert_eq!(v[0], 0);
        assert_eq!(v[2], 32); // top strut
        assert_eq!(v[8], 0); // top_start_x
        assert_eq!(v[9], 1919); // top_end_x
    }
}
