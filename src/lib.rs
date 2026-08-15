pub mod backdrop;
pub mod config;
pub mod data;
pub mod display_manager;
pub mod hit_test;
pub mod jsx;
pub mod layout;
pub mod managed_set;
pub mod pointer;
pub mod presentation;
pub mod render;
pub mod surface;
pub mod theme;
pub mod ui;
pub mod windowing;
pub mod x11;

pub use render::RenderContext;

// layout
pub use layout::{
    parse_layout, parse_root_node, OutputInfo, PanelAnchor, SurfaceKind, SurfaceSpec,
};

// surface — the reconciled <panel> / <wallpaper> lifecycle
pub use surface::Surface;

// managed_set
pub use managed_set::OptativeSet;

// x11 display backend, re-exported for the presenter thread
pub use surface::X11PanelContext;

// render
pub use render::{
    init_global_ctx, measure_layout_frame, preload_layout_images, reload_font_config, render_frame,
    render_frame_keyed, render_frame_rgba, with_global_ctx, with_global_ctx_mut,
};

// hit_test
pub use hit_test::{hit_test, Hit};

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
                "children": [{ "type": "div" }]
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
    fn parse_root_node_extracts_wallpaper_specs() {
        use layout::SurfaceKind;
        let root = serde_json::json!({
            "type": "root",
            "children": [{
                "type": "wallpaper",
                "id": "bg",
                "output": "DP-2",
                "children": [{ "type": "div" }]
            }]
        });
        let specs = parse_root_node(&root).unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].id, "bg");
        assert_eq!(
            specs[0].kind,
            SurfaceKind::Wallpaper,
            "a <wallpaper> child must parse as SurfaceKind::Wallpaper"
        );
        assert_eq!(specs[0].output.as_deref(), Some("DP-2"));
    }

    #[test]
    fn parse_root_node_wallpaper_does_not_require_width_and_height() {
        // Wallpaper dimensions are always the display's — the layout file must
        // not have to state them.
        let root = serde_json::json!({
            "type": "root",
            "children": [{ "type": "wallpaper", "id": "bg", "children": [] }]
        });
        assert!(
            parse_root_node(&root).is_ok(),
            "wallpaper without width/height must parse"
        );
    }

    #[test]
    fn parse_root_node_marks_panels_as_panel_kind() {
        use layout::SurfaceKind;
        let root = serde_json::json!({
            "type": "root",
            "children": [{ "type": "panel", "id": "bar", "width": 10, "height": 10 }]
        });
        let specs = parse_root_node(&root).unwrap();
        assert_eq!(specs[0].kind, SurfaceKind::Panel);
    }

    /// `<surface type="wallpaper">` reaches the parser already flattened to
    /// `{type: "wallpaper"}`, so both spellings must land on the same spec.
    #[test]
    fn parse_root_node_accepts_the_surface_long_hand() {
        use layout::SurfaceKind;
        let root = serde_json::json!({
            "type": "root",
            "children": [
                { "type": "wallpaper", "id": "bg" },
                { "type": "panel", "id": "bar", "width": 10, "height": 10 },
            ]
        });
        let specs = parse_root_node(&root).unwrap();
        assert_eq!(specs[0].kind, SurfaceKind::Wallpaper);
        assert_eq!(specs[1].kind, SurfaceKind::Panel);
    }

    /// A bare `<surface>` names no kind. Silently dropping it would leave the
    /// user staring at a missing bar with nothing in the log.
    #[test]
    fn parse_root_node_rejects_surface_without_a_type() {
        let root = serde_json::json!({
            "type": "root",
            "children": [{ "type": "surface", "id": "bg" }]
        });
        let err = parse_root_node(&root).expect_err("bare <surface> must be an error");
        assert!(
            err.contains("panel") && err.contains("wallpaper"),
            "the error must name the valid types; got {err:?}"
        );
    }

    #[test]
    fn parse_root_node_rejects_non_root_type() {
        let node = serde_json::json!({ "type": "div" });
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
}
