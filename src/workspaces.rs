//! `<Workspaces>`: slicing a decorative wrapper into a frame around the tiled
//! workspace area.
//!
//! A layout file designs one wrapper as if it fully surrounded the area i3 tiles real
//! windows into — rounded corners, a border, a shadow — with a `<Contents/>` placeholder
//! marking where that area is. This module measures where the placeholder actually
//! landed (by really laying the wrapper out — see [`measure_content_rect`]), slices the
//! remaining space into up to four CSS-border-style strips around it
//! ([`frame_rects`]), and turns each into a `<panel>` that re-renders the *whole*
//! wrapper, clipped and shifted so only its own strip shows ([`lay_out_frame`]).
//!
//! This lives in `src/`, not `tauler-core`, because measuring requires
//! [`crate::hit_test::painted_boxes`], which drives takumi's layout tree directly —
//! and `tauler-core` may never depend on takumi (ADR 0010, the wasm32 boundary).
//! `<I3Layout>`'s Rust half can be a `tauler-core` `#[component]` because it only does
//! arithmetic on declared sizes; this can't, because there is nothing declared to do
//! arithmetic on — the whole point is deriving the thickness from CSS nobody restated
//! as a number.

use serde_json::Value;

use crate::hit_test::{painted_boxes, Rect};

/// The attribute the `Contents` JS shim stamps on its placeholder div, so this module
/// can find it again after evaluation without needing a dedicated node type.
const CONTENT_MARKER: &str = "data-tauler-workspaces-content";

/// Up to four strips of `(0, 0, width, height)` left over around `content`, in the same
/// CSS-border-image-slice shape a picture frame uses: top and bottom span the full
/// width, left and right span only the band between them. `None` for any edge flush
/// against `content` — mirrors `i3_layout`'s "an unknown anchor reserves nothing": a
/// degenerate wrapper produces no panel, not a zero-size one.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Frame {
    pub top: Option<Rect>,
    pub right: Option<Rect>,
    pub bottom: Option<Rect>,
    pub left: Option<Rect>,
}

/// Slice `width` × `height` into a frame around `content`.
pub fn frame_rects(content: Rect, width: u32, height: u32) -> Frame {
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

    Frame {
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
pub struct WorkspacesFrame {
    pub panels: Vec<Value>,
    pub gaps: Gaps,
}

/// Build one frame panel: the *whole* `wrapper`, re-rendered at its full natural size
/// inside an `overflow-hidden` viewport sized to `rect`, shifted by `-rect.x, -rect.y`
/// so only that one strip shows through. `translate`, not `position: absolute` for
/// that shift — `docs/takumi-absolute-sibling-bug-research.md` documents an unresolved
/// takumi bug where two-or-more `position: absolute` siblings blank their whole parent
/// subtree. This is exactly `ScrollArea`'s `content_translate` trick
/// (`tauler-core/src/ui/components/scroll_area.rs`), reused because it already avoids
/// that bug family and is already proven in production layouts.
///
/// A `tauler:root-bg` image is added automatically, sized to this panel's own `rect` —
/// not to `wrapper`'s pretend full-size canvas, which is the wrong box for it (ADR
/// 0038). Every hand-written `<Panel>` that wants to look transparent adds this image
/// itself; a generated frame panel gets it for free, since the wrapper's own coordinate
/// space has no way to name "this panel's real geometry" for it to size against. It is
/// the one `position: absolute` element here, so it does not trigger the sibling bug
/// above on its own — only a *second* absolutely-positioned sibling would.
fn panel_json(id: String, rect: Rect, width: u32, height: u32, wrapper: &Value) -> Value {
    serde_json::json!({
        "type": "panel",
        "id": id,
        "x": rect.x.round() as i64,
        "y": rect.y.round() as i64,
        "width": rect.width.round() as u64,
        "height": rect.height.round() as u64,
        "children": [{
            "type": "div",
            "class": "overflow-hidden",
            "style": { "width": width, "height": height, "position": "relative" },
            "children": [
                {
                    "type": "img",
                    "src": "tauler:root-bg",
                    "style": { "position": "absolute", "top": 0, "left": 0, "width": "100%", "height": "100%" },
                },
                {
                    "type": "div",
                    "style": {
                        "width": width,
                        "height": height,
                        "translate": format!("{}px {}px", -rect.x, -rect.y),
                    },
                    "children": [wrapper.clone()],
                },
            ],
        }],
    })
}

/// Measure `wrapper` and turn whichever edges its `<Contents/>` doesn't already touch
/// into panels, positioned relative to `wrapper`'s own `(0, 0)` origin — the caller
/// (the `<I3Layout>` JS shim) knows the absolute offset this needs, this doesn't.
///
/// `id` names the emitted panels (`"{id}-top"` etc.) so more than one `<Workspaces>`
/// on screen — one per output, say — never collide.
pub fn lay_out_frame(id: &str, wrapper: &Value, width: u32, height: u32) -> WorkspacesFrame {
    let Some(content) = measure_content_rect(wrapper, width, height) else {
        return WorkspacesFrame::default();
    };
    let frame = frame_rects(content, width, height);
    let mut panels = Vec::new();
    let mut gaps = Gaps::default();

    if let Some(r) = frame.top {
        gaps.top = r.height.round() as u32;
        panels.push(panel_json(format!("{id}-top"), r, width, height, wrapper));
    }
    if let Some(r) = frame.bottom {
        gaps.bottom = r.height.round() as u32;
        panels.push(panel_json(
            format!("{id}-bottom"),
            r,
            width,
            height,
            wrapper,
        ));
    }
    if let Some(r) = frame.left {
        gaps.left = r.width.round() as u32;
        panels.push(panel_json(format!("{id}-left"), r, width, height, wrapper));
    }
    if let Some(r) = frame.right {
        gaps.right = r.width.round() as u32;
        panels.push(panel_json(format!("{id}-right"), r, width, height, wrapper));
    }

    WorkspacesFrame { panels, gaps }
}

#[cfg(test)]
mod frame_rects_tests {
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
        let frame = frame_rects(rect(10.0, 10.0, 80.0, 80.0), 100, 100);
        assert_eq!(frame.top, Some(rect(0.0, 0.0, 100.0, 10.0)));
        assert_eq!(frame.bottom, Some(rect(0.0, 90.0, 100.0, 10.0)));
        assert_eq!(frame.left, Some(rect(0.0, 10.0, 10.0, 80.0)));
        assert_eq!(frame.right, Some(rect(90.0, 10.0, 10.0, 80.0)));
    }

    #[test]
    fn content_flush_to_the_top_omits_the_top_strip() {
        let frame = frame_rects(rect(10.0, 0.0, 80.0, 90.0), 100, 100);
        assert_eq!(
            frame.top, None,
            "flush against the top edge, nothing to reserve"
        );
        assert_eq!(frame.bottom, Some(rect(0.0, 90.0, 100.0, 10.0)));
        assert_eq!(frame.left, Some(rect(0.0, 0.0, 10.0, 90.0)));
        assert_eq!(frame.right, Some(rect(90.0, 0.0, 10.0, 90.0)));
    }

    #[test]
    fn content_filling_the_wrapper_produces_no_frame_at_all() {
        let frame = frame_rects(rect(0.0, 0.0, 100.0, 100.0), 100, 100);
        assert_eq!(frame, Frame::default());
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
mod lay_out_frame_tests {
    use super::*;
    use crate::workspaces::measure_content_rect_tests::bordered_wrapper;

    #[test]
    fn emits_four_panels_with_matching_gaps() {
        crate::init_global_ctx(crate::config::FontConfig::default());
        let frame = lay_out_frame("ws", &bordered_wrapper(), 100, 100);

        assert_eq!(frame.panels.len(), 4);
        assert_eq!(
            frame.gaps,
            Gaps {
                left: 10,
                right: 10,
                top: 10,
                bottom: 10,
            }
        );

        let top = frame
            .panels
            .iter()
            .find(|p| p["id"] == "ws-top")
            .expect("a top panel");
        assert_eq!(top["x"], 0);
        assert_eq!(top["y"], 0);
        assert_eq!(top["width"], 100);
        assert_eq!(top["height"], 10);
        assert_eq!(top["children"][0]["children"][0]["src"], "tauler:root-bg");
        assert_eq!(
            top["children"][0]["children"][1]["style"]["translate"],
            "-0px -0px"
        );

        let left = frame
            .panels
            .iter()
            .find(|p| p["id"] == "ws-left")
            .expect("a left panel");
        assert_eq!(left["x"], 0);
        assert_eq!(left["y"], 10);
        assert_eq!(left["width"], 10);
        assert_eq!(left["height"], 80);
        assert_eq!(
            left["children"][0]["children"][1]["style"]["translate"],
            "-0px -10px"
        );
    }

    #[test]
    fn a_wrapper_filling_itself_produces_no_panels() {
        crate::init_global_ctx(crate::config::FontConfig::default());
        let wrapper = serde_json::json!({
            "type": "div",
            "style": { "width": 100, "height": 100 },
            "data-tauler-workspaces-content": true,
        });
        let frame = lay_out_frame("ws", &wrapper, 100, 100);
        assert_eq!(frame.panels.len(), 0);
        assert_eq!(frame.gaps, Gaps::default());
    }
}
