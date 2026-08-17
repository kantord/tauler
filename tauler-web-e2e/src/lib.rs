//! Comparing the browser's render with takumi's.
//!
//! Two instruments, deliberately different in strictness (ADR 0028):
//!
//! - [`compare_geometry`] is the gate: every box takumi painted must be within
//!   [`GEOMETRY_TOLERANCE_PX`] of the browser's box for the same render path.
//! - [`rendered_at_all`] is a liveness check, not a similarity one — the distinction is
//!   load-bearing and the reason is on the function.
//!
//! Both pair the two sides by render path: `layout::dom` writes the same child-index path
//! into `data-tauler-path` that `hit_test::painted_boxes` keys takumi's boxes by.

use std::collections::HashMap;

use serde::Deserialize;

pub mod browser;
pub mod server;

/// How far a box may be from takumi's, in CSS pixels.
///
/// Two engines given the same computed styles either agree about where a box goes or one
/// of them is wrong, so this is not a judgement about how close is close enough. When it
/// fails, understand the disagreement rather than raise the number.
pub const GEOMETRY_TOLERANCE_PX: f32 = 1.0;

#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
pub struct Box2D {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PathBox {
    pub path: String,
    #[serde(flatten)]
    pub bounds: Box2D,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
    Width,
    Height,
}

impl Axis {
    fn name(self) -> &'static str {
        match self {
            Axis::X => "x",
            Axis::Y => "y",
            Axis::Width => "width",
            Axis::Height => "height",
        }
    }
}

/// What went wrong with one node.
///
/// Two shapes, because a node the browser never rendered is a different finding from one
/// that moved — folding both into a "worst axis" string forces sentinel geometry that
/// prints as `NaN×NaN`.
#[derive(Debug, Clone)]
pub enum Discrepancy {
    Missing {
        path: String,
        takumi: Box2D,
    },
    Moved {
        path: String,
        takumi: Box2D,
        browser: Box2D,
        axis: Axis,
        delta: f32,
    },
}

impl std::fmt::Display for Discrepancy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Discrepancy::Missing { path, takumi } => write!(
                f,
                "path {path:?}: takumi painted {:.2}x{:.2} here; the browser rendered no such node",
                takumi.width, takumi.height
            ),
            Discrepancy::Moved {
                path,
                takumi,
                browser,
                axis,
                delta,
            } => write!(
                f,
                "path {path:?}: {} differs by {delta:.2}px (takumi {:.2}x{:.2} at {:.2},{:.2}; \
                 browser {:.2}x{:.2} at {:.2},{:.2})",
                axis.name(),
                takumi.width,
                takumi.height,
                takumi.x,
                takumi.y,
                browser.width,
                browser.height,
                browser.x,
                browser.y,
            ),
        }
    }
}

/// Pair the two sets of boxes by render path and report every one that moved.
///
/// Only paths takumi reported are checked. A browser box with no takumi counterpart is not
/// a failure — takumi gives inline content no layout node of its own, so a `<span>` inside
/// text exists in the DOM and not in the paint list (ADR 0018). The reverse is.
pub fn compare_geometry(takumi: &[PathBox], browser: &HashMap<String, Box2D>) -> Vec<Discrepancy> {
    let mut out = Vec::new();
    for entry in takumi {
        let Some(browser_box) = browser.get(&entry.path) else {
            out.push(Discrepancy::Missing {
                path: entry.path.clone(),
                takumi: entry.bounds,
            });
            continue;
        };
        let axes = [
            (Axis::X, (entry.bounds.x - browser_box.x).abs()),
            (Axis::Y, (entry.bounds.y - browser_box.y).abs()),
            (Axis::Width, (entry.bounds.width - browser_box.width).abs()),
            (
                Axis::Height,
                (entry.bounds.height - browser_box.height).abs(),
            ),
        ];
        let (axis, delta) = axes
            .into_iter()
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .expect("four axes");
        if delta > GEOMETRY_TOLERANCE_PX {
            out.push(Discrepancy::Moved {
                path: entry.path.clone(),
                takumi: entry.bounds,
                browser: *browser_box,
                axis,
                delta,
            });
        }
    }
    out
}

/// Per-channel slack below which two pixels count as equal.
pub const CHANNEL_TOLERANCE: u8 = 16;

/// The fraction of a render's pixels that are not its background.
///
/// "Background" is the top-left pixel, which the crop rule guarantees is canvas padding in
/// both renders. Everything else — text, borders, fills — is ink.
pub fn ink_share(image: &image::RgbaImage, channel_tolerance: u8) -> f32 {
    let total = (image.width() * image.height()) as f32;
    if total == 0.0 {
        return 0.0;
    }
    let background = image.get_pixel(0, 0).0;
    let inked = image
        .pixels()
        .filter(|p| {
            p.0.iter()
                .zip(background.iter())
                .take(3)
                .any(|(x, y)| x.abs_diff(*y) > channel_tolerance)
        })
        .count();
    inked as f32 / total
}

/// How the two renders differ, for a person to read. Reported, never gated (ADR 0028).
#[derive(Debug, Clone, Copy)]
pub struct Difference {
    /// Fraction of pixels differing by more than [`CHANNEL_TOLERANCE`].
    pub share: f32,
    /// Mean absolute per-channel difference, 0-255.
    pub mean: f32,
}

pub fn difference(a: &image::RgbaImage, b: &image::RgbaImage) -> Difference {
    if a.dimensions() != b.dimensions() || a.width() == 0 {
        return Difference {
            share: 1.0,
            mean: f32::NAN,
        };
    }
    let total = (a.width() * a.height()) as f32;
    let deltas = a.pixels().zip(b.pixels()).map(|(p, q)| {
        (0..3)
            .map(|i| p.0[i].abs_diff(q.0[i]) as u32)
            .max()
            .unwrap_or(0)
    });
    let (differing, sum) = deltas.fold((0u32, 0u32), |(n, s), d| {
        (n + u32::from(d > u32::from(CHANNEL_TOLERANCE)), s + d)
    });
    Difference {
        share: differing as f32 / total,
        mean: sum as f32 / total,
    }
}

/// The least ink a real render may have, as a fraction of takumi's.
///
/// Measured: across the seven components the browser draws 1.008-5.612x takumi's ink, and
/// a render that produced nothing draws 0.000x. A quarter sits between, with four times the
/// margin either side. No upper bound — this answers whether the component rendered, and
/// nothing else.
pub const MIN_INK_RATIO: f32 = 0.25;

/// Whether the browser rendered the component at all.
///
/// Not a similarity check, and the first attempt at one is worth recording because the
/// mistake is easy to repeat: comparing the share of differing pixels cannot do this job.
/// These components are mostly dark background with sparse text, so a blank rectangle
/// scores *better* against the screenshot (4.6%) than a correct render does (36.9%) — the
/// metric ranks the failure above the success. Ink does not, because a failure has none.
pub fn rendered_at_all(
    browser: &image::RgbaImage,
    takumi: &image::RgbaImage,
) -> Result<(), String> {
    if browser.dimensions() != takumi.dimensions() {
        return Err(format!(
            "sizes differ: browser {:?}, takumi {:?}",
            browser.dimensions(),
            takumi.dimensions()
        ));
    }
    let takumi_ink = ink_share(takumi, CHANNEL_TOLERANCE);
    let browser_ink = ink_share(browser, CHANNEL_TOLERANCE);
    if takumi_ink <= 0.0 {
        return Err("the committed screenshot is blank; there is nothing to compare".into());
    }
    let ratio = browser_ink / takumi_ink;
    if ratio < MIN_INK_RATIO {
        return Err(format!(
            "the browser drew {:.1}% ink against takumi's {:.1}% ({ratio:.3}x, under the \
             {MIN_INK_RATIO:.2}x bar) — the component did not render",
            browser_ink * 100.0,
            takumi_ink * 100.0,
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn b(x: f32, y: f32, width: f32, height: f32) -> Box2D {
        Box2D {
            x,
            y,
            width,
            height,
        }
    }

    fn takumi(path: &str, bounds: Box2D) -> PathBox {
        PathBox {
            path: path.to_string(),
            bounds,
        }
    }

    #[test]
    fn boxes_within_tolerance_are_not_reported() {
        let t = vec![takumi("0.1", b(10.0, 20.0, 100.0, 40.0))];
        let browser = HashMap::from([("0.1".to_string(), b(10.4, 20.0, 100.0, 40.6))]);
        assert!(compare_geometry(&t, &browser).is_empty());
    }

    #[test]
    fn a_box_that_moved_names_the_path_and_the_axis() {
        let t = vec![takumi("0.1", b(10.0, 20.0, 100.0, 40.0))];
        let browser = HashMap::from([("0.1".to_string(), b(10.0, 20.0, 100.0, 44.0))]);
        let found = compare_geometry(&t, &browser);
        assert!(matches!(
            found.as_slice(),
            [Discrepancy::Moved {
                axis: Axis::Height,
                delta,
                ..
            }] if *delta == 4.0
        ));
    }

    #[test]
    fn a_node_the_browser_did_not_render_is_reported_as_missing() {
        let t = vec![takumi("0.1", b(0.0, 0.0, 10.0, 10.0))];
        let found = compare_geometry(&t, &HashMap::new());
        assert!(matches!(found.as_slice(), [Discrepancy::Missing { .. }]));
        assert!(!found[0].to_string().contains("NaN"), "got {}", found[0]);
    }

    /// takumi gives inline content no layout node, so the DOM legitimately holds nodes the
    /// paint list does not (ADR 0018).
    #[test]
    fn a_browser_node_takumi_never_painted_is_not_a_failure() {
        let t = vec![takumi("0", b(0.0, 0.0, 10.0, 10.0))];
        let browser = HashMap::from([
            ("0".to_string(), b(0.0, 0.0, 10.0, 10.0)),
            ("0.0".to_string(), b(0.0, 0.0, 5.0, 5.0)),
        ]);
        assert!(compare_geometry(&t, &browser).is_empty());
    }

    fn inked(w: u32, h: u32, row: u32, colour: [u8; 4]) -> RgbaImage {
        let mut img = RgbaImage::from_pixel(w, h, Rgba([10, 10, 10, 255]));
        for x in 0..w {
            img.put_pixel(x, row, Rgba(colour));
        }
        img
    }

    /// The same ink drawn by another rasterizer passes, however different the pixels are.
    #[test]
    fn a_differently_rasterized_render_passes() {
        let takumi_img = inked(20, 20, 10, [200, 200, 200, 255]);
        let browser = inked(20, 20, 11, [170, 170, 170, 255]);
        assert!(rendered_at_all(&browser, &takumi_img).is_ok());
    }

    /// The failure that motivated the instrument: 99% of pixels agree with the screenshot
    /// and it is still a blank render.
    #[test]
    fn a_blank_render_fails_even_though_it_matches_the_background() {
        let takumi_img = inked(100, 100, 50, [200, 200, 200, 255]);
        let blank = RgbaImage::from_pixel(100, 100, Rgba([10, 10, 10, 255]));
        let err = rendered_at_all(&blank, &takumi_img).unwrap_err();
        assert!(err.contains("did not render"), "got {err}");
    }

    #[test]
    fn a_size_mismatch_fails_before_any_pixel_is_compared() {
        let a = RgbaImage::from_pixel(10, 10, Rgba([0; 4]));
        let b = RgbaImage::from_pixel(20, 20, Rgba([0; 4]));
        assert!(rendered_at_all(&a, &b)
            .unwrap_err()
            .contains("sizes differ"));
    }
}
