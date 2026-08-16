//! The texture ladder: what each texture actually costs to render.
//!
//! Round one's report could say takumi *can* draw a moiré and never what drawing
//! it costs, which for a surface that repaints on every tick is the number that
//! decides the design. This produces that number.
//!
//! It measures `render_frame_rgba`, which is the whole path a repaint pays —
//! parse, lay out, rasterize. Everything in `render/mod.rs` draws fresh every
//! call now that the cache belongs to the render worker (ADR 0023), so twenty
//! calls really are twenty renders. Nothing in tauler had to change to make this
//! measurable.
//!
//! Two costs are deliberately outside these numbers, and both are constant
//! across rungs: the per-pixel BGRX channel swap that `render_frame` does for
//! X11 on top of this, and the JSX evaluation that produced the tree.
//!
//! `#[ignore]` because it renders 540 full frames and takes minutes. Run it with
//!
//!     cargo test --test texture_ladder --release -- --ignored --nocapture
//!
//! `--release` matters: a debug rasterizer is roughly an order of magnitude
//! slower and the ranking, not just the magnitude, can differ.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauler::backdrop;
use tauler::config::FontConfig;
use tauler::layout::{PanelAnchor, SurfaceKind, SurfaceSpec};
use tauler::{init_global_ctx, preload_layout_images, render_frame_rgba};

/// The one output everything in this test lives on.
const OUTPUT: &str = "LADDER";

/// Timed renders per (rung, size). The brief asks for a mean of 20.
const ITERATIONS: usize = 20;
/// Thrown away first: the first render of a tree pays for font fallback
/// selection and image decode that a steady-state tick does not.
const WARMUP: usize = 3;

/// The three geometries a rice actually uses.
const SIZES: &[(&str, u32, u32)] = &[
    ("rail 340x1080", 340, 1080),
    ("bar 1920x58", 1920, 58),
    ("full 1920x1080", 1920, 1080),
];

/// One rung: a style applied to a child filling the panel.
struct Rung {
    id: &'static str,
    what: &'static str,
    style: Value,
}

fn rungs(plate: &str) -> Vec<Rung> {
    let gold = "rgba(216,178,94,0.55)";
    vec![
        Rung {
            id: "A",
            what: "flat fill (the floor)",
            style: json!({ "backgroundColor": "#1B1924" }),
        },
        Rung {
            id: "B",
            what: "1 linear-gradient",
            style: json!({ "backgroundImage": "linear-gradient(160deg, #2E244A 0%, #120F1C 100%)" }),
        },
        Rung {
            id: "C",
            what: "repeating-linear-gradient, axis-aligned",
            style: json!({
                "backgroundColor": "#1B1924",
                "backgroundImage": format!(
                    "repeating-linear-gradient(0deg, {gold} 0px, {gold} 1px, transparent 1px, transparent 6px)"
                ),
            }),
        },
        Rung {
            id: "D",
            what: "repeating-linear-gradient, 87deg (off-axis)",
            style: json!({
                "backgroundColor": "#1B1924",
                "overflow": "hidden",
                "backgroundImage": format!(
                    "repeating-linear-gradient(87deg, {gold} 0px, {gold} 1px, transparent 1px, transparent 6px)"
                ),
            }),
        },
        Rung {
            id: "E",
            what: "2 off-axis repeating gradients (the moire)",
            style: json!({
                "backgroundColor": "#1B1924",
                "overflow": "hidden",
                "backgroundImage": format!(
                    "repeating-linear-gradient(87deg, {gold} 0px, {gold} 1px, transparent 1px, transparent 6px), \
                     repeating-linear-gradient(93deg, {gold} 0px, {gold} 1px, transparent 1px, transparent 6px)"
                ),
            }),
        },
        Rung {
            id: "F",
            what: "radial tile + background-size (halftone)",
            style: json!({
                "backgroundColor": "#2B2836",
                "backgroundImage": "radial-gradient(circle at 3px 3px, rgba(216,178,94,0.30) 0px, rgba(216,178,94,0.30) 1.4px, transparent 1.6px)",
                "backgroundSize": "9px 9px",
                "backgroundRepeat": "repeat",
            }),
        },
        Rung {
            id: "G",
            what: "6-layer background-image (the heat field)",
            style: json!({
                "backgroundColor": "#120F1C",
                "backgroundImage":
                    "repeating-linear-gradient(0deg, rgba(0,0,0,0.20) 0px, rgba(0,0,0,0.20) 1px, transparent 1px, transparent 3px), \
                     radial-gradient(circle 300px at 22% 72%, rgba(244,214,96,0.92) 0%, rgba(214,112,44,0.72) 38%, rgba(168,62,96,0.34) 66%, transparent 100%), \
                     radial-gradient(circle 480px at 68% 30%, rgba(214,112,44,0.70) 0%, rgba(168,62,96,0.46) 42%, transparent 100%), \
                     radial-gradient(circle 640px at 92% 88%, rgba(168,62,96,0.50) 0%, rgba(92,48,118,0.42) 50%, transparent 100%), \
                     radial-gradient(circle 700px at 8% 8%, rgba(92,48,118,0.60) 0%, transparent 100%), \
                     linear-gradient(160deg, #2E244A 0%, #1B1530 55%, #120F1C 100%)",
            }),
        },
        Rung {
            id: "H",
            what: "the same texture as a pre-rendered PNG",
            style: json!({ "__image": plate }),
        },
        Rung {
            id: "I",
            what: "<img src=\"tauler:root-bg\"> (the C-7 path)",
            style: json!({ "__image": "tauler:root-bg" }),
        },
        // Not in the brief's ladder. The layout-file docs state that the
        // background-image form of root-bg costs ~19ms against ~5ms for the
        // <image> node, and round one repeated that as fact. Same panel, same
        // backdrop, one property apart — so the claim either reproduces here or
        // it does not.
        Rung {
            id: "J",
            what: "backgroundImage: url(tauler:root-bg) (the slow path the docs name)",
            style: json!({ "backgroundImage": "url(tauler:root-bg)" }),
        },
    ]
}

/// Every rung renders the same shape: a root filling the panel with one child
/// filling the root. Only the child differs, so the measurement is of the
/// texture and not of the tree.
fn tree(rung: &Rung) -> Value {
    let fill =
        json!({ "position": "absolute", "top": 0, "left": 0, "width": "100%", "height": "100%" });
    let child = match rung.style.get("__image").and_then(Value::as_str) {
        Some(src) => json!({ "type": "img", "src": src, "style": fill }),
        None => {
            let mut style = fill;
            for (k, v) in rung.style.as_object().expect("rung style is an object") {
                style[k] = v.clone();
            }
            json!({ "type": "div", "style": style })
        }
    };
    json!({
        "type": "div",
        "style": { "position": "relative", "width": "100%", "height": "100%" },
        "children": [child],
    })
}

/// A ruled plate on disk, so rung H measures the fallback round one would have
/// used: draw it once, hand the renderer a file.
fn write_plate(dir: &std::path::Path) -> PathBuf {
    use image::{Rgb, RgbImage};
    let (w, h) = (720u32, 480u32);
    let mut img = RgbImage::from_pixel(w, h, Rgb([27, 25, 36]));
    for y in (0..h).step_by(6) {
        for x in 0..w {
            img.put_pixel(x, y, Rgb([57, 49, 45]));
        }
    }
    let path = dir.join("ladder-plate.png");
    img.save(&path).expect("writing the ladder plate");
    path
}

/// A wallpaper published the way the app publishes one, so rung I pays the real
/// `crop_for` cost rather than a stand-in.
fn publish_field(width: u32, height: u32) {
    let spec = wallpaper_spec(width, height);
    let mut bgrx = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let t = (x + y) % 255;
            bgrx.extend_from_slice(&[t as u8, (255 - t) as u8, 128, 0]);
        }
    }
    backdrop::publish_wallpaper(&spec, std::sync::Arc::new(bgrx), width, height);
}

fn wallpaper_spec(width: u32, height: u32) -> SurfaceSpec {
    SurfaceSpec {
        id: "ladder".into(),
        kind: SurfaceKind::Wallpaper,
        anchor: None,
        x: 0,
        y: 0,
        width,
        height,
        outer_gap: 0,
        // crop_for looks the wallpaper up by output name, so the panel and the
        // wallpaper have to agree on one. `None` means "no root-bg at all".
        output: Some(OUTPUT.into()),
        above: false,
        content: serde_json::Value::Null,
        dpr: 1.0,
    }
}

fn panel_spec(width: u32, height: u32) -> SurfaceSpec {
    SurfaceSpec {
        id: "ladder-panel".into(),
        kind: SurfaceKind::Panel,
        anchor: Some(PanelAnchor::Top),
        x: 0,
        y: 0,
        width,
        height,
        outer_gap: 0,
        // crop_for looks the wallpaper up by output name, so the panel and the
        // wallpaper have to agree on one. `None` means "no root-bg at all".
        output: Some(OUTPUT.into()),
        above: false,
        content: serde_json::Value::Null,
        dpr: 1.0,
    }
}

fn mean(samples: &[Duration]) -> f64 {
    samples.iter().map(|d| d.as_secs_f64()).sum::<f64>() / samples.len() as f64 * 1000.0
}

fn min_ms(samples: &[Duration]) -> f64 {
    samples
        .iter()
        .map(|d| d.as_secs_f64())
        .fold(f64::INFINITY, f64::min)
        * 1000.0
}

#[test]
#[ignore = "renders 540 frames; run with --release --ignored --nocapture"]
fn texture_ladder() {
    init_global_ctx(FontConfig::default());

    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/ladder");
    std::fs::create_dir_all(&out).expect("creating target/ladder");
    let plate = write_plate(&out);
    let plate = plate.to_string_lossy().into_owned();

    let mut tsv =
        String::from("rung\ttexture\tsurface\twidth\theight\tmean_ms\tmin_ms\tvs_floor\n");
    let mut table = String::new();

    for &(size_label, width, height) in SIZES {
        // Rung I needs a wallpaper the size of the screen behind the panel.
        publish_field(1920, 1080);
        let crop = backdrop::crop_for(&panel_spec(width, height), (width, height));
        assert!(crop.is_some(), "no backdrop crop for {size_label}");

        let mut floor = f64::NAN;
        table.push_str(&format!("\n{size_label}\n"));
        table.push_str("  rung  mean      min       x floor   texture\n");

        for rung in rungs(&plate) {
            let content = tree(&rung);
            preload_layout_images(&content);

            // Rung I is the only one that renders against a backdrop; giving the
            // others one would fold the crop's cost into every row.
            let backdrop = if matches!(rung.id, "I" | "J") {
                crop.as_ref()
            } else {
                None
            };

            for _ in 0..WARMUP {
                let _ = render_frame_rgba(&content, width, height, 1.0, backdrop);
            }
            let mut samples = Vec::with_capacity(ITERATIONS);
            for _ in 0..ITERATIONS {
                let t = Instant::now();
                let buf = render_frame_rgba(&content, width, height, 1.0, backdrop);
                samples.push(t.elapsed());
                // Keep the buffer alive past the timer so the optimiser cannot
                // decide the render was dead code.
                assert_eq!(buf.len(), (width * height * 4) as usize);
            }

            let m = mean(&samples);
            let lo = min_ms(&samples);
            if rung.id == "A" {
                floor = m;
            }
            let ratio = m / floor;

            tsv.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\t{:.2}\t{:.2}\t{:.2}\n",
                rung.id, rung.what, size_label, width, height, m, lo, ratio
            ));
            table.push_str(&format!(
                "  {:<5} {:>7.2}ms {:>7.2}ms {:>7.2}x   {}\n",
                rung.id, m, lo, ratio, rung.what
            ));
        }

        backdrop::forget(&wallpaper_spec(1920, 1080));
    }

    let tsv_path = out.join("render.tsv");
    std::fs::write(&tsv_path, &tsv).expect("writing render.tsv");

    println!("{table}");
    println!("\nwrote {}", tsv_path.display());
    println!(
        "\nmean of {ITERATIONS} renders after {WARMUP} warmups, release build, dpr 1.0.\n\
         `x floor` is against rung A at the same size. Excludes the per-pixel BGRX\n\
         swap that `render_frame` adds for X11, and excludes JSX evaluation."
    );
}
