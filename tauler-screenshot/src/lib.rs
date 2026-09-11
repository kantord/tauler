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
//! not run — `useStringStream` and `useJSONStream` resolve to whatever
//! [`Options::stream_values`] holds for their `(bin, script)`, and to empty
//! values otherwise, so a layout that reads live data renders in a chosen
//! state rather than a live one.
//!
//! A layout that declares a `<dom>` surface next to its `<panel>`s also comes
//! back as markup ([`Screenshot::dom`]), produced by the same walk the browser
//! runtime uses (ADR 0026) — the one file, rendered by both renderers.

use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::path::PathBuf;

use tauler::jsx::JsxEvaluator;
use tauler::theme::resolver::resolve_theme_tokens;

pub use tauler::theme::ThemeMode;

/// The screen a layout is evaluated against: what `ctx.screen_width` and
/// `ctx.screen_height` report. Panels size themselves from it, and
/// `<I3Layout>` reads it to place them, so a layout that declares surfaces
/// cannot evaluate without one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Screen {
    pub width: u32,
    pub height: u32,
}

impl Default for Screen {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
        }
    }
}

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
    /// The symbol font file for `<Icon>` glyphs. `None` asks fontconfig, which
    /// varies by host — pin the vendored file when the output has to be
    /// reproducible.
    pub symbol_font_path: Option<PathBuf>,
    /// When set, only `font_path` and `symbol_font_path` are registered and the
    /// host's fonts are never consulted, so the output is the same on every
    /// machine. `Default` is `false`.
    pub files_only: bool,
    /// The screen the layout's `ctx` describes. Default is 1920×1080.
    pub screen: Screen,
    /// The latest line of each stream, keyed by the `(bin, script)` the layout
    /// declares it with — the same identity the bar uses. A stream not listed
    /// here resolves to an empty value.
    pub stream_values: HashMap<(String, Option<String>), String>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            theme: ThemeMode::Dark,
            width: tauler::preview::WIDTH,
            font_path: None,
            symbol_font_path: None,
            files_only: false,
            screen: Screen::default(),
            stream_values: HashMap::new(),
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
    /// The layout's `<dom>` surface as markup, when it declares one: the same
    /// tree the SVG was painted from, walked the way the browser runtime walks
    /// it. `None` for a layout with no `<dom>`.
    pub dom: Option<String>,
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
        symbol_path: options.symbol_font_path.clone(),
        files_only: options.files_only,
        ..Default::default()
    };
    // The first call in the process installs the context; on every later call
    // `init_global_ctx` is a no-op and the reload is what applies this call's
    // fonts. The reload is unconditional because there is no way to ask which
    // fonts the context currently holds.
    tauler::init_global_ctx(font_config.clone());
    tauler::reload_font_config(font_config);

    let theme = tauler::theme::Theme::default_theme();

    // The same keys the bar sets (see `X11Init` in the binary), so a layout
    // that reads `ctx` sees a screen rather than `null`.
    let jsx_ctx = serde_json::json!({
        "output": "preview",
        "dpi": 96,
        "screen_width": options.screen.width,
        "screen_height": options.screen.height,
    });
    let eval_output = JsxEvaluator::new(jsx_source, jsx_ctx, None)
        .map_err(|e| Error(e.to_string()))?
        .eval(&options.stream_values)
        .map_err(|e| Error(e.to_string()))?;

    let mut layout = eval_output.layout;
    resolve_theme_tokens(&mut layout, &theme, options.theme);

    // Before the preview canvas wraps it: the walk wants the layout's own
    // `<root>` (or a bare `<dom>`), and a layout without either is simply not
    // a web surface, which is not an error here.
    let dom = match tauler::dom::render_output(&layout) {
        Ok(tauler::dom::Output::Dom { dom }) => Some(dom),
        Err(tauler::dom::DomError::NotADomSurface(_)) => None,
        Err(e) => return Err(Error(e.to_string())),
    };

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

    let svg = tauler::render_svg_document(&canvas, options.width);
    Ok(Screenshot {
        svg,
        dom,
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

    /// One file, two surfaces: the `<panel>` sizes itself from `ctx`, which
    /// used to be `null` here and threw, and the `<dom>` beside it comes back
    /// as markup carrying the stream value handed in.
    #[test]
    fn a_layout_with_a_panel_and_a_dom_renders_both_with_stream_values() {
        const BOTH: &str = r#"
function Bar({ time }) { return <span class="text-foreground">{time}</span>; }
export default function render() {
  const time = useStringStream("/bin/sh", "date");
  return (
    <root>
      <panel anchor="top" height={28} width={ctx.screen_width}><Bar time={time} /></panel>
      <dom><Bar time={time} /></dom>
    </root>
  );
}"#;
        let mut options = Options::default();
        options.stream_values.insert(
            ("/bin/sh".to_string(), Some("date".to_string())),
            "09:41".to_string(),
        );
        let shot = render(BOTH, &options).expect("a panel plus a dom should render");
        let dom = shot
            .dom
            .expect("the <dom> surface should come back as markup");
        assert!(
            dom.contains("09:41"),
            "the stream value did not reach the dom: {dom}"
        );
        assert!(shot.svg.starts_with("<svg"));
    }

    #[test]
    fn a_layout_without_a_dom_has_no_markup_and_is_not_an_error() {
        let shot = render(HELLO, &Options::default()).expect("hello world should render");
        assert!(shot.dom.is_none());
    }
}
