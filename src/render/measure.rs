//! `measureText`: how wide a run of text will be drawn, before it is drawn.
//!
//! There is no second text engine here. A measurement is a tiny layout — the text
//! in a `white-space: pre` line, styled with the family, size, weight and style it
//! was asked for — parsed by [`crate::layout::parse_layout`] and laid out by takumi
//! against the same [`Fonts`](takumi::prelude::Fonts) a frame is drawn with. So the
//! family list resolves exactly as a Text node's does, including the fallback when
//! no family in it is registered, and the width is takumi's own, in its 1/64 px
//! layout units.
//!
//! Ascent and descent are those of the face that shapes a space in that family
//! list — the line's first face, which is what a browser's canvas reports too. They
//! are read off a second line holding an empty inline-block and a space: the block
//! sits on the baseline, and the space's run spans ascent plus descent around it.
//!
//! An uncached measurement costs about 100 µs (release, the bar's own font set), and
//! a chart asks for dozens of labels every Tick in two runtimes, so answers are kept
//! in [`Cache`], which lives in the [`RenderContext`] beside the fonts it was
//! measured with. A font reload builds a new context and with it an empty cache.
//! That is what lets every runtime call this outside the effects gate: given the
//! font set, the answer is a pure function of the arguments (ADR 0046).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use takumi::prelude::{MeasuredNode, RenderOptions, Viewport};

use super::RenderContext;

/// The font a text is measured in, as a layout file names it.
#[derive(Debug, Clone, PartialEq)]
pub struct TextFont {
    /// A CSS `font-family` list.
    pub family: String,
    /// Logical pixels.
    pub size: f64,
    pub weight: f64,
    /// `normal`, `italic` or `oblique`.
    pub style: String,
}

/// A text's extent, in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextMetrics {
    pub width: f64,
    pub ascent: f64,
    pub descent: f64,
}

impl TextMetrics {
    /// What a font set that cannot shape anything measures: nothing is drawn.
    const NOTHING: Self = Self {
        width: 0.0,
        ascent: 0.0,
        descent: 0.0,
    };
}

/// The family is not a CSS `font-family` list at all (`"a;b"`), so there is no
/// font to resolve, fallback or otherwise. Unlike a family no font answers, this is
/// a mistake in the layout file, and `measureText` throws for it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NotAFamilyList;

/// Wide enough that no line is ever squeezed: the probe is laid out at its
/// max-content width anyway, this only keeps the containing block out of the way.
const PROBE_VIEWPORT: (u32, u32) = (100_000, 1_000);

/// Everything a measurement depends on besides the font set.
#[derive(PartialEq, Eq, Hash)]
struct Key {
    text: String,
    family: String,
    size: u64,
    weight: u64,
    style: String,
}

/// Bounds what a layout measuring ever-new texts can hold on to.
const CACHE_CAPACITY: usize = 4096;

/// What has been measured with one font set. [`super::rebuild_font_context`]
/// starts a new one, so nothing measured with the old fonts survives a reload.
/// Clones share it, as they share the fonts it was measured with.
#[derive(Default, Clone)]
pub struct Cache(Arc<Mutex<HashMap<Key, Measured>>>);

type Measured = Result<TextMetrics, NotAFamilyList>;

#[cfg(test)]
thread_local! {
    static LAYOUTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// [`measure_text`] with the fonts the bar draws with. `None` before
/// [`super::init_global_ctx`] has loaded them.
pub fn measure_with_bar_fonts(text: &str, font: &TextFont) -> Option<Measured> {
    let ctx = Arc::clone(&super::GLOBAL_CTX.get()?.read().unwrap());
    Some(measure_text(&ctx, text, font))
}

/// `text` on one line in `font`, measured against `ctx`'s fonts, memoised.
pub fn measure_text(ctx: &RenderContext, text: &str, font: &TextFont) -> Measured {
    let key = Key {
        text: text.to_owned(),
        family: font.family.clone(),
        size: font.size.to_bits(),
        weight: font.weight.to_bits(),
        style: font.style.clone(),
    };
    if let Some(hit) = ctx.text_metrics.0.lock().unwrap().get(&key) {
        return *hit;
    }
    let metrics = lay_out(ctx, text, font);
    let mut cache = ctx.text_metrics.0.lock().unwrap();
    if cache.len() >= CACHE_CAPACITY {
        cache.clear();
    }
    cache.insert(key, metrics);
    metrics
}

/// The uncached measurement: one takumi layout of a two-line probe.
fn lay_out(ctx: &RenderContext, text: &str, font: &TextFont) -> Measured {
    #[cfg(test)]
    LAYOUTS.with(|n| n.set(n.get() + 1));
    let style = serde_json::json!({
        "fontFamily": font.family,
        "fontSize": font.size,
        "fontWeight": font.weight,
        "fontStyle": font.style,
        "whiteSpace": "pre",
    });
    let probe = serde_json::json!({
        "type": "div",
        "style": {"display": "flex", "flexDirection": "column", "alignItems": "flex-start"},
        "children": [
            {"type": "div", "style": style, "children": [text]},
            // An empty inline-block sits on the baseline, so its top is the
            // baseline; the space beside it is shaped in the family's first face.
            {"type": "div", "style": style, "children": [
                {"type": "span", "style": {"display": "inline-block", "width": 0, "height": 0}},
                " ",
            ]},
        ],
    });
    // Every other style value was checked by `measureText` before it got here.
    let node = crate::layout::parse_layout(&probe).map_err(|_| NotAFamilyList)?;
    let measured = takumi::measure(
        RenderOptions::builder()
            .fonts(&ctx.fonts)
            .viewport(Viewport::new((
                Some(PROBE_VIEWPORT.0),
                Some(PROBE_VIEWPORT.1),
            )))
            .node(node)
            .build(),
    );
    Ok(measured
        .ok()
        .and_then(|m| metrics_of(&m))
        .unwrap_or(TextMetrics::NOTHING))
}

/// The text line's width, and the second line's run split around its baseline mark.
fn metrics_of(probe: &MeasuredNode) -> Option<TextMetrics> {
    let [text, metrics] = probe.children.as_slice() else {
        return None;
    };
    let mark = metrics.children.first()?;
    let run = metrics.runs.first()?;
    // The mark's transform is in the probe's coordinates, the run's in its line's.
    let baseline = mark.transform[5] - metrics.transform[5];
    let ascent = f64::from(baseline - run.y);
    Some(TextMetrics {
        width: f64::from(text.width),
        ascent,
        descent: f64::from(run.height) - ascent,
    })
}

#[cfg(test)]
mod tests {
    use super::{measure_text, NotAFamilyList, RenderContext, TextFont, LAYOUTS};
    use takumi::prelude::{MeasuredNode, RenderOptions, Viewport};

    fn sans(size: f64) -> TextFont {
        TextFont {
            family: "sans-serif".into(),
            size,
            weight: 400.0,
            style: "normal".into(),
        }
    }

    /// The vendored Inter alone, so the numbers are the same on every host.
    fn inter_ctx() -> RenderContext {
        let mut ctx = RenderContext::default();
        super::super::rebuild_font_context(
            &mut ctx,
            &crate::config::FontConfig {
                primary_path: Some(
                    concat!(
                        env!("CARGO_MANIFEST_DIR"),
                        "/assets/fonts/inter/InterVariable.ttf"
                    )
                    .into(),
                ),
                files_only: true,
                ..Default::default()
            },
        );
        ctx
    }

    /// How a layout lays `content` out: parsed and measured the way a frame is.
    fn lay_out_tree(ctx: &RenderContext, content: serde_json::Value) -> MeasuredNode {
        let node = crate::layout::parse_layout(&content).unwrap();
        takumi::measure(
            RenderOptions::builder()
                .fonts(&ctx.fonts)
                .viewport(Viewport::new((Some(400), Some(100))))
                .node(node)
                .build(),
        )
        .unwrap()
    }

    #[test]
    fn the_width_is_the_laid_out_width_of_the_same_text() {
        let ctx = inter_ctx();
        let laid_out = lay_out_tree(
            &ctx,
            serde_json::json!({
                "type": "div",
                "style": {"display": "flex", "alignItems": "flex-start"},
                "children": [{
                    "type": "span",
                    "style": {"fontFamily": "sans-serif", "fontSize": 11},
                    "children": ["Requests per second"],
                }],
            }),
        );
        let expected = f64::from(laid_out.children[0].width);
        assert!(expected > 50.0, "the probe laid something out: {expected}");

        let width = measure_text(&ctx, "Requests per second", &sans(11.0))
            .unwrap()
            .width;

        assert!(
            (width - expected).abs() < 1.0 / 64.0,
            "measureText {width} vs laid out {expected}"
        );
    }

    /// The line box's own split of the run's height around the baseline: Inter's
    /// hhea ascender and descender are 1984 and 494 units of 2048.
    #[test]
    fn ascent_and_descent_are_the_faces_split_of_the_line_around_the_baseline() {
        let ctx = inter_ctx();
        let laid_out = lay_out_tree(
            &ctx,
            serde_json::json!({
                "type": "div",
                "style": {"display": "flex", "alignItems": "flex-start"},
                "children": [{
                    "type": "span",
                    "style": {"fontFamily": "sans-serif", "fontSize": 11},
                    "children": ["Hello"],
                }],
            }),
        );
        let run_height = f64::from(laid_out.children[0].runs[0].height);

        let m = measure_text(&ctx, "Hello", &sans(11.0)).unwrap();

        assert!(
            (m.ascent - 11.0 * 1984.0 / 2048.0).abs() < 1.0 / 64.0,
            "{m:?}"
        );
        assert!(
            (m.descent - 11.0 * 494.0 / 2048.0).abs() < 1.0 / 64.0,
            "{m:?}"
        );
        assert!(
            (m.ascent + m.descent - run_height).abs() < 1.0 / 64.0,
            "{m:?} vs {run_height}"
        );
    }

    /// No family in the list resolves: the answer is the face drawing falls back
    /// to, not an error, so a host without the font still lays the bar out.
    #[rstest::rstest]
    #[case::unknown("No Such Family")]
    #[case::unclosed_quote("'unclosed")]
    fn an_unresolved_family_measures_as_the_fallback_drawing_uses(#[case] family: &str) {
        let ctx = inter_ctx();
        let laid_out = lay_out_tree(
            &ctx,
            serde_json::json!({
                "type": "div",
                "style": {"display": "flex", "alignItems": "flex-start"},
                "children": [{
                    "type": "span",
                    "style": {"fontFamily": family, "fontSize": 11},
                    "children": ["Requests per second"],
                }],
            }),
        );
        let expected = f64::from(laid_out.children[0].width);
        let font = TextFont {
            family: family.into(),
            ..sans(11.0)
        };

        let m = measure_text(&ctx, "Requests per second", &font).unwrap();

        assert!(expected > 50.0, "drawing fell back to a face: {expected}");
        assert!(
            (m.width - expected).abs() < 1.0 / 64.0,
            "{m:?} vs {expected}"
        );
        assert!(m.ascent > 0.0, "{m:?}");
    }

    /// Not a family list at all: there is nothing to fall back from, so it is
    /// refused rather than measured as nothing.
    #[test]
    fn a_family_that_does_not_parse_is_refused() {
        let ctx = inter_ctx();
        let font = TextFont {
            family: "a;b".into(),
            ..sans(11.0)
        };

        assert_eq!(measure_text(&ctx, "Hello", &font), Err(NotAFamilyList));
    }

    #[test]
    fn a_repeated_call_is_answered_without_laying_out_again() {
        let ctx = inter_ctx();
        let before = LAYOUTS.with(|n| n.get());
        for _ in 0..100 {
            measure_text(&ctx, "Hello", &sans(11.0));
        }
        assert_eq!(LAYOUTS.with(|n| n.get()) - before, 1);
        measure_text(&ctx, "Hello", &sans(11.01));
        assert_eq!(
            LAYOUTS.with(|n| n.get()) - before,
            2,
            "another size is another text"
        );
    }

    #[test]
    fn a_font_reload_forgets_what_was_measured_with_the_old_fonts() {
        let mut ctx = inter_ctx();
        let with_inter = measure_text(&ctx, "Hello", &sans(11.0));
        super::super::rebuild_font_context(
            &mut ctx,
            &crate::config::FontConfig {
                files_only: true,
                ..Default::default()
            },
        );

        let with_none = measure_text(&ctx, "Hello", &sans(11.0));

        assert_ne!(with_inter, with_none);
    }
}
