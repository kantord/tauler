//! Driving a real browser over the web renderer, and comparing what it produces with what
//! takumi produced for the same component.
//!
//! Two instruments, deliberately different in strictness — see ADR 0026:
//!
//! - [`compare_geometry`] is the gate. Every box takumi painted must be within
//!   [`GEOMETRY_TOLERANCE_PX`] of the browser's box for the same render path. That is an
//!   exact claim about layout, and it fails the build.
//! - [`liveness`] is a smoke check, not a quality one. It answers "is this still
//!   recognisably the same picture", which is the one thing a threshold on cross-engine
//!   pixels can honestly answer. The fine-grained difference is reported for a person.
//!
//! What makes the pairing possible is the render path. `layout::dom` writes the same
//! child-index path into `data-tauler-path` that `hit_test::painted_boxes` keys takumi's
//! boxes by, so neither side has to guess which box is which.

use std::collections::HashMap;

use serde::Deserialize;

pub mod browser;
pub mod server;

/// How far a box may be from takumi's before the gate fails, in CSS pixels.
///
/// One pixel, and it is meant to be uncomfortable. Two engines given the same computed
/// styles either agree about where a box goes or one of them is wrong; the number is not a
/// judgement about how close is close enough, which is exactly what ADR 0005 says not to
/// buy. When this fails the answer is to understand the disagreement, not to raise it.
pub const GEOMETRY_TOLERANCE_PX: f32 = 1.0;

/// One node's box, from either side.
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

/// A box that moved, and by how much on each axis.
#[derive(Debug, Clone)]
pub struct Discrepancy {
    pub path: String,
    pub takumi: Box2D,
    pub browser: Box2D,
    pub worst_axis: &'static str,
    pub delta: f32,
}

impl std::fmt::Display for Discrepancy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "path {:?}: {} differs by {:.2}px (takumi {:.2}×{:.2} at {:.2},{:.2}; \
             browser {:.2}×{:.2} at {:.2},{:.2})",
            self.path,
            self.worst_axis,
            self.delta,
            self.takumi.width,
            self.takumi.height,
            self.takumi.x,
            self.takumi.y,
            self.browser.width,
            self.browser.height,
            self.browser.x,
            self.browser.y,
        )
    }
}

/// Pair the two sets of boxes by render path and report every one that moved.
///
/// Only paths takumi reported are checked. A browser box with no takumi counterpart is not
/// a failure: takumi gives inline content no layout node of its own, so a `<span>` inside
/// text exists in the DOM and not in the paint list — the same asymmetry ADR 0018 records.
/// The reverse *is* a failure, and it is reported as a missing node rather than silently
/// skipped, because a takumi box the browser never rendered is the loudest thing this
/// comparison can find.
pub fn compare_geometry(takumi: &[PathBox], browser: &HashMap<String, Box2D>) -> Vec<Discrepancy> {
    let mut out = Vec::new();
    for entry in takumi {
        let Some(browser_box) = browser.get(&entry.path) else {
            out.push(Discrepancy {
                path: entry.path.clone(),
                takumi: entry.bounds,
                browser: Box2D {
                    x: f32::NAN,
                    y: f32::NAN,
                    width: f32::NAN,
                    height: f32::NAN,
                },
                worst_axis: "presence",
                delta: f32::INFINITY,
            });
            continue;
        };
        let axes = [
            ("x", (entry.bounds.x - browser_box.x).abs()),
            ("y", (entry.bounds.y - browser_box.y).abs()),
            ("width", (entry.bounds.width - browser_box.width).abs()),
            ("height", (entry.bounds.height - browser_box.height).abs()),
        ];
        let (worst_axis, delta) = axes
            .into_iter()
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .expect("four axes");
        if delta > GEOMETRY_TOLERANCE_PX {
            out.push(Discrepancy {
                path: entry.path.clone(),
                takumi: entry.bounds,
                browser: *browser_box,
                worst_axis,
                delta,
            });
        }
    }
    out
}

/// The share of a render's pixels that are not its background, as a percentage.
///
/// "Background" is the top-left pixel, which in both renders is the canvas padding the
/// crop rule guarantees is there. Everything else — text, borders, fills — is ink.
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

/// Per-channel slack below which a pixel counts as background.
pub const INK_CHANNEL_TOLERANCE: u8 = 16;

/// The least ink a real render may have, as a fraction of takumi's.
///
/// Measured, not chosen. Across the seven components the browser's ink runs from **1.008**
/// to **5.612** times takumi's — the upper end is Chrome's text antialiasing spreading over
/// more pixels than tiny-skia's, not a difference in what was drawn — and a render that
/// produced nothing scores exactly **0.000**. A quarter sits in the middle of that gap with
/// four times the margin on either side, which is what makes it a number nobody has cause
/// to argue about.
///
/// There is no upper bound, deliberately. This is a liveness check: it answers whether the
/// component rendered, and nothing else. Judging *how* it rendered is the geometry gate's
/// job, and judging how it looks is a person's (ADR 0005, ADR 0026).
pub const MIN_INK_RATIO: f32 = 0.25;

/// Whether the browser rendered the component at all.
///
/// **Not** a similarity check, and the first attempt at one is worth recording because the
/// mistake is easy to repeat. Comparing the share of pixels that differ between the two
/// images cannot do this job: these components are mostly dark background with sparse text,
/// so a blank dark rectangle scores *better* against the screenshot (4.6%) than a correct
/// render does (36.9%). The metric ranked a failure above a success. Ink does not, because
/// a failure has none.
pub fn liveness(browser: &image::RgbaImage, takumi: &image::RgbaImage) -> Result<(), String> {
    if browser.dimensions() != takumi.dimensions() {
        return Err(format!(
            "sizes differ: browser {:?}, takumi {:?}",
            browser.dimensions(),
            takumi.dimensions()
        ));
    }
    let takumi_ink = ink_share(takumi, INK_CHANNEL_TOLERANCE);
    let browser_ink = ink_share(browser, INK_CHANNEL_TOLERANCE);
    if takumi_ink <= 0.0 {
        return Err("the committed screenshot is blank; there is nothing to compare".into());
    }
    let ratio = browser_ink / takumi_ink;
    if ratio < MIN_INK_RATIO {
        return Err(format!(
            "the browser drew {:.1}% ink against takumi's {:.1}% ({ratio:.3}×, under the \
             {MIN_INK_RATIO:.2}× bar) — the component did not render",
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
        let mut browser = HashMap::new();
        browser.insert("0.1".to_string(), b(10.4, 20.0, 100.0, 40.6));
        assert!(compare_geometry(&t, &browser).is_empty());
    }

    #[test]
    fn a_box_that_moved_names_the_path_and_the_axis() {
        let t = vec![takumi("0.1", b(10.0, 20.0, 100.0, 40.0))];
        let mut browser = HashMap::new();
        browser.insert("0.1".to_string(), b(10.0, 20.0, 100.0, 44.0));
        let found = compare_geometry(&t, &browser);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, "0.1");
        assert_eq!(found[0].worst_axis, "height");
        assert_eq!(found[0].delta, 4.0);
    }

    /// A takumi box the browser never rendered is the loudest finding available, so it is
    /// reported rather than skipped for want of a counterpart.
    #[test]
    fn a_node_the_browser_did_not_render_is_reported() {
        let t = vec![takumi("0.1", b(0.0, 0.0, 10.0, 10.0))];
        let found = compare_geometry(&t, &HashMap::new());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].worst_axis, "presence");
    }

    /// takumi gives inline content no layout node, so the DOM legitimately holds nodes the
    /// paint list does not — see ADR 0018.
    #[test]
    fn a_browser_node_takumi_never_painted_is_not_a_failure() {
        let t = vec![takumi("0", b(0.0, 0.0, 10.0, 10.0))];
        let mut browser = HashMap::new();
        browser.insert("0".to_string(), b(0.0, 0.0, 10.0, 10.0));
        browser.insert("0.0".to_string(), b(0.0, 0.0, 5.0, 5.0));
        assert!(compare_geometry(&t, &browser).is_empty());
    }

    fn solid(w: u32, h: u32, colour: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba(colour))
    }

    /// A render with the same ink drawn by another rasterizer passes, however different
    /// the individual pixels are.
    #[test]
    fn a_differently_rasterized_render_passes() {
        let mut takumi_img = solid(20, 20, [10, 10, 10, 255]);
        let mut browser = takumi_img.clone();
        for x in 0..20 {
            takumi_img.put_pixel(x, 10, Rgba([200, 200, 200, 255]));
            // The same stroke, one row over and a different grey: a rasterizer's choice.
            browser.put_pixel(x, 11, Rgba([170, 170, 170, 255]));
        }
        assert!(liveness(&browser, &takumi_img).is_ok());
    }

    #[test]
    fn a_blank_render_fails() {
        let mut takumi_img = solid(20, 20, [10, 10, 10, 255]);
        for x in 0..20 {
            takumi_img.put_pixel(x, 10, Rgba([200, 200, 200, 255]));
        }
        let blank = solid(20, 20, [10, 10, 10, 255]);
        let err = liveness(&blank, &takumi_img).unwrap_err();
        assert!(err.contains("did not render"), "got {err}");
    }

    /// The failure that motivated the instrument: a share-of-differing-pixels bar ranked a
    /// blank render *above* a correct one, because these components are mostly background.
    #[test]
    fn a_blank_render_is_not_rescued_by_matching_the_background() {
        let mut takumi_img = solid(100, 100, [10, 10, 10, 255]);
        for x in 0..100 {
            takumi_img.put_pixel(x, 50, Rgba([200, 200, 200, 255]));
        }
        // 99% of pixels agree with the screenshot, and it is still a failure.
        let blank = solid(100, 100, [10, 10, 10, 255]);
        assert!(liveness(&blank, &takumi_img).is_err());
    }

    #[test]
    fn a_size_mismatch_fails_before_any_pixel_is_compared() {
        let err = liveness(&solid(10, 10, [0; 4]), &solid(20, 20, [0; 4])).unwrap_err();
        assert!(err.contains("sizes differ"), "got {err}");
    }
}
