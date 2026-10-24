//! Tracer for the weather card's City scene (`examples/weather-map/`).
//!
//!   ./examples/weather-map/fetch-city.sh <name> <lat> <lon>   # once per city
//!   cargo run --release --example weather_map_tracer -- <out_dir> [city ...]
//!
//! For each city it reports the three costs the design rests on, and writes the
//! rendered panel to `<out_dir>/<city>.png` so the look can be judged:
//!
//! - load: `JsxEvaluator::new` — parsing d3 and the geometry, once per reload
//! - first eval: the render that builds the scene SVG (then memoized)
//! - eval: a later render, which only looks the scene up
//! - raster: drawing the sidebar panel, three ways — no scene, the scene
//!   as an SVG data URI (what the layout emits), and as a PNG file path
//!
//! The `.data/` directory it reads is gitignored: it holds OSM extracts and a d3
//! bundle, neither of which belongs in this repository.
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use serde_json::Value;
use tauler::jsx::JsxEvaluator;
use tauler::layout::{parse_root_node, SurfaceKind};
use tauler::theme::resolver::resolve_theme_tokens;
use tauler::theme::{builtin, ThemeMode};

const RUNS: u32 = 20;

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

/// Replace the scene's `src` everywhere in the tree (`None` drops the node).
fn swap_scene(v: &mut Value, src: Option<&str>) {
    match v {
        Value::Object(map) => {
            if let Some(Value::Array(children)) = map.get_mut("children") {
                children.retain(|c| {
                    src.is_some()
                        || !c
                            .get("src")
                            .and_then(Value::as_str)
                            .is_some_and(|s| s.starts_with("data:"))
                });
            }
            if map.get("type").and_then(Value::as_str) == Some("img") {
                if let (Some(s), Some(Value::String(cur))) = (src, map.get_mut("src")) {
                    if cur.starts_with("data:") {
                        *cur = s.to_string();
                    }
                }
            }
            map.values_mut().for_each(|c| swap_scene(c, src));
        }
        Value::Array(a) => a.iter_mut().for_each(|c| swap_scene(c, src)),
        _ => {}
    }
}

fn find_data_uri(v: &Value) -> Option<String> {
    match v {
        Value::Object(map) => map
            .get("src")
            .and_then(Value::as_str)
            .filter(|s| s.starts_with("data:"))
            .map(str::to_string)
            .or_else(|| map.values().find_map(find_data_uri)),
        Value::Array(a) => a.iter().find_map(find_data_uri),
        _ => None,
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out_dir = Path::new(args.first().expect("usage: <out_dir> [city ...]"));
    std::fs::create_dir_all(out_dir).unwrap();
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/weather-map");
    let cities: Vec<String> = if args.len() > 1 {
        args[1..].to_vec()
    } else {
        ["suburb", "manhattan", "eixample", "gothic"]
            .map(String::from)
            .to_vec()
    };
    let template = std::fs::read_to_string(dir.join("layout.jsx")).unwrap();
    let look = std::env::var("TRACER_LOOK").unwrap_or_else(|_| "planet".into());
    let template = template.replace("__LOOK__", &look);

    tauler::init_global_ctx(tauler::config::FontConfig::default());
    let theme = builtin::default_for(ThemeMode::Dark);
    let ctx = serde_json::json!({
        "output": "DP-1", "dpi": 140.0, "screen_width": 2560, "screen_height": 1440,
        "theme": theme.ctx_json(),
    });

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
                let ctx = serde_json::json!({
                    "output": "DP-1", "dpi": 140.0, "screen_width": 2560, "screen_height": 1440,
                    "theme": theme.ctx_json(),
                });
                let ev = JsxEvaluator::new(&source, ctx, Some(dir.as_path())).expect("compiles");
                let mut layout = ev.eval(&HashMap::new()).expect("eval").layout;
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
                let crop_h = ((96.0 + 24.0) * panel.dpr).round() as u32;
                image::imageops::crop_imm(&img, 0, 0, w, crop_h)
                    .to_image()
                    .save(out_dir.join(format!("{city}--{name}.png")))
                    .unwrap();

                let uri = find_data_uri(&panel.content).expect("scene data uri");
                let scale = panel.dpr * 2.0;
                let (cw, ch) = (
                    (248.0 * scale).round() as u32,
                    (96.0 * scale).round() as u32,
                );
                let card = serde_json::json!({"type": "img", "src": uri,
                    "style": {"width": 248, "height": 96}});
                let rgba = tauler::render_frame_rgba(&card, cw, ch, scale, None);
                image::RgbaImage::from_raw(cw, ch, rgba.to_vec())
                    .unwrap()
                    .save(out_dir.join(format!("{city}--{name}-scene.png")))
                    .unwrap();
            }
        }
        return;
    }

    for city in &cities {
        let source = template.replace("__CITY__", city);
        let t = Instant::now();
        let ev = JsxEvaluator::new(&source, ctx.clone(), Some(dir.as_path())).expect("compiles");
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
            .expect("root")
            .into_iter()
            .find(|s| s.kind == SurfaceKind::Panel)
            .expect("panel");
        let w = (panel.width as f32 * panel.dpr).round() as u32;
        let h = (panel.height as f32 * panel.dpr).round() as u32;
        let uri = find_data_uri(&panel.content).expect("scene data uri");

        // The PNG variant: draw the scene alone once at device pixels and hand
        // the file to the panel by path, the way `preload_layout_images` caches.
        let card = serde_json::json!({"type": "img", "src": uri,
            "style": {"width": 248, "height": 96}});
        let (cw, ch) = (
            (248.0 * panel.dpr).round() as u32,
            (96.0 * panel.dpr).round() as u32,
        );
        let rgba = tauler::render_frame_rgba(&card, cw, ch, panel.dpr, None);
        let png = out_dir.join(format!("{city}-scene.png"));
        image::RgbaImage::from_raw(cw, ch, rgba.to_vec())
            .unwrap()
            .save(&png)
            .unwrap();

        let raster = |content: &Value| {
            tauler::preload_layout_images(content);
            let t = Instant::now();
            for _ in 0..RUNS {
                std::hint::black_box(tauler::render_frame_rgba(content, w, h, panel.dpr, None));
            }
            ms(t) / RUNS as f64
        };
        let mut none = panel.content.clone();
        swap_scene(&mut none, None);
        let mut file = panel.content.clone();
        swap_scene(&mut file, Some(png.to_str().unwrap()));
        let (r_none, r_uri, r_file) = (raster(&none), raster(&panel.content), raster(&file));

        let crop_h = ((96.0 + 24.0) * panel.dpr).round() as u32;
        for (content, name) in [
            (&panel.content, city.to_string()),
            (&file, format!("{city}-via-file")),
        ] {
            let frame = tauler::render_frame_rgba(content, w, h, panel.dpr, None);
            let img = image::RgbaImage::from_raw(w, h, frame.to_vec()).unwrap();
            image::imageops::crop_imm(&img, 0, 0, w, crop_h)
                .to_image()
                .save(out_dir.join(format!("{name}.png")))
                .unwrap();
        }

        // Handoff: the same geometry as a stream value, scene as a short key.
        let stream_src = std::fs::read_to_string(dir.join("layout-stream.jsx")).unwrap();
        let ev2 =
            JsxEvaluator::new(&stream_src, ctx.clone(), Some(dir.as_path())).expect("compiles");
        let module = std::fs::read_to_string(dir.join(format!(".data/{city}.js"))).unwrap();
        let json = module
            .trim()
            .trim_start_matches("export default ")
            .trim_end_matches(';')
            .to_string();
        let streams = HashMap::from([(("city-fetcher".to_string(), None), json)]);
        let t = Instant::now();
        ev2.eval(&streams).expect("eval");
        let s_first = ms(t);
        let t = Instant::now();
        for _ in 0..RUNS {
            std::hint::black_box(ev2.eval(&streams).expect("eval"));
        }
        let s_eval = ms(t) / RUNS as f64;
        println!("{city:<10} via stream + key: first eval {s_first:6.1} ms | eval {s_eval:5.2} ms");

        println!(
            "{city:<10} load {load:6.1} ms | first eval {first_eval:6.1} ms | eval {eval:5.2} ms | \
             svg {:4} KB | raster: none {r_none:5.1} ms, data-uri {r_uri:5.1} ms, png file {r_file:5.1} ms",
            uri.len() / 1024
        );
    }
}
