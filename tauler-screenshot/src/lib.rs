//! Render a tauler JSX layout to a vector SVG, as a library.
//!
//! [`render`] is the whole API: JSX source in, [`Screenshot`] out. It runs the
//! same pipeline as the bar itself — evaluate the JSX, resolve theme tokens,
//! wrap the result in the padded preview canvas, render — so the output is what
//! the bar would actually draw.
//!
//! ```no_run
//! let shot = tauler_screenshot::render(
//!     "export default () => <span class=\"text-foreground\">Hello, world</span>;",
//!     &tauler_screenshot::Options::default(),
//! )?;
//! std::fs::write("hello.svg", &shot.svg)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! The font set is process-global: the first [`render`] in a process installs
//! it, and a later call with a different `font_path` reloads it. Streams are
//! not run — `useStringStream` and `useJSONStream` resolve to empty values, so
//! a layout that reads live data renders in its empty state.

use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::path::PathBuf;

use tauler::jsx::JsxEvaluator;
use tauler::theme::resolver::resolve_theme_tokens;

pub use tauler::theme::ThemeMode;

/// How to render. `Options::default()` is the dark theme at the standard
/// preview width, with fonts resolved through fontconfig.
#[derive(Debug, Clone)]
pub struct Options {
    pub theme: ThemeMode,
    /// Document width in CSS pixels, including the 16px margins.
    pub width: u32,
    /// A TTF/OTF file to use as the primary (sans-serif) font. `None` takes
    /// whatever fontconfig resolves, which varies by host — pin a file when the
    /// output has to be reproducible.
    pub font_path: Option<PathBuf>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            theme: ThemeMode::Dark,
            width: tauler::preview::WIDTH,
            font_path: None,
        }
    }
}

/// The JSX source failed to parse or evaluate.
#[derive(Debug)]
pub struct Error(String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "evaluating the layout: {}", self.0)
    }
}

impl std::error::Error for Error {}

/// One rendered layout: the SVG document, plus the two harvests the docs
/// pipeline reads from the same render (ADR 0026, ADR 0028).
pub struct Screenshot {
    /// The vector SVG document, sized to the content.
    pub svg: String,
    /// Every Tailwind utility the resolved tree carries, one entry per class.
    pub classes: BTreeSet<String>,
    /// The resolved canvas, kept so [`Self::geometry`] measures exactly what
    /// was rendered rather than a re-evaluation that could differ.
    canvas: serde_json::Value,
    width: u32,
}

/// One painted node's box, in CSS pixels, keyed by its render path.
#[derive(Debug, Clone, PartialEq)]
pub struct PaintedBox {
    /// Dot-joined child indices from the root, e.g. `"0.1"`.
    pub path: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Screenshot {
    /// Every painted node's box. A separate layout pass, run on demand because
    /// only the geometry gate wants it.
    pub fn geometry(&self) -> Vec<PaintedBox> {
        // A tall scratch height so any component fits; the boxes carry their
        // own extents, so the surplus below the content costs nothing.
        const SCRATCH_H: u32 = 2000;
        tauler::hit_test::painted_boxes(&self.canvas, self.width, SCRATCH_H, 1.0)
            .into_iter()
            .map(|(render_path, r)| PaintedBox {
                path: render_path
                    .iter()
                    .map(usize::to_string)
                    .collect::<Vec<_>>()
                    .join("."),
                x: r.x,
                y: r.y,
                width: r.width,
                height: r.height,
            })
            .collect()
    }
}

/// Render `jsx_source` — a module whose default export returns the layout.
pub fn render(jsx_source: &str, options: &Options) -> Result<Screenshot, Error> {
    let font_config = tauler::config::FontConfig {
        primary_path: options
            .font_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string()),
        ..Default::default()
    };
    // The first call in the process installs the context; on every later call
    // `init_global_ctx` is a no-op and the reload is what applies this call's
    // fonts. The reload is unconditional because there is no way to ask which
    // fonts the context currently holds.
    tauler::init_global_ctx(font_config.clone());
    tauler::reload_font_config(font_config);

    let theme = tauler::theme::Theme::default_theme();

    let eval_output = JsxEvaluator::new(jsx_source, serde_json::Value::Null, None)
        .map_err(|e| Error(e.to_string()))?
        .eval(&HashMap::new())
        .map_err(|e| Error(e.to_string()))?;

    let mut layout = eval_output.layout;
    resolve_theme_tokens(&mut layout, &theme, options.theme);

    use tauler::preview::{CANVAS_CLASS, FRAME_CLASS};
    // The frame makes every component render at the full content width, whether
    // or not it uses `w-full`. Both classes come from `tauler::preview` because
    // the browser has to build the same canvas to be photographing the same
    // thing.
    let frame = serde_json::json!({
        "type": "div",
        "class": FRAME_CLASS,
        "children": [layout]
    });
    let mut canvas = serde_json::json!({
        "type": "div",
        "class": CANVAS_CLASS,
        "children": [frame]
    });
    resolve_theme_tokens(&mut canvas, &theme, options.theme);

    // After resolution, not before: the point of harvesting is to see the
    // classes as takumi sees them, which is the same string the browser will be
    // handed.
    let mut classes = BTreeSet::new();
    collect_classes(&canvas, &mut classes);

    let svg = tauler::render_frame_svg(&canvas, options.width);
    Ok(Screenshot {
        svg,
        classes,
        canvas,
        width: options.width,
    })
}

/// Every `class` string in the tree, split into individual utilities.
fn collect_classes(value: &serde_json::Value, out: &mut BTreeSet<String>) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(class) = map.get("class").and_then(|c| c.as_str()) {
                out.extend(class.split_whitespace().map(str::to_string));
            }
            for v in map.values() {
                collect_classes(v, out);
            }
        }
        serde_json::Value::Array(items) => {
            for v in items {
                collect_classes(v, out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HELLO: &str = "export default () => <span class=\"text-foreground\">Hello, world</span>;";

    #[test]
    fn render_produces_an_svg_document_with_classes_and_geometry() {
        let shot = render(HELLO, &Options::default()).expect("hello world should render");
        assert!(
            shot.svg.starts_with("<svg"),
            "output does not begin with an <svg> element"
        );
        // Not `text-foreground`: the harvest runs after theme resolution, so the
        // token comes back as the literal color takumi renders with.
        assert!(
            shot.classes.iter().any(|c| c.starts_with("text-[")),
            "the class harvest missed the source's text class in resolved form; got {:?}",
            shot.classes
        );
        assert!(
            !shot.geometry().is_empty(),
            "nothing was painted; the geometry gate would pass vacuously"
        );
    }

    #[test]
    fn a_source_that_does_not_evaluate_is_an_error_not_a_panic() {
        assert!(render("export default squiggle(", &Options::default()).is_err());
    }
}
