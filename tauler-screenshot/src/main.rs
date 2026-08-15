// tauler-screenshot: render a JSX layout to a PNG file.
//
// Pipeline:
//  1. Parse args (clap)
//  2. Read JSX source from --input
//  3. init_global_ctx, load default theme
//  4. Evaluate JSX with JsxEvaluator, resolve theme tokens
//  5. Wrap in a padded canvas (bg-background + 16px padding + w-full frame)
//  6. render_frame (BGRX, same path as the bar)
//  7. measure_layout_frame → actual component bounds
//  8. Crop BGRX to [obj ± 16px], convert to RGBA, save PNG

use std::collections::HashMap;

use clap::Parser;
use image::RgbaImage;
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

    /// Maximum render width in CSS pixels. The final image is this wide (including 16px margins).
    #[arg(long, default_value_t = 400)]
    width: u32,

    /// Device pixel ratio to render at. The image comes out `dpr` times larger, and
    /// fractional values reproduce what a HiDPI display actually does to a layout —
    /// sub-pixel seams between adjacent boxes show up here and nowhere else.
    #[arg(long, default_value_t = 1.0)]
    dpr: f32,

    /// Path to a TTF/OTF font file to use as the primary (sans-serif) font.
    #[arg(long)]
    font_path: Option<std::path::PathBuf>,
}

/// Extract the x/y translation from a 2D affine transform matrix stored as [a,b,c,d,tx,ty].
fn translation_xy(transform: &[f32; 6]) -> (u32, u32) {
    (transform[4].floor() as u32, transform[5].floor() as u32)
}

fn main() {
    let args = Args::parse();

    let source = std::fs::read_to_string(&args.input)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", args.input, e));

    tauler::init_global_ctx(tauler::config::FontConfig {
        primary_path: args.font_path,
        ..Default::default()
    });
    let theme = tauler::theme::Theme::default_theme();

    let eval_output: EvalOutput = JsxEvaluator::new(&source, serde_json::Value::Null, None)
        .expect("JsxEvaluator failed")
        .eval(&HashMap::<(String, Option<String>), String>::new())
        .expect("eval failed");

    let mut layout = eval_output.layout;
    resolve_theme_tokens(&mut layout, &theme, args.theme);

    const PAD: u32 = 16;
    // Large scratch height so auto-crop can handle any component without a pre-measurement pass.
    const CANVAS_H: u32 = 2000;
    let render_w = args.width;

    // Inject a w-full frame wrapper so every component renders at the full content
    // width (render_w - 2×PAD), giving consistent screenshot widths regardless of
    // whether the component itself uses w-full.
    let frame = serde_json::json!({
        "type": "div",
        "class": "w-full flex flex-col",
        "children": [layout]
    });
    let mut canvas = serde_json::json!({
        "type": "div",
        "class": "bg-background w-full flex flex-col p-[16px]",
        "children": [frame]
    });
    resolve_theme_tokens(&mut canvas, &theme, args.theme);

    // Everything below is in physical pixels: `render_frame` takes the buffer size,
    // and the dpr is what turns the layout's CSS pixels into those.
    let dpr = args.dpr.max(0.1);
    let phys_w = (render_w as f32 * dpr).round() as u32;
    let phys_h = (CANVAS_H as f32 * dpr).round() as u32;
    let pad = (PAD as f32 * dpr).round() as u32;

    let bgrx = tauler::render_frame(&canvas, phys_w, phys_h, dpr);
    let measured = tauler::measure_layout_frame(&canvas, phys_w, phys_h, dpr);

    let (obj_x, obj_y, obj_w, obj_h) = if let Some(child) = measured.children.first() {
        let (x, y) = translation_xy(&child.transform);
        (x, y, child.width.ceil() as u32, child.height.ceil() as u32)
    } else {
        (
            pad,
            pad,
            phys_w.saturating_sub(2 * pad),
            measured.height.ceil() as u32,
        )
    };

    let x0 = obj_x.saturating_sub(pad);
    let y0 = obj_y.saturating_sub(pad);
    let x1 = (obj_x + obj_w + pad).min(phys_w);
    let y1 = (obj_y + obj_h + pad).min(phys_h);
    let crop_w = x1 - x0;
    let crop_h = y1 - y0;

    let src_row = (phys_w * 4) as usize;
    let mut rgba: Vec<u8> = Vec::with_capacity((crop_w * crop_h * 4) as usize);
    for y in y0..y1 {
        let row = y as usize * src_row;
        for px in bgrx[row + x0 as usize * 4..row + x1 as usize * 4].chunks_exact(4) {
            rgba.extend_from_slice(&[px[2], px[1], px[0], 255u8]);
        }
    }

    RgbaImage::from_raw(crop_w, crop_h, rgba)
        .expect("RgbaImage::from_raw failed")
        .save(&args.output)
        .expect("save PNG failed");

    eprintln!("wrote {} ({}x{})", args.output, crop_w, crop_h);
}
