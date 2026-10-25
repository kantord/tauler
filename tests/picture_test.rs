//! Pictures end to end: store an SVG under a key, render content with an
//! `<img src="tauler:picture/<key>">`, and check the pixels under the `<img>`.
//!
//! Every render goes through [`render_frame_rgba`], so what these check is the
//! binding and the raster, not the frame cache around them — that lives with
//! the key in `src/render/cache.rs`.
//!
//! The Picture registry is process-wide and `cargo test` runs these in
//! parallel, so each test uses keys of its own, and the one retain here keeps
//! every key that is not its own.

use tauler::config::FontConfig;
use tauler::render::picture::{retain_pictures, set_fallback, set_picture, PICTURE_SRC_PREFIX};
use tauler::{init_global_ctx, render_frame_rgba};

/// CSS px; the `<img>` and the viewport are both this size.
const SIZE: u32 = 10;

const BLUE: (u8, u8, u8, u8) = (0, 0, 255, 255);
const RED: (u8, u8, u8, u8) = (255, 0, 0, 255);

fn svg(fill: &str) -> String {
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{SIZE}" height="{SIZE}"><rect width="{SIZE}" height="{SIZE}" fill="{fill}"/></svg>"#
    )
}

fn content(key: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "img",
        "src": format!("{PICTURE_SRC_PREFIX}{key}"),
        "style": { "width": SIZE, "height": SIZE }
    })
}

/// The RGBA of the centre pixel of `content(key)` rendered at `dpr`.
fn centre(key: &str, dpr: f32) -> (u8, u8, u8, u8) {
    let side = (SIZE as f32 * dpr) as u32;
    let rgba = render_frame_rgba(&content(key), side, side, dpr, None);
    let i = ((side / 2 * side + side / 2) * 4) as usize;
    (rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3])
}

#[test]
fn an_img_naming_a_picture_draws_the_picture() {
    init_global_ctx(FontConfig::default());
    set_picture("draws/blue", svg("#0000ff"));

    assert_eq!(centre("draws/blue", 1.0), BLUE, "at dpr 1");
    assert_eq!(centre("draws/blue", 2.0), BLUE, "at dpr 2");
}

#[test]
fn an_img_naming_a_key_with_no_picture_draws_nothing() {
    init_global_ctx(FontConfig::default());

    let (_, _, _, alpha) = centre("missing/never-set", 1.0);
    assert_eq!(alpha, 0, "a key with no Picture must leave the box empty");
}

#[test]
fn a_picture_replaced_under_the_same_key_draws_the_new_markup() {
    init_global_ctx(FontConfig::default());
    set_picture("replaced/k", svg("#0000ff"));
    assert_eq!(centre("replaced/k", 1.0), BLUE, "the first markup");

    set_picture("replaced/k", svg("#ff0000"));
    assert_eq!(
        centre("replaced/k", 1.0),
        RED,
        "the replaced markup, not a bitmap kept from the first"
    );
}

#[test]
fn retaining_pictures_drops_the_others() {
    init_global_ctx(FontConfig::default());
    set_picture("retain/a", svg("#0000ff"));
    set_picture("retain/b", svg("#0000ff"));
    assert_eq!(centre("retain/b", 1.0), BLUE, "precondition: b draws");

    // "Only a" among this test's keys; other tests' keys are left alone.
    retain_pictures(|k| !k.starts_with("retain/") || k == "retain/a");

    assert_eq!(centre("retain/a", 1.0), BLUE, "a is kept");
    let (_, _, _, alpha) = centre("retain/b", 1.0);
    assert_eq!(alpha, 0, "b was dropped and must draw nothing");
}

#[test]
fn a_key_with_a_fallback_draws_the_fallbacks_picture_until_its_own_arrives() {
    init_global_ctx(FontConfig::default());
    set_picture("fallback/old", svg("#0000ff"));
    set_fallback("fallback/new", "fallback/old");

    assert_eq!(
        centre("fallback/new", 1.0),
        BLUE,
        "with no Picture of its own, the key shows the fallback's"
    );

    set_picture("fallback/new", svg("#ff0000"));
    assert_eq!(
        centre("fallback/new", 1.0),
        RED,
        "once its own Picture arrives, the key shows that"
    );
}

#[test]
fn retaining_pictures_keeps_the_fallback_of_a_kept_key() {
    init_global_ctx(FontConfig::default());
    set_picture("retain-fallback/old", svg("#0000ff"));
    set_fallback("retain-fallback/new", "retain-fallback/old");

    // The new tree names only "new" among this test's keys.
    retain_pictures(|k| !k.starts_with("retain-fallback/") || k == "retain-fallback/new");

    assert_eq!(
        centre("retain-fallback/old", 1.0),
        BLUE,
        "\"new\" still shows \"old\" as its fallback, so \"old\" must survive the retain"
    );
}

/// A `draw` is the layout author's code, and nothing stops it producing markup that is
/// not an SVG document. That must cost the figure its pixels, never the bar its renderer.
#[test]
fn a_picture_whose_markup_is_not_svg_draws_nothing_and_does_not_panic() {
    init_global_ctx(FontConfig::default());

    for (i, markup) in [
        "<svg",
        "not markup at all",
        "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect width=\"x\"/></svg>",
    ]
    .into_iter()
    .enumerate()
    {
        let key = format!("broken/{i}");
        set_picture(&key, markup.to_string());
        let (_, _, _, alpha) = centre(&key, 1.0);
        assert_eq!(alpha, 0, "markup {markup:?} drew something");
    }
}
