//! Tracer for the weather card's City scene (`examples/weather-map/`).
//!
//!   ./examples/weather-map/setup.sh                            # once
//!   cargo run --release --example weather_map_tracer -- <out_dir> [city ...]
//!
//! For each city it reports what the scene costs, and writes the card to
//! `<out_dir>/<city>.png` and the scene alone to `<out_dir>/<city>-scene.png` so
//! the look can be judged:
//!
//! - load: the render runtime (`JsxEvaluator::new_with_packages`) — parsing d3
//!   and the geometry, once per reload
//! - eval: a render, averaged — what every tick pays; it only names the scene
//!   by key (the first one also hashes the geometry for that key)
//! - draw: the draw runtime's setup, loading d3 + linkedom into it, and the
//!   scene's `draw` — what tauler pays off the loop, once per key
//! - raster: the panel with no scene, its first raster with the scene (which
//!   rasterizes the Picture) and a later one (which reuses the bitmap)
//!
//! Why the scene is a Picture and not a data URI or a render-time build is
//! measured in docs/adr/0044. The `.data/` directory it reads is gitignored: it
//! holds OSM extracts, which do not belong in this repository. d3 comes the way
//! tauler gets it: `TAULER_D3_DIR`, else downloaded once into
//! `~/.cache/tauler/d3` (see `figures/mod.rs`).
mod figures;

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use figures::ms;
use serde_json::Value;
use tauler::jsx::JsxEvaluator;
use tauler::layout::{parse_root_node, SurfaceKind};
use tauler::theme::resolver::resolve_theme_tokens;
use tauler::theme::{builtin, ThemeMode};

const RUNS: u32 = 20;
const PICTURE: &str = tauler::render::picture::PICTURE_SRC_PREFIX;
/// The card's size in CSS px, as layout.jsx sets it.
const CARD: (f64, f64) = (248.0, 96.0);

/// Drop every `<img>` that shows a Picture.
fn drop_scene(v: &mut Value) {
    match v {
        Value::Object(map) => {
            if let Some(Value::Array(children)) = map.get_mut("children") {
                children.retain(|c| {
                    !c.get("src")
                        .and_then(Value::as_str)
                        .is_some_and(|s| s.starts_with(PICTURE))
                });
            }
            map.values_mut().for_each(drop_scene);
        }
        Value::Array(a) => a.iter_mut().for_each(drop_scene),
        _ => {}
    }
}

/// The first `tauler:picture/<key>` src in the tree.
fn find_picture(v: &Value) -> Option<String> {
    match v {
        Value::Object(map) => map
            .get("src")
            .and_then(Value::as_str)
            .filter(|s| s.starts_with(PICTURE))
            .map(str::to_string)
            .or_else(|| map.values().find_map(find_picture)),
        Value::Array(a) => a.iter().find_map(find_picture),
        _ => None,
    }
}

/// The Picture `src` alone, at `scale` device pixels per CSS px.
fn scene_alone(src: &str, scale: f32) -> image::RgbaImage {
    let card = serde_json::json!({"type": "img", "src": src,
        "style": {"width": CARD.0, "height": CARD.1}});
    let (cw, ch) = (
        (CARD.0 as f32 * scale).round() as u32,
        (CARD.1 as f32 * scale).round() as u32,
    );
    let rgba = tauler::render_frame_rgba(&card, cw, ch, scale, None);
    image::RgbaImage::from_raw(cw, ch, rgba.to_vec()).unwrap()
}

fn save_card(frame: &[u8], w: u32, h: u32, dpr: f32, path: &Path) {
    let img = image::RgbaImage::from_raw(w, h, frame.to_vec()).unwrap();
    let crop_h = ((CARD.1 as f32 + 24.0) * dpr).round() as u32;
    image::imageops::crop_imm(&img, 0, 0, w, crop_h)
        .to_image()
        .save(path)
        .unwrap();
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out_dir = Path::new(args.first().expect("usage: <out_dir> [city ...]"));
    std::fs::create_dir_all(out_dir).unwrap();
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/weather-map");
    // Relative imports resolve from the layout file's own directory.
    let layout_path = dir.join("layout.jsx");
    let cities: Vec<String> = if args.len() > 1 {
        args[1..].to_vec()
    } else {
        ["levittown", "manhattan", "melbourne", "venice"]
            .map(String::from)
            .to_vec()
    };
    let missing: Vec<&str> = cities
        .iter()
        .map(String::as_str)
        .filter(|n| !dir.join(format!(".data/{n}.js")).exists())
        .collect();
    if !missing.is_empty() {
        eprintln!(
            "missing .data/{{{}}}.js — run examples/weather-map/setup.sh \
             (a place not in places.tsv needs fetch-city.sh first)",
            missing.join(",")
        );
        std::process::exit(1);
    }
    let template = std::fs::read_to_string(dir.join("layout.jsx")).unwrap();
    let look = std::env::var("TRACER_LOOK").unwrap_or_else(|_| "planet".into());
    let template = template.replace("__LOOK__", &look);
    let pkg = figures::package_context(&dir);

    tauler::init_global_ctx(tauler::config::FontConfig::default());
    let ctx_for = |theme: &tauler::theme::Theme| {
        serde_json::json!({
            "output": "DP-1", "dpi": 140.0, "screen_width": 2560, "screen_height": 1440,
            "theme": theme.ctx_json(),
        })
    };

    // `TRACER_THEMES=<a,b,...|all>`: every city given × every theme named, no
    // timings. Writes `<city>--<theme>.png` (the card) and `<city>--<theme>-
    // scene.png` (the drawing alone, 2× device pixels). The look has to hold for
    // the population of places and themes, not one of each.
    if let Ok(list) = std::env::var("TRACER_THEMES") {
        let themes = if list == "all" {
            builtin::names()
        } else {
            list.split(',').map(String::from).collect()
        };
        for city in &cities {
            let source = template.replace("__CITY__", city);
            for name in &themes {
                let theme = builtin::by_name(name).unwrap();
                let ctx = ctx_for(&theme);
                let ev = JsxEvaluator::new_with_packages(
                    &source,
                    ctx.clone(),
                    Some(&layout_path),
                    pkg.clone(),
                )
                .expect("compiles");
                let out = ev.eval(&HashMap::new()).expect("eval");
                figures::draw_figures(&source, ctx, &layout_path, &pkg, &out.figure_calls);
                let mut layout = out.layout;
                resolve_theme_tokens(&mut layout, &theme);
                let panel = parse_root_node(&layout)
                    .unwrap()
                    .into_iter()
                    .next()
                    .unwrap();
                let w = (panel.width as f32 * panel.dpr).round() as u32;
                let h = (panel.height as f32 * panel.dpr).round() as u32;
                let frame = tauler::render_frame_rgba(&panel.content, w, h, panel.dpr, None);
                save_card(
                    &frame,
                    w,
                    h,
                    panel.dpr,
                    &out_dir.join(format!("{city}--{name}.png")),
                );
                let src = find_picture(&panel.content).expect("scene picture");
                scene_alone(&src, panel.dpr * 2.0)
                    .save(out_dir.join(format!("{city}--{name}-scene.png")))
                    .unwrap();
            }
        }
        return;
    }

    let theme = builtin::default_for(ThemeMode::Dark);
    let ctx = ctx_for(&theme);
    for city in &cities {
        let source = template.replace("__CITY__", city);
        let t = Instant::now();
        let ev =
            JsxEvaluator::new_with_packages(&source, ctx.clone(), Some(&layout_path), pkg.clone())
                .expect("compiles");
        let load = ms(t);

        let t = Instant::now();
        let first = ev.eval(&HashMap::new()).expect("eval");
        let first_eval = ms(t);
        let t = Instant::now();
        for _ in 0..RUNS {
            std::hint::black_box(ev.eval(&HashMap::new()).expect("eval"));
        }
        let eval = ms(t) / RUNS as f64;

        let drawn = figures::draw_figures(
            &source,
            ctx.clone(),
            &layout_path,
            &pkg,
            &first.figure_calls,
        );
        let (_, draw, svg_len) = drawn.figures.first().expect("one City scene");

        let mut layout = first.layout;
        resolve_theme_tokens(&mut layout, &theme);
        let panel = parse_root_node(&layout)
            .expect("root")
            .into_iter()
            .find(|s| s.kind == SurfaceKind::Panel)
            .expect("panel");
        let w = (panel.width as f32 * panel.dpr).round() as u32;
        let h = (panel.height as f32 * panel.dpr).round() as u32;
        let raster = |content: &Value| {
            tauler::preload_layout_images(content);
            let t = Instant::now();
            for _ in 0..RUNS {
                std::hint::black_box(tauler::render_frame_rgba(content, w, h, panel.dpr, None));
            }
            ms(t) / RUNS as f64
        };
        let mut none = panel.content.clone();
        drop_scene(&mut none);
        let r_none = raster(&none);
        let t = Instant::now();
        let frame = tauler::render_frame_rgba(&panel.content, w, h, panel.dpr, None);
        let r_first = ms(t);
        let r_cached = raster(&panel.content);

        save_card(
            &frame,
            w,
            h,
            panel.dpr,
            &out_dir.join(format!("{city}.png")),
        );
        let src = find_picture(&panel.content).expect("scene picture");
        scene_alone(&src, panel.dpr)
            .save(out_dir.join(format!("{city}-scene.png")))
            .unwrap();

        println!(
            "{city:<10} load {load:6.1} ms | first eval {first_eval:6.1} ms, eval {eval:5.2} ms | \
             draw: setup {:6.1} ms, d3+linkedom {:5.1} ms, scene {draw:6.1} ms ({:4} KB svg) | \
             raster: none {r_none:5.1} ms, first {r_first:5.1} ms, then {r_cached:5.1} ms",
            drawn.setup_ms,
            drawn.libraries_ms,
            svg_len / 1024
        );
    }
}
