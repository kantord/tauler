//! Tracer for the d3 chart examples (`examples/d3-charts/`): a line chart, a bar
//! chart and a force-directed graph, each written against the planned
//! `<D3View>` API with idiomatic d3 on linkedom.
//!
//!   cargo run --release --example d3_charts_tracer -- <out_dir> [theme ...]
//!
//! Writes `<out_dir>/<theme>.png` (the sidebar, cropped to the charts) for each
//! theme — all built-ins when none are named — and prints, per theme, the load
//! (parsing d3 + linkedom), the first eval (which draws all three charts) and a
//! later eval (which only looks them up).
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use tauler::jsx::JsxEvaluator;
use tauler::layout::parse_root_node;
use tauler::theme::builtin;
use tauler::theme::resolver::resolve_theme_tokens;

const RUNS: u32 = 20;
/// Logical height of the three sections, padding included.
const CONTENT_H: f32 = 444.0;

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out_dir = Path::new(args.first().expect("usage: <out_dir> [theme ...]"));
    std::fs::create_dir_all(out_dir).unwrap();
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/d3-charts");
    // `TRACER_LAYOUT=<file>` swaps the layout, e.g. to time one chart alone.
    let layout_file = std::env::var("TRACER_LAYOUT").unwrap_or_else(|_| "layout.jsx".into());
    let source = std::fs::read_to_string(dir.join(layout_file)).unwrap();
    let themes = if args.len() > 1 {
        args[1..].to_vec()
    } else {
        builtin::names()
    };

    tauler::init_global_ctx(tauler::config::FontConfig::default());
    for name in &themes {
        let theme = builtin::by_name(name).unwrap();
        let ctx = serde_json::json!({
            "output": "DP-1", "dpi": 140.0, "screen_width": 2560, "screen_height": 1440,
            "theme": theme.ctx_json(),
        });
        let t = Instant::now();
        let ev = JsxEvaluator::new(&source, ctx, Some(dir.as_path())).expect("compiles");
        let load = ms(t);
        let t = Instant::now();
        let first = ev.eval(&HashMap::new()).expect("eval");
        let first_eval = ms(t);
        let t = Instant::now();
        for _ in 0..RUNS {
            std::hint::black_box(ev.eval(&HashMap::new()).expect("eval"));
        }
        let eval = ms(t) / RUNS as f64;

        let mut layout = first.layout;
        resolve_theme_tokens(&mut layout, &theme);
        let panel = parse_root_node(&layout)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        let w = (panel.width as f32 * panel.dpr).round() as u32;
        let h = (panel.height as f32 * panel.dpr).round() as u32;
        let frame = tauler::render_frame_rgba(&panel.content, w, h, panel.dpr, None);
        let img = image::RgbaImage::from_raw(w, h, frame.to_vec()).unwrap();
        let crop_h = (CONTENT_H * panel.dpr).round() as u32;
        image::imageops::crop_imm(&img, 0, 0, w, crop_h)
            .to_image()
            .save(out_dir.join(format!("{name}.png")))
            .unwrap();
        println!(
            "{name:<22} load {load:6.1} ms | first eval (draws all three) {first_eval:6.1} ms | eval {eval:5.2} ms"
        );
    }
}
