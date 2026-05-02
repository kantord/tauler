// costae-screenshot: render a JSX layout to a PNG file.
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
use costae::jsx::{EvalOutput, JsxEvaluator};
use costae::theme::resolver::resolve_tw_in_json;
use costae::theme::ThemeMode;
use image::RgbaImage;

fn parse_theme_mode(s: &str) -> Result<ThemeMode, String> {
    match s {
        "dark" => Ok(ThemeMode::Dark),
        "light" => Ok(ThemeMode::Light),
        other => Err(format!("unknown theme '{other}'; expected dark or light")),
    }
}

#[derive(Parser)]
#[command(name = "costae-screenshot")]
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

    costae::init_global_ctx(costae::config::FontConfig {
        primary_path: args.font_path,
        ..Default::default()
    });
    let theme = costae::theme::Theme::default_theme();

    let eval_output: EvalOutput = JsxEvaluator::new(&source, serde_json::Value::Null, None)
        .expect("JsxEvaluator failed")
        .eval(&HashMap::<(String, Option<String>), String>::new())
        .expect("eval failed");

    let mut layout = eval_output.layout;
    resolve_tw_in_json(&mut layout, &theme, args.theme);

    const PAD: u32 = 16;
    // Large scratch height so auto-crop can handle any component without a pre-measurement pass.
    const CANVAS_H: u32 = 2000;
    let render_w = args.width;

    // Inject a w-full frame wrapper so every component renders at the full content
    // width (render_w - 2×PAD), giving consistent screenshot widths regardless of
    // whether the component itself uses w-full.
    let frame = serde_json::json!({
        "type": "container",
        "tw": "w-full flex flex-col",
        "children": [layout]
    });
    let mut canvas = serde_json::json!({
        "type": "container",
        "tw": "bg-background w-full flex flex-col p-[16px]",
        "children": [frame]
    });
    resolve_tw_in_json(&mut canvas, &theme, args.theme);

    let bgrx = costae::render_frame(&canvas, render_w, CANVAS_H, 1.0);
    let measured = costae::measure_layout_frame(&canvas, render_w, CANVAS_H, 1.0);

    let (obj_x, obj_y, obj_w, obj_h) = if let Some(child) = measured.children.first() {
        let (x, y) = translation_xy(&child.transform);
        (x, y, child.width.ceil() as u32, child.height.ceil() as u32)
    } else {
        (
            PAD,
            PAD,
            render_w.saturating_sub(2 * PAD),
            measured.height.ceil() as u32,
        )
    };

    let x0 = obj_x.saturating_sub(PAD);
    let y0 = obj_y.saturating_sub(PAD);
    let x1 = (obj_x + obj_w + PAD).min(render_w);
    let y1 = (obj_y + obj_h + PAD).min(CANVAS_H);
    let crop_w = x1 - x0;
    let crop_h = y1 - y0;

    let src_row = (render_w * 4) as usize;
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
