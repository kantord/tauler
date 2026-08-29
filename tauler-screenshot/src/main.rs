// tauler-screenshot: render a JSX layout to a vector SVG file.
//
// Pipeline:
//  1. Parse args (clap)
//  2. Read JSX source from --input
//  3. init_global_ctx, load default theme
//  4. Evaluate JSX with JsxEvaluator, resolve theme tokens
//  5. Wrap in a padded canvas (bg-background + 16px padding + w-full frame)
//  6. render_frame_svg at --width, auto height — the document ends where the
//     layout does, which is what the raster path needed an oversized scratch
//     canvas and a pixel crop for

use std::collections::HashMap;

use clap::Parser;
use tauler::jsx::{EvalOutput, JsxEvaluator};
use tauler::theme::resolver::resolve_theme_tokens;
use tauler::theme::ThemeMode;

fn parse_theme_mode(s: &str) -> Result<ThemeMode, String> {
    match s {
        "dark" => Ok(ThemeMode::Dark),
        "light" => Ok(ThemeMode::Light),
        other => Err(format!("unknown theme '{other}'; expected dark or light")),
    }
}

#[derive(Parser)]
#[command(name = "tauler-screenshot")]
struct Args {
    #[arg(long)]
    input: String,

    #[arg(long)]
    output: String,

    #[arg(long, default_value = "dark", value_parser = parse_theme_mode)]
    theme: ThemeMode,

    /// Render width in CSS pixels. The document is this wide (including 16px margins).
    #[arg(long, default_value_t = tauler::preview::WIDTH)]
    width: u32,

    /// Path to a TTF/OTF font file to use as the primary (sans-serif) font.
    #[arg(long)]
    font_path: Option<std::path::PathBuf>,

    /// Also write every Tailwind class the resolved tree carries, one per line.
    ///
    /// The web renderer hands `class` to the browser verbatim, so Tailwind has to be told
    /// which utilities to compile — and a text scan of the sources cannot say, because
    /// `theme/resolver.rs` has already rewritten `bg-background` into `bg-[#hex]` by the
    /// time anything renders. Harvesting from the tree that actually rendered is the only
    /// list that is guaranteed complete (ADR 0026).
    #[arg(long)]
    classes_out: Option<std::path::PathBuf>,

    /// Also write every painted node's box, keyed by render path, as JSON.
    ///
    /// The other half of the geometry gate: the browser reports the same paths from
    /// `getBoundingClientRect()`, and the two are compared node by node (ADR 0028).
    /// Boxes are in CSS pixels, same as the SVG itself.
    #[arg(long)]
    geometry_out: Option<std::path::PathBuf>,
}

/// Every `class` string in the tree, split into individual utilities.
fn collect_classes(value: &serde_json::Value, out: &mut std::collections::BTreeSet<String>) {
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

fn main() {
    let args = Args::parse();

    let source = std::fs::read_to_string(&args.input)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", args.input, e));

    tauler::init_global_ctx(tauler::config::FontConfig {
        primary_path: args.font_path.map(|p| p.to_string_lossy().to_string()),
        ..Default::default()
    });
    let theme = tauler::theme::Theme::default_theme();

    let eval_output: EvalOutput = JsxEvaluator::new(&source, serde_json::Value::Null, None)
        .expect("JsxEvaluator failed")
        .eval(&HashMap::<(String, Option<String>), String>::new())
        .expect("eval failed");

    let mut layout = eval_output.layout;
    resolve_theme_tokens(&mut layout, &theme, args.theme);

    use tauler::preview::{CANVAS_CLASS, FRAME_CLASS};

    // The frame makes every component render at the full content width, whether or not it
    // uses `w-full`. Both classes come from `tauler::preview` because the browser has to
    // build the same canvas to be photographing the same thing.
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
    resolve_theme_tokens(&mut canvas, &theme, args.theme);

    // After resolution, not before: the point of harvesting here is to see the classes as
    // takumi sees them, which is the same string the browser will be handed.
    if let Some(path) = &args.classes_out {
        let mut classes = std::collections::BTreeSet::new();
        collect_classes(&canvas, &mut classes);
        let body = classes.into_iter().collect::<Vec<_>>().join("\n");
        std::fs::write(path, body).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
    }

    if let Some(path) = &args.geometry_out {
        // A tall scratch height so any component fits; the boxes carry their own
        // extents, so the surplus below the content costs nothing.
        const SCRATCH_H: u32 = 2000;
        let boxes: Vec<serde_json::Value> = tauler::hit_test::painted_boxes(
            &canvas, args.width, SCRATCH_H, 1.0,
        )
        .into_iter()
        .map(|(render_path, r)| {
            serde_json::json!({
                "path": render_path.iter().map(usize::to_string).collect::<Vec<_>>().join("."),
                "x": r.x,
                "y": r.y,
                "width": r.width,
                "height": r.height,
            })
        })
        .collect();
        std::fs::write(
            path,
            serde_json::to_string_pretty(&boxes).expect("serialise boxes"),
        )
        .unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
    }

    let svg = tauler::render_frame_svg(&canvas, args.width);
    std::fs::write(&args.output, &svg).unwrap_or_else(|e| panic!("write {}: {e}", args.output));

    eprintln!("wrote {} ({} bytes)", args.output, svg.len());
}
