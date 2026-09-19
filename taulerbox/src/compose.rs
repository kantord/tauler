//! Pure geometry for placing tauler `Panel` surfaces inside taulerbox's one
//! synthesized window.
//!
//! taulerbox has no real monitor: the whole window is treated as a single
//! [`tauler::layout::OutputInfo`]-equivalent output sitting at `(0, 0)` with
//! `dpr` 1.0. [`place`] reuses `tauler::layout::surface_origin` as the source
//! of truth for anchor placement, so a panel anchored `Left` here lands
//! exactly where it would on a real `Left`-edge output at the origin.

use tauler::layout::{surface_origin, Rect, SurfaceKind, SurfaceSpec};

/// Computes a panel's rectangle and visibility within the window bounds.
///
/// Returns `(x, y, width, height, visible)` where x/y/width/height are the
/// unclamped dimensions, and `visible` indicates whether the rectangle
/// intersects the window.
fn clamp_to_window(x: i32, y: i32, width: u32, height: u32, window: (u32, u32)) -> (i32, i32, u32, u32, bool) {
    // Compute visible based on ORIGINAL unclamped dimensions
    let visible = x < window.0 as i32
        && x + width as i32 > 0
        && y < window.1 as i32
        && y + height as i32 > 0;

    // Only clamp dimensions if the rectangle is at least partially visible
    if visible {
        // Clamp x and width to fit within [0, window.0)
        let right = x + width as i32;
        let clamped_x = x.max(0);
        let clamped_right = right.min(window.0 as i32);
        let clamped_width = (clamped_right - clamped_x).max(0) as u32;

        // Clamp y and height to fit within [0, window.1)
        let bottom = y + height as i32;
        let clamped_y = y.max(0);
        let clamped_bottom = bottom.min(window.1 as i32);
        let clamped_height = (clamped_bottom - clamped_y).max(0) as u32;

        (clamped_x, clamped_y, clamped_width, clamped_height, visible)
    } else {
        (x, y, width, height, visible)
    }
}

/// A [`SurfaceSpec`] of kind `Panel`, placed within taulerbox's window.
///
/// `rect` is window-relative, in physical pixels — the same convention
/// `tauler::layout::Rect` uses for root-screen rects, just relative to the
/// synthesized window instead of the root screen.
pub struct PlacedPanel {
    pub id: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    /// Not exercised by this cycle's test; later cycles place panels that can
    /// end up fully outside the window (e.g. an unanchored panel with a large
    /// offset) and need to be marked non-visible rather than clipped.
    pub visible: bool,
}

/// Places each `Panel` spec within `window`, treating `window` as one output
/// at origin `(0, 0)` with `dpr` 1.0.
pub fn place(specs: &[SurfaceSpec], window: (u32, u32)) -> Vec<PlacedPanel> {
    let output = Rect {
        x: 0,
        y: 0,
        width: window.0,
        height: window.1,
    };

    specs
        .iter()
        .filter(|spec| spec.kind == SurfaceKind::Panel)
        .map(|spec| {
            let phys = (
                (spec.width as f32 * spec.dpr).round() as u32,
                (spec.height as f32 * spec.dpr).round() as u32,
            );
            let (x, y) = surface_origin(spec, phys, output);
            let x = x as i32;
            let y = y as i32;
            let (x, y, width, height, visible) = clamp_to_window(x, y, phys.0, phys.1, window);
            PlacedPanel {
                id: spec.id.clone(),
                x,
                y,
                width,
                height,
                visible,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauler::layout::{PanelAnchor, SurfaceKind};

    fn panel_spec(anchor: Option<PanelAnchor>, width: u32, height: u32) -> SurfaceSpec {
        SurfaceSpec {
            id: "left-panel".into(),
            kind: SurfaceKind::Panel,
            anchor,
            width,
            height,
            x: 0,
            y: 0,
            outer_gap: 0,
            output: None,
            above: false,
            content: serde_json::Value::Null,
            dpr: 1.0,
        }
    }

    #[test]
    fn left_anchored_panel_is_flush_with_the_windows_left_edge() {
        // Ground truth: tauler::layout::surface_origin, for `Some(PanelAnchor::Left)`,
        // returns `(mon_x, mon_y)` unconditionally — see src/layout/mod.rs's
        // `surface_origin` match arm for `Some(PanelAnchor::Left) | Some(PanelAnchor::Top)`,
        // and its own test `left_and_top_sit_at_the_output_origin`, which asserts this
        // returns exactly the output's origin regardless of the panel's phys size.
        //
        // taulerbox treats the whole window as one output at (mon_x, mon_y) = (0, 0).
        // So for a Left-anchored panel, surface_origin's ground truth says x = 0 + 0 = 0,
        // y = 0 + 0 = 0, independent of the panel's width/height/dpr.
        let spec = panel_spec(Some(PanelAnchor::Left), 272, 1080);
        let window = (1920, 1080);

        let placed = place(std::slice::from_ref(&spec), window);

        assert_eq!(placed.len(), 1);
        assert_eq!(placed[0].x, 0, "left-anchored panel must be flush with x = 0");
    }

    fn unanchored_panel_spec(id: &str, x: i32, y: i32, dpr: f32) -> SurfaceSpec {
        SurfaceSpec {
            id: id.into(),
            kind: SurfaceKind::Panel,
            anchor: None,
            width: 100,
            height: 50,
            x,
            y,
            outer_gap: 0,
            output: None,
            above: false,
            content: serde_json::Value::Null,
            dpr,
        }
    }

    #[test]
    fn unanchored_panels_land_at_their_own_independently_scaled_xy() {
        // Ground truth: tauler::layout::surface_origin's `None` arm returns
        // `(mon_x + (spec.x as f32 * spec.dpr).round() as i16,
        //   mon_y + (spec.y as f32 * spec.dpr).round() as i16)`.
        // taulerbox treats the window as one output at (mon_x, mon_y) = (0, 0),
        // so an unanchored panel's placed origin is exactly (x * dpr, y * dpr).
        //
        // Panel A: x = 100, y = 50, dpr = 2.0
        //   -> x = 0 + round(100 * 2.0) = 200, y = 0 + round(50 * 2.0) = 100
        let spec_a = unanchored_panel_spec("panel-a", 100, 50, 2.0);
        // Panel B: x = 10, y = 20, dpr = 1.5
        //   -> x = 0 + round(10 * 1.5) = 15, y = 0 + round(20 * 1.5) = 30
        let spec_b = unanchored_panel_spec("panel-b", 10, 20, 1.5);
        let window = (1920, 1080);

        let placed = place(&[spec_a, spec_b], window);

        assert_eq!(placed.len(), 2);
        assert_eq!(placed[0].x, 200, "panel A's x must be x * dpr = 100 * 2.0");
        assert_eq!(placed[0].y, 100, "panel A's y must be y * dpr = 50 * 2.0");
        assert_eq!(placed[1].x, 15, "panel B's x must be x * dpr = 10 * 1.5, independent of panel A");
        assert_eq!(placed[1].y, 30, "panel B's y must be y * dpr = 20 * 1.5, independent of panel A");
    }

    #[test]
    fn panel_entirely_outside_the_window_is_not_visible() {
        // Ground truth: tauler::layout::surface_origin's `None` arm (unanchored) returns
        // `(mon_x + (spec.x as f32 * spec.dpr).round() as i16, ...)`. taulerbox treats
        // the window as one output at (mon_x, mon_y) = (0, 0), so an unanchored panel's
        // placed x is exactly x * dpr.
        //
        // Panel: x = -500, dpr = 1.0, width = 100 (from unanchored_panel_spec).
        //   -> placed x = 0 + round(-500 * 1.0) = -500
        //   -> placed width = round(100 * 1.0) = 100
        //   -> right edge of the panel rect = x + width = -500 + 100 = -400
        // -400 < 0, so the entire rect [x, x + width) = [-500, -400) lies strictly to
        // the left of the window's left edge (0). It does not intersect
        // [0, window.0) x [0, window.1) at all, so the panel must be reported
        // as not visible.
        let spec = unanchored_panel_spec("offscreen-panel", -500, 0, 1.0);
        let window = (1920, 1080);

        let placed = place(std::slice::from_ref(&spec), window);

        assert_eq!(placed.len(), 1);
        assert_eq!(placed[0].x, -500);
        assert_eq!(
            placed[0].visible, false,
            "a panel entirely outside the window bounds must be reported as not visible"
        );
    }

    fn wallpaper_spec(id: &str) -> SurfaceSpec {
        SurfaceSpec {
            id: id.into(),
            kind: SurfaceKind::Wallpaper,
            anchor: None,
            width: 1920,
            height: 1080,
            x: 0,
            y: 0,
            outer_gap: 0,
            output: None,
            above: false,
            content: serde_json::Value::Null,
            dpr: 1.0,
        }
    }

    #[test]
    fn wallpaper_specs_are_silently_skipped() {
        // taulerbox has no wallpaper concept yet; `place` only knows how to place
        // `Panel` surfaces (see its `filter(|spec| spec.kind == SurfaceKind::Panel)`).
        // A `Wallpaper` spec mixed in with a `Panel` spec must produce no
        // corresponding `PlacedPanel` — only the panel comes out the other end.
        let wallpaper = wallpaper_spec("the-wallpaper");
        let panel = panel_spec(Some(PanelAnchor::Left), 272, 1080);
        let window = (1920, 1080);

        let placed = place(&[wallpaper, panel], window);

        assert_eq!(placed.len(), 1, "the wallpaper spec must not produce a PlacedPanel");
        assert_eq!(
            placed[0].id, "left-panel",
            "the only placed surface must be the panel, not the wallpaper"
        );
    }

    #[test]
    fn unanchored_panel_extending_past_right_edge_has_width_clamped() {
        // An unanchored panel positioned such that it extends past the window's
        // right edge must have its width clamped so the reported rect fits entirely
        // inside the window bounds.
        //
        // Panel: x=1800, width=300, dpr=1.0, window width=1920
        //   -> placed x = 0 + round(1800 * 1.0) = 1800
        //   -> placed width = round(300 * 1.0) = 300
        //   -> unclamped right edge = 1800 + 300 = 2100 (past window's right edge at 1920)
        //   -> clamped width = 1920 - 1800 = 120
        //   -> result must be (1800, 0, 120, 50, true) — visible because it overlaps the window
        let spec = SurfaceSpec {
            id: "right-overflow-panel".into(),
            kind: SurfaceKind::Panel,
            anchor: None,
            width: 300,
            height: 50,
            x: 1800,
            y: 0,
            outer_gap: 0,
            output: None,
            above: false,
            content: serde_json::Value::Null,
            dpr: 1.0,
        };
        let window = (1920, 1080);

        let placed = place(std::slice::from_ref(&spec), window);

        assert_eq!(placed.len(), 1);
        assert_eq!(placed[0].x, 1800, "x must remain at the placed position");
        assert_eq!(
            placed[0].width, 120,
            "width must be clamped to fit within the window's right edge"
        );
        assert_eq!(
            placed[0].visible, true,
            "panel extending past right edge but overlapping must still be visible"
        );
    }

    #[test]
    fn unanchored_panel_extending_past_left_edge_has_x_and_width_clamped() {
        // An unanchored panel positioned with negative x (partially off the left edge)
        // must have both x and width clamped so the reported rect fits entirely inside
        // the window bounds.
        //
        // Panel: x=-50, width=100, dpr=1.0, window width=1920
        //   -> placed x = 0 + round(-50 * 1.0) = -50
        //   -> placed width = round(100 * 1.0) = 100
        //   -> unclamped rect spans [-50, 50)
        //   -> clamped rect spans [0, 50), so clamped x = 0, clamped width = 50
        //   -> result must be (0, 0, 50, 50, true) — visible because it overlaps the window
        let spec = SurfaceSpec {
            id: "left-overflow-panel".into(),
            kind: SurfaceKind::Panel,
            anchor: None,
            width: 100,
            height: 50,
            x: -50,
            y: 0,
            outer_gap: 0,
            output: None,
            above: false,
            content: serde_json::Value::Null,
            dpr: 1.0,
        };
        let window = (1920, 1080);

        let placed = place(std::slice::from_ref(&spec), window);

        assert_eq!(placed.len(), 1);
        assert_eq!(placed[0].x, 0, "x must be clamped to the window's left edge");
        assert_eq!(placed[0].width, 50, "width must be adjusted to fit the visible portion");
        assert_eq!(
            placed[0].visible, true,
            "panel extending past left edge but overlapping must still be visible"
        );
    }
}
