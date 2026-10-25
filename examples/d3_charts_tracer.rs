//! Tracer for the d3 chart examples (`examples/d3-charts/`): a line chart, a bar
//! chart and a force-directed graph, each a `<D3Figure>` drawn with idiomatic d3.
//!
//!   cargo run --release --example d3_charts_tracer -- <out_dir> [theme ...]
//!
//! Writes `<out_dir>/<theme>.png` (the sidebar, cropped to the charts) for each
//! theme — all built-ins when none are named — and prints, per theme:
//!
//! - load: the render runtime (`JsxEvaluator::new_with_packages`), d3 included
//! - eval: a render, averaged — what every tick pays; it only names the figures
//! - draw: the draw runtime's setup, loading d3 + linkedom into it, and each
//!   figure's `draw` — what tauler pays off the loop, once per key
//! - raster: the panel's first raster (which rasterizes the Pictures) and a
//!   later one (which reuses their bitmaps)
//!
//! d3 comes the way tauler gets it: `TAULER_D3_DIR`, else downloaded once into
//! `~/.cache/tauler/d3` (see `figures/mod.rs`).
mod figures;

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use figures::ms;
use tauler::jsx::JsxEvaluator;
use tauler::layout::parse_root_node;
use tauler::theme::builtin;
use tauler::theme::resolver::resolve_theme_tokens;

const RUNS: u32 = 20;
/// Logical height of the three sections, padding included.
const CONTENT_H: f32 = 444.0;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out_dir = Path::new(args.first().expect("usage: <out_dir> [theme ...]"));
    std::fs::create_dir_all(out_dir).unwrap();
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/d3-charts");
    if !dir.join(".data/crates.js").exists() {
        eprintln!("missing examples/d3-charts/.data/crates.js — run examples/d3-charts/setup.sh");
        std::process::exit(1);
    }
    // `TRACER_LAYOUT=<file>` swaps the layout, e.g. to time one chart alone.
    let layout_file = std::env::var("TRACER_LAYOUT").unwrap_or_else(|_| "layout.jsx".into());
    // Relative imports resolve from the layout file's own directory.
    let layout_path = dir.join(&layout_file);
    let source = std::fs::read_to_string(&layout_path).unwrap();
    let themes = if args.len() > 1 {
        args[1..].to_vec()
    } else {
        builtin::names()
    };
    let pkg = figures::package_context(&dir);

    tauler::init_global_ctx(tauler::config::FontConfig::default());
    for name in &themes {
        let theme = builtin::by_name(name).unwrap();
        let ctx = serde_json::json!({
            "output": "DP-1", "dpi": 140.0, "screen_width": 2560, "screen_height": 1440,
            "theme": theme.ctx_json(),
        });
        let t = Instant::now();
        let ev =
            JsxEvaluator::new_with_packages(&source, ctx.clone(), Some(&layout_path), pkg.clone())
                .expect("compiles");
        let load = ms(t);
        let first = ev.eval(&HashMap::new()).expect("eval");
        let t = Instant::now();
        for _ in 0..RUNS {
            std::hint::black_box(ev.eval(&HashMap::new()).expect("eval"));
        }
        let eval = ms(t) / RUNS as f64;

        let drawn = figures::draw_figures(&source, ctx, &layout_path, &pkg, &first.figure_calls);

        let mut layout = first.layout;
        resolve_theme_tokens(&mut layout, &theme);
        let panel = parse_root_node(&layout)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        let w = (panel.width as f32 * panel.dpr).round() as u32;
        let h = (panel.height as f32 * panel.dpr).round() as u32;
        let t = Instant::now();
        let frame = tauler::render_frame_rgba(&panel.content, w, h, panel.dpr, None);
        let raster_first = ms(t);
        let t = Instant::now();
        for _ in 0..RUNS {
            std::hint::black_box(tauler::render_frame_rgba(
                &panel.content,
                w,
                h,
                panel.dpr,
                None,
            ));
        }
        let raster = ms(t) / RUNS as f64;
        let img = image::RgbaImage::from_raw(w, h, frame.to_vec()).unwrap();
        let crop_h = (CONTENT_H * panel.dpr).round() as u32;
        image::imageops::crop_imm(&img, 0, 0, w, crop_h)
            .to_image()
            .save(out_dir.join(format!("{name}.png")))
            .unwrap();

        println!(
            "{name:<22} load {load:6.1} ms | eval {eval:5.2} ms | draw: setup {:5.1} ms, \
             d3+linkedom {:5.1} ms | raster: first {raster_first:5.1} ms, then {raster:5.1} ms",
            drawn.setup_ms, drawn.libraries_ms
        );
        for (key, took, len) in &drawn.figures {
            let kind = key.split('/').next().unwrap_or(key);
            println!(
                "{:<22}   draw {kind:<5} {took:6.1} ms | svg {:3} KB",
                "",
                len / 1024
            );
        }
    }
}
