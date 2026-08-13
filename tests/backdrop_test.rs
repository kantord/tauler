//! The wallpaper-behind-a-panel chain: publish a wallpaper, render a panel over
//! it, and check the panel actually shows the pixels it covers.
//!
//! Every render here goes through [`render_frame_keyed`] — the cached path the
//! pipeline uses. `render_frame_rgba` renders fresh every call, so it cannot
//! catch a stale or colliding cache entry, which is where these bugs live.
//!
//! Tests share one process-wide wallpaper registry, so each one uses its own
//! output name and its own surface ids and stays independent under `cargo test`'s
//! default parallelism.

use tauler::backdrop::{crop_for, Backdrop, ROOT_BG_KEY};
use tauler::config::FontConfig;
use tauler::layout::{SurfaceKind, SurfaceSpec};
use tauler::{init_global_ctx, render_frame_keyed};

const WALL: u32 = 100;
const PANEL: u32 = 20;

/// Left half red, right half blue, so a crop at the wrong offset is visible.
fn split_wallpaper_bgrx() -> Vec<u8> {
    (0..WALL * WALL)
        .flat_map(|i| {
            if i % WALL < WALL / 2 {
                [0, 0, 255, 0] // BGRX red
            } else {
                [255, 0, 0, 0] // BGRX blue
            }
        })
        .collect()
}

/// A uniformly green wallpaper, standing in for "the user changed their wallpaper".
fn green_wallpaper_bgrx() -> Vec<u8> {
    (0..WALL * WALL).flat_map(|_| [0, 255, 0, 0]).collect()
}

fn spec(kind: SurfaceKind, id: &str, output: &str, x: i32, w: u32, h: u32) -> SurfaceSpec {
    SurfaceSpec {
        id: id.to_string(),
        kind,
        anchor: None,
        width: w,
        height: h,
        x,
        y: 0,
        outer_gap: 0,
        output: Some(output.to_string()),
        above: false,
        content: serde_json::Value::Null,
        dpr: 1.0,
    }
}

fn panel(id: &str, output: &str, x: i32) -> SurfaceSpec {
    spec(SurfaceKind::Panel, id, output, x, PANEL, PANEL)
}

/// A panel whose whole surface is the backdrop image.
///
/// An `<image>` node, not `backgroundImage: url(root-bg)` — the image node is the
/// documented path (~5ms vs ~19ms), so that is the one worth covering.
fn backdrop_content() -> serde_json::Value {
    serde_json::json!({
        "type": "container",
        "style": { "position": "relative", "width": "100%", "height": "100%" },
        "children": [{
            "type": "image",
            "src": ROOT_BG_KEY,
            "style": {
                "position": "absolute",
                "top": 0, "left": 0,
                "width": "100%", "height": "100%"
            }
        }]
    })
}

fn publish(output: &str, pixels: Vec<u8>) {
    tauler::backdrop::publish_wallpaper(
        &spec(SurfaceKind::Wallpaper, "wall", output, 0, WALL, WALL),
        std::sync::Arc::new(pixels),
        WALL,
        WALL,
    );
}

/// Render `p` exactly as the pipeline does: crop first, then render against it.
fn render_panel(p: &SurfaceSpec) -> ((u8, u8, u8), Option<Backdrop>) {
    let backdrop = crop_for(p, (PANEL, PANEL));
    let bgrx = render_frame_keyed(&backdrop_content(), PANEL, PANEL, 1.0, backdrop.as_ref());
    // The frame is BGRX; report it as RGB so the assertions read naturally.
    ((bgrx[2], bgrx[1], bgrx[0]), backdrop)
}

const RED: (u8, u8, u8) = (255, 0, 0);
const BLUE: (u8, u8, u8) = (0, 0, 255);
const GREEN: (u8, u8, u8) = (0, 255, 0);

#[test]
fn panel_over_the_right_half_of_the_wallpaper_renders_blue() {
    init_global_ctx(FontConfig::default());
    publish("right-half", split_wallpaper_bgrx());

    // A 20x20 panel at x=60 sits entirely inside the blue half.
    let (rgb, backdrop) = render_panel(&panel("rh", "right-half", 60));
    assert!(
        backdrop.is_some(),
        "a panel over a published wallpaper must get a backdrop"
    );
    assert_eq!(rgb, BLUE, "panel at x=60 must sample the blue half");
}

#[test]
fn panel_over_the_left_half_of_the_wallpaper_renders_red() {
    init_global_ctx(FontConfig::default());
    publish("left-half", split_wallpaper_bgrx());

    let (rgb, _) = render_panel(&panel("lh", "left-half", 0));
    assert_eq!(rgb, RED, "panel at x=0 must sample the red half");
}

#[test]
fn a_wallpaper_does_not_sample_itself() {
    init_global_ctx(FontConfig::default());
    publish("self", split_wallpaper_bgrx());
    let wallpaper = spec(SurfaceKind::Wallpaper, "wall", "self", 0, WALL, WALL);
    assert!(
        crop_for(&wallpaper, (WALL, WALL)).is_none(),
        "a wallpaper has nothing behind it and must not get a backdrop"
    );
}

/// The bug this guards: `root-bg` used to be one global key that nothing ever
/// cleared, so a panel on a bare output rendered whatever slice the *previous*
/// panel installed — another output's pixels. Asserting the crop is `None` is
/// not enough; the rendered pixels are what the user sees.
#[test]
fn a_panel_on_an_output_with_no_wallpaper_renders_no_backdrop() {
    init_global_ctx(FontConfig::default());
    publish("bare-neighbour", split_wallpaper_bgrx());

    // Render a panel that *does* have a backdrop first, so a stale `root-bg`
    // would be sitting there for the next render to pick up.
    let (rgb, _) = render_panel(&panel("covered", "bare-neighbour", 0));
    assert_eq!(rgb, RED, "precondition: the covered panel renders red");

    let (rgb, backdrop) = render_panel(&panel("bare", "no-wallpaper-here", 0));
    assert!(
        backdrop.is_none(),
        "no wallpaper on that output means no backdrop"
    );
    assert_ne!(
        rgb, RED,
        "a panel with no wallpaper must not show the previous panel's crop"
    );
}

/// The bug this guards: the render cache was keyed without the crop's rect, so
/// two same-size, same-content panels over one wallpaper collided on a single
/// entry and the second was served the first one's slice.
#[test]
fn two_identical_panels_over_one_wallpaper_get_their_own_slices() {
    init_global_ctx(FontConfig::default());
    publish("two-panels", split_wallpaper_bgrx());

    // Same size, same content JSON, same output, same generation — only the
    // position differs, so the rect is the only thing keeping them apart.
    let (left, _) = render_panel(&panel("left", "two-panels", 0));
    let (right, _) = render_panel(&panel("right", "two-panels", 60));

    assert_eq!(left, RED, "the left panel sees the red half");
    assert_eq!(
        right, BLUE,
        "the right panel must see blue, not the left panel's cached red frame"
    );
}

/// The bug this guards: a panel whose own content never changes still has to
/// re-render when the wallpaper moves under it. The generation is what makes the
/// otherwise-identical render miss the cache.
#[test]
fn a_wallpaper_change_re_renders_a_panel_whose_content_is_unchanged() {
    init_global_ctx(FontConfig::default());
    publish("changing", split_wallpaper_bgrx());

    let p = panel("restful", "changing", 0);
    let (before, first) = render_panel(&p);
    assert_eq!(before, RED, "starts on the red half");

    publish("changing", green_wallpaper_bgrx());
    let (after, second) = render_panel(&p);

    assert_ne!(
        first.map(|b| b.generation),
        second.map(|b| b.generation),
        "a republished wallpaper must bump the generation"
    );
    assert_eq!(
        after, GREEN,
        "the panel must show the new wallpaper even though its content is identical"
    );
}
