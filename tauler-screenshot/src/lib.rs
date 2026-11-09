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
    /// Which shipped default to render with when `palette` is `None`. Ignored when a palette is
    /// given: a theme carries its own mode.
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
    /// The theme to resolve theme tokens against. `None` is the shipped default for `theme`.
    ///
    /// Named `palette`, not `theme`, because `theme` above is the *mode* — the two words
    /// mean different things in `CONTEXT.md` and a rename of the older field is a breaking
    /// change to a published crate.
    pub palette: Option<tauler::theme::Theme>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            theme: ThemeMode::Dark,
            width: tauler::preview::WIDTH,
            font_path: None,
            symbol_font_path: None,
            files_only: false,
            palette: None,
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

/// Render `jsx_source` — a module whose default export returns the layout, or a whole
/// `layout.op.mdx`: a leading YAML frontmatter block is split off as the bar splits it, and
/// the `theme.file` or `theme.name` it names is the theme unless `options.palette` is set.
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

    let loaded = tauler::layout_source::parse_source(jsx_source).map_err(|e| match e {
        tauler::layout_source::SourceError::UnterminatedFrontmatter => {
            Error("frontmatter opened with `---` but never closed".to_string())
        }
        tauler::layout_source::SourceError::Config(e) => Error(format!("frontmatter: {e}")),
    })?;
    let jsx_source = loaded.js_source.as_str();

    // An explicit palette wins over the frontmatter's theme, which wins over the default.
    // `theme.mode` is not read: the mode is `options.theme`, and a theme carries its own.
    let theme = match &options.palette {
        Some(palette) => palette.clone(),
        None => frontmatter_theme(&loaded.config.theme)?
            .unwrap_or_else(|| tauler::theme::builtin::default_for(options.theme)),
    };

    // `ctx.theme` is the theme the classes below are resolved against, so `themeColor` and
    // `bg-primary` paint the same palette.
    let ctx = serde_json::json!({ "theme": theme.ctx_json() });
    let eval_output = JsxEvaluator::new(jsx_source, ctx, None)
        .map_err(|e| Error(e.to_string()))?
        .eval(&HashMap::new())
        .map_err(|e| Error(e.to_string()))?;

    let mut layout = eval_output.layout;
    resolve_theme_tokens(&mut layout, &theme);

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
    resolve_theme_tokens(&mut canvas, &theme);

    // After resolution, not before: the point of harvesting is to see the
    // classes as takumi sees them, which is the same string the browser will be
    // handed.
    let mut classes = BTreeSet::new();
    collect_classes(&canvas, &mut classes);

    let svg = tauler::render_svg_document(&canvas, options.width);
    Ok(Screenshot {
        svg,
        classes,
        canvas,
        width: options.width,
    })
}

/// The theme the frontmatter's `theme.file` or `theme.name` names, if either.
fn frontmatter_theme(
    config: &tauler::config::ThemeConfig,
) -> Result<Option<tauler::theme::Theme>, Error> {
    match (&config.file, &config.name) {
        (Some(_), Some(_)) => Err(Error(
            "frontmatter: theme.name and theme.file are both set; use one".to_string(),
        )),
        (None, Some(name)) => tauler::theme::builtin::by_name(name)
            .map(Some)
            .map_err(|e| Error(format!("frontmatter: {e}"))),
        (Some(file), None) => {
            let path = tauler::config::expand_tilde(file);
            let theme_err =
                |e: &dyn fmt::Display| Error(format!("theme file {}: {e}", path.display()));
            let yaml = std::fs::read_to_string(&path).map_err(|e| theme_err(&e))?;
            tauler::theme::Theme::from_yaml(&yaml)
                .map_err(|e| theme_err(&e))?
                .validated()
                .map(Some)
                .map_err(|e| theme_err(&e))
        }
        (None, None) => Ok(None),
    }
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

    /// A complete theme file whose `foreground` is a colour no other theme uses.
    fn write_theme(dir: &std::path::Path, name: &str, foreground: &str) -> std::path::PathBuf {
        let base = tauler::theme::builtin::default_for(ThemeMode::Dark);
        let mut yaml = String::from("name: probe\nmode: dark\ncolors:\n");
        for (token, value) in &base.colors {
            let value = if token == "foreground" {
                foreground
            } else {
                value
            };
            yaml.push_str(&format!("  {token}: \"{value}\"\n"));
        }
        let path = dir.join(name);
        std::fs::write(&path, yaml).expect("write theme");
        path
    }

    const THEMED: &str = "export default () => <span class=\"text-foreground\">Hi</span>;\n";

    /// A whole `layout.op.mdx`: the frontmatter never reaches the JS engine, and the
    /// theme file it names paints the render.
    #[test]
    fn frontmatter_is_split_off_and_its_theme_file_is_honoured() {
        let dir = tempfile::tempdir().expect("tempdir");
        let theme = write_theme(dir.path(), "theme.yaml", "#12ab34");
        let source = format!("---\ntheme:\n  file: {}\n---\n{THEMED}", theme.display());
        let shot = render(&source, &Options::default()).expect("renders");
        assert!(
            shot.classes.iter().any(|c| c.contains("12ab34")),
            "{:?}",
            shot.classes
        );
    }

    #[test]
    fn a_palette_option_wins_over_the_frontmatter_theme() {
        let dir = tempfile::tempdir().expect("tempdir");
        let theme = write_theme(dir.path(), "theme.yaml", "#12ab34");
        let source = format!("---\ntheme:\n  file: {}\n---\n{THEMED}", theme.display());
        let shot = render(
            &source,
            &Options {
                palette: Some(tauler::theme::builtin::default_for(ThemeMode::Light)),
                ..Options::default()
            },
        )
        .expect("renders");
        assert!(!shot.classes.iter().any(|c| c.contains("12ab34")));
    }

    #[test]
    fn plain_jsx_renders_with_the_default_theme() {
        let shot = render(THEMED, &Options::default()).expect("renders");
        assert!(shot.svg.starts_with("<svg"));
        assert!(!shot.classes.iter().any(|c| c.contains("12ab34")));
    }

    #[test]
    fn unterminated_frontmatter_is_a_clear_error() {
        let err = render("---\ntheme: {}\n", &Options::default())
            .err()
            .expect("fails");
        assert!(err.to_string().contains("never closed"), "{err}");
    }

    #[test]
    fn invalid_frontmatter_yaml_is_a_clear_error() {
        let err = render(
            "---\ntheme: [x\n---\nexport default 1;\n",
            &Options::default(),
        )
        .err()
        .expect("fails");
        assert!(err.to_string().contains("frontmatter"), "{err}");
    }

    #[test]
    fn a_missing_theme_file_is_a_clear_error() {
        let err = render(
            "---\ntheme:\n  file: /nonexistent/theme.yaml\n---\nexport default 1;\n",
            &Options::default(),
        )
        .err()
        .expect("fails");
        assert!(err.to_string().contains("/nonexistent/theme.yaml"), "{err}");
    }
}
