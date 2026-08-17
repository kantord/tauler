//! Generating everything the documentation site needs to run a component in a browser.
//!
//! Four kinds of artifact, all derived rather than written by hand:
//!
//! - **`ui/*.js`** — the ES modules `@ui/card` and friends resolve to, from
//!   `tauler_core::ui::registry::web_module_sources`. An import map points at them, so a
//!   layout file's imports work unaltered and the printed example is the one that runs.
//! - **`examples/*.js`** — each `# JSX` block, wrapped in the same padded canvas
//!   `tauler-screenshot` wraps it in, then transformed by the same `optative-script` the
//!   desktop uses. Transforming here rather than in the page is ADR 0025: every example is
//!   known at build time, so the browser never needs a JSX transformer.
//! - **`classes.txt`** — every Tailwind utility the resolved trees carry, for Tailwind to
//!   compile. Harvested rather than scanned, because `theme/resolver.rs` has already
//!   rewritten `bg-background` into `bg-[#hex]` by the time anything renders.
//! - **`tailwind.css`** — the stylesheet input: the scoped reset, the fonts, and the
//!   `@source` line pointing at `classes.txt`.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::Path;

use crate::Component;

/// The canvas `tauler-screenshot` renders into, as JSX.
///
/// Identical to the JSON wrapper in `tauler-screenshot/src/main.rs`, and it has to stay
/// that way: the crop the browser reproduces is measured from the frame's first child, so
/// a different wrapper here would compare two different pictures (ADR 0026).
const CANVAS_CLASS: &str = "bg-background w-full flex flex-col p-[16px]";
const FRAME_CLASS: &str = "w-full flex flex-col";

/// The width `tauler-screenshot` renders at, and therefore the width the embed is given.
pub const EMBED_WIDTH: u32 = 400;

/// A `# JSX` block as the body of a `render` function.
///
/// Two shapes, and which one a block is is decided by whether it says `return`. Most
/// examples are a bare JSX expression and are wrapped in one; an example that has to show
/// *where its value comes from* needs statements first, and a control's example does —
/// ADR 0012 says a control never holds its own value, so an example that did not name a
/// source would be documenting something the system does not do.
pub fn render_body(jsx_block: &[String]) -> String {
    let body = jsx_block.join("\n");
    if has_return_statement(jsx_block) {
        body
    } else {
        format!("  return (\n{body}\n  );")
    }
}

fn has_return_statement(jsx_block: &[String]) -> bool {
    jsx_block
        .iter()
        .any(|line| line.trim_start().starts_with("return "))
}

/// Wrap an example in a `<dom>` surface and the screenshot canvas.
///
/// The `# JSX` block itself is untouched — the wrapper is the harness's, exactly as it is
/// on the takumi side, so the block stays the thing a reader can paste.
pub fn build_web_module(imports: &str, jsx_block: &[String]) -> String {
    let body = render_body(jsx_block);
    let wrapped = body.replacen(
        "return (",
        &format!(
            "return (\n    <dom>\n      <div class=\"{CANVAS_CLASS}\">\n        <div class=\"{FRAME_CLASS}\">"
        ),
        1,
    );
    let wrapped = replace_last(
        &wrapped,
        ");",
        "        </div>\n      </div>\n    </dom>\n  );",
    );
    format!("{imports}export default function render() {{\n{wrapped}\n}}\n")
}

fn replace_last(haystack: &str, needle: &str, replacement: &str) -> String {
    match haystack.rfind(needle) {
        Some(at) => format!(
            "{}{replacement}{}",
            &haystack[..at],
            &haystack[at + needle.len()..]
        ),
        None => haystack.to_string(),
    }
}

/// Write the `@ui/*` modules.
pub fn write_ui_modules(out_dir: &Path) -> io::Result<()> {
    let ui_dir = out_dir.join("ui");
    fs::create_dir_all(&ui_dir)?;
    for (specifier, source) in tauler_core::ui::registry::web_module_sources() {
        fs::write(ui_dir.join(module_file_name(&specifier)), source)?;
    }
    Ok(())
}

fn module_file_name(specifier: &str) -> String {
    format!("{}.js", specifier.trim_start_matches("@ui/"))
}

/// The import map that resolves `@ui/*` for the page.
///
/// Inlined into the document rather than served as a file, because an import map has to be
/// in the document before the first module loads — and because that is what lets the
/// printed example be the one that runs, imports and all.
pub fn import_map_json() -> String {
    let imports: Vec<String> = tauler_core::ui::registry::web_module_sources()
        .into_iter()
        .map(|(specifier, _)| {
            format!(
                "    \"{specifier}\": \"/tauler/ui/{}\"",
                module_file_name(&specifier)
            )
        })
        .collect();
    format!("{{\n  \"imports\": {{\n{}\n  }}\n}}\n", imports.join(",\n"))
}

/// The Fragment marker, baked out of `optative-script` so the page's `h` recognises the
/// same one the transform emits.
pub fn write_constants(out_dir: &Path) -> io::Result<()> {
    fs::write(
        out_dir.join("constants.js"),
        format!(
            "// Generated by tauler-docgen. The marker `optative-script`'s transform emits.\nexport const ESTO_FRAGMENT = {:?}\n",
            optative_script::tags::ESTO_FRAGMENT
        ),
    )
}

/// Transform one example to JavaScript and write it.
pub fn write_example(out_dir: &Path, component: &Component, source: &str) -> io::Result<()> {
    let dir = out_dir.join("examples");
    fs::create_dir_all(&dir)?;
    let name = component.export_name.to_lowercase();
    let js = optative_script::jsx::transform_source(source, &format!("{name}.jsx"));
    fs::write(dir.join(format!("{name}.js")), js)
}

/// Write the harvested class list and the stylesheet input that compiles it.
pub fn write_stylesheet_input(
    build_dir: &Path,
    classes: &BTreeSet<String>,
    font_url: &str,
) -> io::Result<()> {
    fs::create_dir_all(build_dir)?;
    fs::write(
        build_dir.join("classes.txt"),
        classes.iter().cloned().collect::<Vec<_>>().join("\n"),
    )?;
    fs::write(build_dir.join("tailwind.css"), stylesheet_input(font_url))
}

/// The stylesheet the documentation site compiles with Tailwind.
///
/// The layer order is load-bearing. Tailwind emits its utilities into `@layer utilities`,
/// and unlayered CSS beats layered CSS — so the reset has to sit in a layer declared
/// *below* Tailwind's, or it would win against the utilities it is meant to sit under.
fn stylesheet_input(font_url: &str) -> String {
    format!(
        r#"/* Generated by tauler-docgen. Do not edit by hand. */
@layer tauler-reset, theme, base, components, utilities;

@import "tailwindcss" source(none);
@source "./classes.txt";

/* The embed sits inside Starlight's `.sl-markdown-content`, whose descendant rules would
   otherwise land on every <p>, <ul> and <h*> tauler renders. `all: revert` rolls each
   property back to the *user-agent* value — which is the same table `layout::presets`
   vendors, and the one the comparison is meant to be against (ADR 0024). */
@layer tauler-reset {{
  .tauler-embed, .tauler-embed * {{ all: revert; }}
}}

/* `all: revert` cannot plug inherited properties: those still arrive from Starlight's
   <body>. They are set explicitly here, to the same values `tauler-screenshot` renders
   with, because they are what the two pictures have to agree about. */
.tauler-embed {{
  font-family: "Inter", sans-serif;
  font-size: 16px;
  font-weight: 400;
  line-height: normal;
  font-style: normal;
  letter-spacing: normal;
  text-align: left;
  /* Inter is variable. If the browser ever synthesises semibold instead of moving the
     `wght` axis, the result is a smeared fake against takumi's real instance — a
     categorical difference that would swamp every other one in the diff. */
  font-synthesis: none;
  width: {width}px;
  max-width: 100%;
}}

.tauler-embed [data-tauler-on] {{ cursor: pointer; touch-action: none; }}

@font-face {{
  font-family: "Inter";
  /* The bytes committed at assets/fonts/inter, not a CDN copy: Inter's releases differ
     between distribution channels, and both sides have to rasterize the same file. */
  src: url("{font_url}") format("truetype");
  font-weight: 100 900;
  font-display: block;
}}
"#,
        width = EMBED_WIDTH,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_example_wrapper_matches_the_screenshot_canvas() {
        let js = build_web_module("", &["<Card />".to_string()]);
        assert!(js.contains(CANVAS_CLASS), "got {js}");
        assert!(js.contains(FRAME_CLASS), "got {js}");
        assert!(js.contains("<dom>"), "got {js}");
    }

    #[test]
    fn a_bare_expression_block_is_wrapped_in_a_return() {
        assert_eq!(
            render_body(&["<Card />".to_string()]),
            "  return (\n<Card />\n  );"
        );
    }

    /// A control's example declares its source before it renders anything, so the block is
    /// already a function body and must not be wrapped again.
    #[test]
    fn a_block_that_returns_is_used_as_written() {
        let block = vec![
            "const v = 1;".to_string(),
            "return (".to_string(),
            "  <Slider value={v} />".to_string(),
            ");".to_string(),
        ];
        assert_eq!(render_body(&block), block.join("\n"));
    }

    /// The wrapper has to land inside the returned expression whichever shape the block
    /// is, or a stateful example renders its statements outside the canvas.
    #[test]
    fn a_stateful_block_still_gets_the_canvas() {
        let js = build_web_module(
            "",
            &[
                "const v = 1;".to_string(),
                "return (".to_string(),
                "  <Slider value={v} />".to_string(),
                ");".to_string(),
            ],
        );
        assert!(js.contains("const v = 1;"), "got {js}");
        assert!(js.contains("<dom>"), "got {js}");
        assert!(js.contains(CANVAS_CLASS), "got {js}");
        let dom_at = js.find("<dom>").expect("dom");
        let decl_at = js.find("const v").expect("decl");
        assert!(decl_at < dom_at, "the declaration must precede the surface");
    }

    /// The reset has to be declared below Tailwind's layers, or unlayered CSS beats the
    /// utilities it is meant to sit under.
    /// A layout file's imports have to work unaltered, so every module the registry knows
    /// about has to be resolvable.
    #[test]
    fn the_import_map_covers_every_ui_module() {
        let map = import_map_json();
        for c in tauler_core::ui::registry::WEB_COMPONENTS {
            assert!(
                map.contains(c.module_path),
                "{} missing from the import map:\n{map}",
                c.module_path
            );
        }
    }

    #[test]
    fn the_reset_layer_is_declared_before_tailwinds() {
        let css = stylesheet_input("/f.ttf");
        let order = css.find("@layer tauler-reset, theme").expect("layer order");
        let reset = css.find("@layer tauler-reset {").expect("reset block");
        assert!(order < reset);
        assert!(css.find("@import \"tailwindcss\"").expect("import") < reset);
    }

    #[test]
    fn the_stylesheet_pins_the_inherited_properties() {
        let css = stylesheet_input("/f.ttf");
        for property in ["font-family", "font-size", "line-height", "font-synthesis"] {
            assert!(css.contains(property), "{property} missing from {css}");
        }
    }
}
