//! `<Workspaces>`: rendering a decorative wrapper as one panel behind the tiled
//! workspace area, and measuring how much space it reserves around that area.
//!
//! A layout file designs one wrapper as if it fully surrounded the area i3 tiles real
//! windows into — rounded corners, a border, a shadow — with a `<Contents/>` placeholder
//! marking where that area is. This module measures where the placeholder actually
//! landed (by really laying the wrapper out — see [`measure_content_rect`]), slices the
//! remaining space into up to four CSS-border-style strips around it ([`edge_strips`])
//! to compute how much of each edge i3/sway must reserve, and renders the *whole*
//! wrapper unmodified as a single `<panel>` spanning the full area ([`lay_out`]) — one
//! panel rather than one clipped-and-shifted copy per strip, since backdrop-filter
//! paints a visible seam at every internal boundary between separately-rasterized
//! panels, and `edge_strips()` plus the content rect always exactly reconstruct the
//! full area with no gaps or overlaps, so there is nothing left to slice apart.
//!
//! `<Contents/>` is a real, painted `<div>`, not literally invisible — the issue that
//! asked for this pictured "an invisible dummy component," but nothing here needs the
//! placeholder to be invisible. It marks where the tiled workspace area goes, and the
//! whole panel is stacked with `above: false` (the same rule every `<Panel>` uses to sit
//! under real windows on both X11 and Wayland — `src/x11/panel.rs`'s `StackMode::BELOW`,
//! `src/windowing/wayland/mod.rs`'s `Layer::Bottom`), so a real window always paints
//! over it where one is tiled there — but its CSS background now shows through the
//! gaps between windows, instead of leaving them showing raw wallpaper.
//!
//! This lives in `src/`, not `tauler-core`, because measuring requires
//! [`crate::hit_test::painted_boxes`], which drives takumi's layout tree directly —
//! and `tauler-core` may never depend on takumi (ADR 0010, the wasm32 boundary).
//! `<I3Layout>`'s Rust half can be a `tauler-core` `#[component]` because it only does
//! arithmetic on declared sizes; this can't, because there is nothing declared to do
//! arithmetic on — the whole point is deriving the thickness from CSS nobody restated
//! as a number.

use serde_json::Value;

use crate::backdrop::ROOT_BG_KEY;
use crate::hit_test::{painted_boxes, Rect};

/// The attribute the `Contents` JS shim stamps on its placeholder div, so this module
/// can find it again after evaluation without needing a dedicated node type.
const CONTENT_MARKER: &str = "data-tauler-workspaces-content";

/// Up to four strips of `(0, 0, width, height)` left over around `content`, in the same
/// CSS-border-image-slice shape a picture frame uses: top and bottom span the full
/// width, left and right span only the band between them. `None` for any edge flush
/// against `content` — mirrors `i3_layout`'s "an unknown anchor reserves nothing": a
/// degenerate wrapper produces no panel, not a zero-size one.
///
/// Named for what it holds, not what a "frame" is elsewhere in this codebase —
/// CONTEXT.md's **Frame** is a Render target's finished pixels, an unrelated concept
/// this would collide with under the same name.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct EdgeStrips {
    pub top: Option<Rect>,
    pub right: Option<Rect>,
    pub bottom: Option<Rect>,
    pub left: Option<Rect>,
}

/// Slice `width` × `height` into strips around `content`.
pub fn edge_strips(content: Rect, width: u32, height: u32) -> EdgeStrips {
    let (width, height) = (width as f32, height as f32);

    let top = (content.y > 0.0).then_some(Rect {
        x: 0.0,
        y: 0.0,
        width,
        height: content.y,
    });
    let bottom_y = content.y + content.height;
    let bottom = (bottom_y < height).then_some(Rect {
        x: 0.0,
        y: bottom_y,
        width,
        height: height - bottom_y,
    });
    let left = (content.x > 0.0).then_some(Rect {
        x: 0.0,
        y: content.y,
        width: content.x,
        height: content.height,
    });
    let right_x = content.x + content.width;
    let right = (right_x < width).then_some(Rect {
        x: right_x,
        y: content.y,
        width: width - right_x,
        height: content.height,
    });

    EdgeStrips {
        top,
        right,
        bottom,
        left,
    }
}

/// The child-index path to the node carrying [`CONTENT_MARKER`], or `None` if no node
/// carries it. Walks the raw JSON directly rather than going through
/// `layout::html::build_tree`'s `Binding` machinery — that exists to bind click
/// handlers, and dragging it in here for one boolean flag would be the wrong kind of
/// reuse. Safe to walk `children` unfiltered: the five tags `html::build_tree` drops
/// wholesale (`head`, `meta`, `link`, `style`, `script`) have no reason to appear in a
/// decorative frame wrapper.
fn find_marker_path(node: &Value, path: &mut Vec<usize>) -> Option<Vec<usize>> {
    if node.get(CONTENT_MARKER).and_then(Value::as_bool) == Some(true) {
        return Some(path.clone());
    }
    let children = node.get("children")?.as_array()?;
    for (i, child) in children.iter().enumerate() {
        path.push(i);
        if let Some(found) = find_marker_path(child, path) {
            return Some(found);
        }
        path.pop();
    }
    None
}

/// Where `<Contents/>` actually landed inside `wrapper`, laid out at `width` × `height`.
///
/// `None` — logged, never panicked, same rule `hit_test` uses for an unreachable
/// handler — if the marker is missing, or takumi never painted it (e.g. it sits under a
/// `display: none` ancestor). A missing measurement means no frame at all rather than a
/// guess: better an unstyled edge than a wrong one.
pub fn measure_content_rect(wrapper: &Value, width: u32, height: u32) -> Option<Rect> {
    let path = find_marker_path(wrapper, &mut Vec::new()).or_else(|| {
        tracing::warn!("<Workspaces>: no <Contents/> found in the wrapper — rendering no frame");
        None
    })?;
    painted_boxes(wrapper, width, height, 1.0)
        .into_iter()
        .find(|(p, _)| *p == path)
        .map(|(_, rect)| rect)
        .or_else(|| {
            tracing::warn!("<Workspaces>: <Contents/> was never painted — rendering no frame");
            None
        })
}

/// A generated frame panel plus the gap thickness its edge consumed.
#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Serialize)]
pub struct Gaps {
    pub left: u32,
    pub right: u32,
    pub top: u32,
    pub bottom: u32,
}

/// Positioned frame panels plus the gaps they consumed — the same shape `EdgeLayout`
/// (`tauler-core`'s `i3_layout.rs`) uses for plain `<Panel>`s, so the JS shim can merge
/// the two additively without caring which produced which.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct WorkspacesLayout {
    pub panels: Vec<Value>,
    pub gaps: Gaps,
}

/// Build the one frame panel: the *whole* `wrapper`, re-rendered unmodified at
/// `width` × `height` — no clipping, no shifting. `edge_strips()` plus the content rect
/// always exactly reconstruct `(0, 0, width, height)` with no gaps or overlaps, so there
/// is nothing left to slice apart; a single panel covering the whole wrapper is
/// equivalent to the old five-panel translate/clip scheme, without the seams
/// backdrop-filter used to paint at every internal boundary between separately
/// rasterized panels.
///
/// [`ROOT_BG_KEY`] is added automatically, sized to this panel's own full `width` ×
/// `height`. Every hand-written `<Panel>` that wants to look transparent adds this
/// image itself; a generated frame panel gets it for free.
fn panel_json(id: &str, width: u32, height: u32, wrapper: &Value) -> Value {
    serde_json::json!({
        "type": "panel",
        "id": id,
        "x": 0,
        "y": 0,
        "width": width,
        "height": height,
        "children": [{
            "type": "div",
            "class": "overflow-hidden",
            "style": { "width": width, "height": height, "position": "relative" },
            "children": [
                {
                    "type": "img",
                    "src": ROOT_BG_KEY,
                    "style": { "position": "absolute", "top": 0, "left": 0, "width": "100%", "height": "100%" },
                },
                wrapper.clone(),
            ],
        }],
    })
}

/// Measure `wrapper` and emit one panel spanning the whole `(0, 0, width, height)` area,
/// positioned relative to `wrapper`'s own origin — the caller (the `<I3Layout>` JS shim)
/// knows the absolute offset this needs, this doesn't. `Gaps` still comes from
/// `edge_strips()`, computed exactly as before — i3/sway still needs to know how much
/// space the frame reserves around the tiled area — only panel *emission* has collapsed
/// from up to five clipped strips down to one unmodified copy of `wrapper`.
///
/// `id` names the single emitted panel directly (no `"{id}-top"`/`"{id}-content"`
/// suffixes anymore) so more than one `<Workspaces>` on screen — one per output, say —
/// never collide.
pub fn lay_out(id: &str, wrapper: &Value, width: u32, height: u32) -> WorkspacesLayout {
    let Some(content) = measure_content_rect(wrapper, width, height) else {
        return WorkspacesLayout::default();
    };
    let strips = edge_strips(content, width, height);
    let mut gaps = Gaps::default();

    if let Some(r) = strips.top {
        gaps.top = r.height.round() as u32;
    }
    if let Some(r) = strips.bottom {
        gaps.bottom = r.height.round() as u32;
    }
    if let Some(r) = strips.left {
        gaps.left = r.width.round() as u32;
    }
    if let Some(r) = strips.right {
        gaps.right = r.width.round() as u32;
    }

    let panels = vec![panel_json(id, width, height, wrapper)];

    WorkspacesLayout { panels, gaps }
}

#[cfg(test)]
mod edge_strips_tests {
    use super::*;

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn symmetric_padding_produces_all_four_strips() {
        let strips = edge_strips(rect(10.0, 10.0, 80.0, 80.0), 100, 100);
        assert_eq!(strips.top, Some(rect(0.0, 0.0, 100.0, 10.0)));
        assert_eq!(strips.bottom, Some(rect(0.0, 90.0, 100.0, 10.0)));
        assert_eq!(strips.left, Some(rect(0.0, 10.0, 10.0, 80.0)));
        assert_eq!(strips.right, Some(rect(90.0, 10.0, 10.0, 80.0)));
    }

    #[test]
    fn content_flush_to_the_top_omits_the_top_strip() {
        let strips = edge_strips(rect(10.0, 0.0, 80.0, 90.0), 100, 100);
        assert_eq!(
            strips.top, None,
            "flush against the top edge, nothing to reserve"
        );
        assert_eq!(strips.bottom, Some(rect(0.0, 90.0, 100.0, 10.0)));
        assert_eq!(strips.left, Some(rect(0.0, 0.0, 10.0, 90.0)));
        assert_eq!(strips.right, Some(rect(90.0, 0.0, 10.0, 90.0)));
    }

    #[test]
    fn content_filling_the_wrapper_produces_no_strips_at_all() {
        let strips = edge_strips(rect(0.0, 0.0, 100.0, 100.0), 100, 100);
        assert_eq!(strips, EdgeStrips::default());
    }
}

#[cfg(test)]
mod measure_content_rect_tests {
    use super::*;

    /// A 100x100 wrapper with a 10px border on every side around the placeholder,
    /// built from plain flex rows/columns rather than `padding` — the point is to
    /// exercise real takumi layout, not to assume which CSS properties it accepts.
    pub(super) fn bordered_wrapper() -> Value {
        serde_json::json!({
            "type": "div",
            "class": "flex flex-col",
            "style": { "width": 100, "height": 100 },
            "children": [
                { "type": "div", "style": { "height": 10 } },
                {
                    "type": "div",
                    "class": "flex flex-row flex-1",
                    "children": [
                        { "type": "div", "style": { "width": 10 } },
                        {
                            "type": "div",
                            "class": "flex-1 h-full",
                            "data-tauler-workspaces-content": true,
                        },
                        { "type": "div", "style": { "width": 10 } },
                    ],
                },
                { "type": "div", "style": { "height": 10 } },
            ],
        })
    }

    #[test]
    fn finds_the_content_rect_inset_by_the_surrounding_border() {
        crate::init_global_ctx(crate::config::FontConfig::default());
        let rect = measure_content_rect(&bordered_wrapper(), 100, 100)
            .expect("the marker is present and painted");
        assert_eq!(
            rect,
            Rect {
                x: 10.0,
                y: 10.0,
                width: 80.0,
                height: 80.0,
            }
        );
    }

    #[test]
    fn a_wrapper_with_no_placeholder_measures_to_nothing() {
        crate::init_global_ctx(crate::config::FontConfig::default());
        let wrapper =
            serde_json::json!({ "type": "div", "style": { "width": 100, "height": 100 } });
        assert_eq!(measure_content_rect(&wrapper, 100, 100), None);
    }
}

#[cfg(test)]
mod lay_out_tests {
    use super::*;
    use crate::workspaces::measure_content_rect_tests::bordered_wrapper;

    /// Recursively searches `value` for any object carrying a `"translate"` key,
    /// anywhere in the tree. The single-panel scheme has nothing left to shift — the
    /// whole wrapper is embedded unmodified — so a passing panel must have none at all,
    /// unlike the old translate-per-strip trick this replaces.
    fn contains_translate(value: &Value) -> bool {
        match value {
            Value::Object(map) => {
                map.contains_key("translate") || map.values().any(contains_translate)
            }
            Value::Array(items) => items.iter().any(contains_translate),
            _ => false,
        }
    }

    /// Recursively searches `value` for an `<img>` node whose `src` is `ROOT_BG_KEY` —
    /// the `root-bg` backdrop image `panel_json` adds behind every panel.
    fn contains_root_bg_img(value: &Value) -> bool {
        match value {
            Value::Object(map) => {
                (map.get("type") == Some(&Value::from("img"))
                    && map.get("src") == Some(&Value::from(ROOT_BG_KEY)))
                    || map.values().any(contains_root_bg_img)
            }
            Value::Array(items) => items.iter().any(contains_root_bg_img),
            _ => false,
        }
    }

    #[test]
    fn emits_a_single_panel_spanning_the_whole_wrapper() {
        crate::init_global_ctx(crate::config::FontConfig::default());
        let layout = lay_out("ws", &bordered_wrapper(), 100, 100);

        assert_eq!(
            layout.panels.len(),
            1,
            "the five-panel translate/clip scheme collapses to one panel covering the \
             whole wrapper, since backdrop-filter seams appear at every internal \
             boundary between separately-rasterized panels"
        );

        let panel = &layout.panels[0];
        assert_eq!(
            panel["id"], "ws",
            "the bare id, not a per-strip suffix like \"ws-top\" or \"ws-content\" — \
             there's only one panel now"
        );
        assert_eq!(panel["x"], 0);
        assert_eq!(panel["y"], 0);
        assert_eq!(panel["width"], 100);
        assert_eq!(panel["height"], 100);
    }

    #[test]
    fn the_single_panel_gaps_are_unchanged_from_the_five_panel_scheme() {
        crate::init_global_ctx(crate::config::FontConfig::default());
        let layout = lay_out("ws", &bordered_wrapper(), 100, 100);

        assert_eq!(
            layout.gaps,
            Gaps {
                left: 10,
                right: 10,
                top: 10,
                bottom: 10,
            },
            "Gaps still comes from edge_strips(), unaffected by collapsing panel emission"
        );
    }

    #[test]
    fn the_single_panel_has_the_root_bg_image_and_no_translate() {
        crate::init_global_ctx(crate::config::FontConfig::default());
        let layout = lay_out("ws", &bordered_wrapper(), 100, 100);
        let panel = &layout.panels[0];

        assert!(
            contains_root_bg_img(panel),
            "still needs the root-bg backdrop image behind it, same ROOT_BG_KEY \
             mechanism as before, just sized to the panel's own full width/height"
        );
        assert!(
            !contains_translate(panel),
            "nothing is sliced or shifted anymore — the wrapper is embedded as-is, so \
             there is no clip box and no translate style left to assert on"
        );
    }

    #[test]
    fn a_wrapper_filling_itself_still_produces_exactly_one_full_size_panel() {
        crate::init_global_ctx(crate::config::FontConfig::default());
        let wrapper = serde_json::json!({
            "type": "div",
            "style": { "width": 100, "height": 100 },
            "data-tauler-workspaces-content": true,
        });
        let layout = lay_out("ws", &wrapper, 100, 100);

        assert_eq!(
            layout
                .panels
                .iter()
                .map(|p| p["id"].clone())
                .collect::<Vec<_>>(),
            vec![Value::from("ws")],
            "no edges to reserve, but the wrapper still gets its one panel"
        );
        let panel = &layout.panels[0];
        assert_eq!(panel["x"], 0);
        assert_eq!(panel["y"], 0);
        assert_eq!(panel["width"], 100);
        assert_eq!(panel["height"], 100);
        assert_eq!(layout.gaps, Gaps::default());
    }
}
