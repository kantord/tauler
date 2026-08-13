//! Edge layout: place panels around a screen and report what they consumed.
//!
//! Declaring a bar twice — once as a `<panel>` and once as an i3 gap — is easy
//! to get wrong and impossible to notice, because a stale gap just leaves dead
//! space or lets windows slide under the bar. `<I3Layout>` derives one from the
//! other: each `<Panel>` eats from an edge of the remaining rectangle in
//! document order, and the four amounts eaten *are* the gaps.
//!
//! ```jsx
//! <I3Layout module="~/.cargo/bin/tauler-i3">
//!   <Panel id="sidebar" anchor="left" size={272}>…</Panel>
//!   <Panel id="topbar"  anchor="top"  size={26}>…</Panel>
//!   <Panel id="dock"    anchor="left" size={120}>…</Panel>
//! </I3Layout>
//! ```
//!
//! `topbar` starts to the right of `sidebar` and spans the rest of the width;
//! `dock` sits below `topbar`, because it was declared after it. Order is the
//! whole API — there is nothing else to keep in sync.
//!
//! Sizes are logical pixels, like every other length in a layout file, and are
//! passed to i3 unconverted: i3's `cmd_gaps` applies `logical_px` itself.

use serde::{Deserialize, Serialize};

use crate::ui::component;

/// One `<Panel>` as declared, before it knows where it goes.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct PanelDecl {
    pub id: String,
    /// `"left"`, `"right"`, `"top"` or `"bottom"`. An unrecognised value places
    /// the panel at the origin and reserves nothing, rather than failing the
    /// whole render for one typo.
    #[serde(default)]
    pub anchor: String,
    /// Thickness along the anchored axis, in logical pixels. The other axis
    /// always fills what is left.
    #[serde(default)]
    pub size: u32,
    #[serde(default)]
    pub output: Option<String>,
    #[serde(default)]
    pub children: Vec<serde_json::Value>,
}

/// How much each edge gave up, in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Gaps {
    pub left: u32,
    pub right: u32,
    pub top: u32,
    pub bottom: u32,
}

/// Positioned panels plus the space they consumed.
#[derive(Debug, Clone, Default, Serialize)]
pub struct EdgeLayout {
    pub panels: Vec<serde_json::Value>,
    pub gaps: Gaps,
}

/// Lay `decls` out around a `width` × `height` screen, in document order.
///
/// Split out from the component so the arithmetic is testable without a JS
/// runtime — the component itself only shuffles JSON.
pub fn lay_out(decls: &[PanelDecl], width: u32, height: u32) -> EdgeLayout {
    let mut gaps = Gaps::default();
    let mut panels = Vec::with_capacity(decls.len());

    for d in decls {
        // Whatever no earlier panel has claimed.
        let free_w = width.saturating_sub(gaps.left + gaps.right);
        let free_h = height.saturating_sub(gaps.top + gaps.bottom);
        let (x, y, w, h) = match d.anchor.as_str() {
            "left" => {
                let r = (gaps.left, gaps.top, d.size, free_h);
                gaps.left += d.size;
                r
            }
            "right" => {
                let r = (
                    width.saturating_sub(gaps.right + d.size),
                    gaps.top,
                    d.size,
                    free_h,
                );
                gaps.right += d.size;
                r
            }
            "top" => {
                let r = (gaps.left, gaps.top, free_w, d.size);
                gaps.top += d.size;
                r
            }
            "bottom" => {
                let r = (
                    gaps.left,
                    height.saturating_sub(gaps.bottom + d.size),
                    free_w,
                    d.size,
                );
                gaps.bottom += d.size;
                r
            }
            _ => (gaps.left, gaps.top, free_w, free_h),
        };

        let mut node = serde_json::json!({
            "type": "panel",
            "id": d.id,
            "x": x, "y": y, "width": w, "height": h,
            "children": d.children,
        });
        if let Some(output) = &d.output {
            node["output"] = serde_json::json!(output);
        }
        panels.push(node);
    }

    EdgeLayout { panels, gaps }
}

/// The `<I3Layout>` shim's Rust half — see `JSX_GLOBALS_JS`.
///
/// Returns data rather than nodes because the caller needs both halves: the
/// panels go into the tree, the gaps go to the module. A component may return
/// any serialisable type, not only `Node`.
///
/// # Internal
#[component("@ui/i3-layout")]
pub fn i3_layout(children: Vec<PanelDecl>, width: u32, height: u32) -> EdgeLayout {
    lay_out(&children, width, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decl(id: &str, anchor: &str, size: u32) -> PanelDecl {
        PanelDecl {
            id: id.into(),
            anchor: anchor.into(),
            size,
            output: None,
            children: vec![],
        }
    }

    fn rect(l: &EdgeLayout, id: &str) -> (u32, u32, u32, u32) {
        let p = l
            .panels
            .iter()
            .find(|p| p["id"] == id)
            .unwrap_or_else(|| panic!("no panel {id}"));
        (
            p["x"].as_u64().unwrap() as u32,
            p["y"].as_u64().unwrap() as u32,
            p["width"].as_u64().unwrap() as u32,
            p["height"].as_u64().unwrap() as u32,
        )
    }

    #[test]
    fn a_left_panel_takes_the_full_height_and_reserves_its_width() {
        let l = lay_out(&[decl("s", "left", 300)], 1920, 1080);
        assert_eq!(rect(&l, "s"), (0, 0, 300, 1080));
        assert_eq!(
            l.gaps,
            Gaps {
                left: 300,
                ..Default::default()
            }
        );
    }

    #[test]
    fn a_right_panel_sits_against_the_right_edge() {
        let l = lay_out(&[decl("c", "right", 60)], 1920, 1080);
        assert_eq!(rect(&l, "c"), (1860, 0, 60, 1080));
        assert_eq!(l.gaps.right, 60);
    }

    #[test]
    fn a_bottom_panel_sits_against_the_bottom_edge() {
        let l = lay_out(&[decl("b", "bottom", 26)], 1920, 1080);
        assert_eq!(rect(&l, "b"), (0, 1054, 1920, 26));
        assert_eq!(l.gaps.bottom, 26);
    }

    /// The point of the whole thing: a later panel only sees what is left.
    #[test]
    fn a_top_panel_starts_after_an_earlier_left_panel() {
        let l = lay_out(&[decl("s", "left", 300), decl("t", "top", 50)], 1920, 1080);
        assert_eq!(
            rect(&l, "t"),
            (300, 0, 1620, 50),
            "starts right of the sidebar"
        );
    }

    /// Declaration order decides who is on top: the top bar was declared first,
    /// so the second sidebar begins below it.
    #[test]
    fn a_second_left_panel_stacks_beside_the_first_and_below_a_top_panel() {
        let l = lay_out(
            &[
                decl("s1", "left", 300),
                decl("t", "top", 50),
                decl("s2", "left", 120),
            ],
            1920,
            1080,
        );
        assert_eq!(rect(&l, "s2"), (300, 50, 120, 1030));
        assert_eq!(
            l.gaps,
            Gaps {
                left: 420,
                right: 0,
                top: 50,
                bottom: 0
            },
            "both sidebars count toward the left gap"
        );
    }

    /// Reversing the two lines changes the result — order is the API.
    #[test]
    fn declaring_the_top_panel_first_gives_it_the_corner() {
        let top_first = lay_out(&[decl("t", "top", 50), decl("s", "left", 300)], 1920, 1080);
        assert_eq!(
            rect(&top_first, "t"),
            (0, 0, 1920, 50),
            "spans the full width"
        );
        assert_eq!(rect(&top_first, "s"), (0, 50, 300, 1030), "starts below it");
    }

    #[test]
    fn the_four_gaps_are_the_totals_consumed() {
        let l = lay_out(
            &[
                decl("l", "left", 10),
                decl("r", "right", 20),
                decl("t", "top", 30),
                decl("b", "bottom", 40),
            ],
            1000,
            1000,
        );
        assert_eq!(
            l.gaps,
            Gaps {
                left: 10,
                right: 20,
                top: 30,
                bottom: 40
            }
        );
    }

    /// A typo must not take the bar down with it.
    #[test]
    fn an_unknown_anchor_reserves_nothing() {
        let l = lay_out(&[decl("x", "middle", 300)], 1920, 1080);
        assert_eq!(l.gaps, Gaps::default());
        assert_eq!(rect(&l, "x"), (0, 0, 1920, 1080));
    }

    /// Over-declaring must clamp rather than underflow into a huge panel.
    #[test]
    fn panels_wider_than_the_screen_clamp_instead_of_wrapping() {
        let l = lay_out(
            &[decl("a", "left", 2000), decl("b", "left", 2000)],
            1920,
            1080,
        );
        assert_eq!(rect(&l, "b").2, 2000, "the declared size is honoured");
        assert_eq!(rect(&l, "b").3, 1080);
        assert_eq!(l.gaps.left, 4000, "the gap reflects what was asked for");
    }

    #[test]
    fn children_are_carried_through_to_the_emitted_panel() {
        let mut d = decl("s", "left", 300);
        d.children = vec![serde_json::json!({"type": "text", "text": "hi"})];
        let l = lay_out(&[d], 1920, 1080);
        assert_eq!(l.panels[0]["children"][0]["text"], "hi");
    }

    #[test]
    fn an_output_is_carried_through_and_omitted_when_absent() {
        let mut d = decl("s", "left", 300);
        d.output = Some("DP-4".into());
        assert_eq!(lay_out(&[d], 1920, 1080).panels[0]["output"], "DP-4");
        assert!(lay_out(&[decl("s", "left", 300)], 1920, 1080).panels[0]
            .get("output")
            .is_none());
    }
}
