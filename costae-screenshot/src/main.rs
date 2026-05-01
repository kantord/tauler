// costae-screenshot: render a JSX layout to a PNG file.
//
// Pipeline:
//  1. Parse args: --input <jsx-file> --output <png-file> --theme dark|light (default: dark)
//  2. Read JSX source from the input file
//  3. costae::init_global_ctx()
//  4. Load Theme::default_theme()
//  5. Evaluate JSX with JsxEvaluator
//  6. Resolve theme tokens with resolve_tw_in_json
//  7. measure_layout_frame to get content bounds
//  8. render_frame_rgba at measured dimensions (+ 8px padding each side)
//  9. image::RgbaImage::from_raw + save PNG

use std::collections::HashMap;
use std::process;

use costae::jsx::{JsxEvaluator, EvalOutput};
use costae::theme::resolver::resolve_tw_in_json;
use costae::theme::ThemeMode;
use image::RgbaImage;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let mut input: Option<String> = None;
    let mut output: Option<String> = None;
    let mut theme_arg = String::from("dark");
    let mut width: u32 = 400;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--input" => {
                i += 1;
                input = args.get(i).cloned();
            }
            "--output" => {
                i += 1;
                output = args.get(i).cloned();
            }
            "--theme" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    theme_arg = v.clone();
                }
            }
            "--width" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    width = v.parse().unwrap_or(400);
                }
            }
            _ => {}
        }
        i += 1;
    }

    let input_path = input.unwrap_or_else(|| {
        eprintln!("costae-screenshot: --input <path> is required");
        process::exit(1);
    });

    let output_path = output.unwrap_or_else(|| {
        eprintln!("costae-screenshot: --output <path> is required");
        process::exit(1);
    });

    let theme_mode = match theme_arg.as_str() {
        "light" => ThemeMode::Light,
        _ => ThemeMode::Dark,
    };

    // Read JSX source
    let source = std::fs::read_to_string(&input_path).unwrap_or_else(|e| {
        eprintln!("costae-screenshot: failed to read {}: {}", input_path, e);
        process::exit(1);
    });

    // Initialize global context (fonts, etc.)
    costae::init_global_ctx();

    // Load theme
    let theme = costae::theme::Theme::default_theme();

    // Evaluate JSX
    let eval_output: EvalOutput = JsxEvaluator::new(&source, serde_json::Value::Null, None)
        .expect("JsxEvaluator failed")
        .eval(&HashMap::<(String, Option<String>), String>::new())
        .expect("eval failed");

    // Resolve theme tokens
    let mut layout = eval_output.layout;
    resolve_tw_in_json(&mut layout, &theme, theme_mode);

    // Wrap in a background container so screenshots render on a solid background
    // rather than transparent, making borders and subtle colors visible.
    let mut canvas = serde_json::json!({
        "type": "container",
        "tw": "bg-background w-full flex flex-col",
        "children": [layout]
    });
    resolve_tw_in_json(&mut canvas, &theme, theme_mode);

    // Render at fixed width, tall canvas — then crop to content height.
    const PAD: u32 = 8;
    const CANVAS_H: u32 = 2000;
    let render_w = width + PAD * 2;

    // Use the same render_frame path as the bar (BGRX output).
    let bgrx = costae::render_frame(&canvas, render_w, CANVAS_H, 1.0);

    // Measure content height (cache-warm after render).
    let measured = costae::measure_layout_frame(&canvas, render_w, CANVAS_H, 1.0);
    let content_h = (measured.height.ceil() as u32).max(1);
    let final_h = (content_h + PAD * 2).min(CANVAS_H);

    // Crop to final_h rows and convert BGRX → RGBA for PNG.
    let row_bytes = (render_w * 4) as usize;
    let rgba: Vec<u8> = bgrx[..row_bytes * final_h as usize]
        .chunks_exact(4)
        .flat_map(|px| [px[2], px[1], px[0], 255u8])
        .collect();
    let img = RgbaImage::from_raw(render_w, final_h, rgba)
        .expect("RgbaImage::from_raw failed");
    img.save(&output_path).expect("save PNG failed");
    eprintln!("wrote {} ({}x{})", output_path, render_w, final_h);
}
