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

    // 16px margin on all sides: baked into the canvas wrapper as padding so the
    // component sits at (16, 16) and the crop region works out automatically.
    const PAD: u32 = 16;
    const CANVAS_H: u32 = 2000;
    let render_w = width;

    // Inject a w-full frame wrapper so every component renders at the full content
    // width (render_w - 2×PAD). This gives consistent screenshot widths regardless
    // of whether the component uses w-full itself.
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
    resolve_tw_in_json(&mut canvas, &theme, theme_mode);

    let bgrx = costae::render_frame(&canvas, render_w, CANVAS_H, 1.0);
    let measured = costae::measure_layout_frame(&canvas, render_w, CANVAS_H, 1.0);

    // Frame bounds: the injected w-full wrapper is always content-area wide,
    // so obj_w = render_w - 2*PAD and obj_h = actual component height.
    let (obj_x, obj_y, obj_w, obj_h) = if let Some(child) = measured.children.first() {
        let x = child.transform[4].floor() as u32;
        let y = child.transform[5].floor() as u32;
        let w = child.width.ceil() as u32;
        let h = child.height.ceil() as u32;
        (x, y, w, h)
    } else {
        (PAD, PAD, render_w.saturating_sub(2 * PAD), measured.height.ceil() as u32)
    };

    // Crop: 16px margin around the object on all four sides.
    let x0 = obj_x.saturating_sub(PAD);
    let y0 = obj_y.saturating_sub(PAD);
    let x1 = (obj_x + obj_w + PAD).min(render_w);
    let y1 = (obj_y + obj_h + PAD).min(CANVAS_H);
    let crop_w = x1 - x0;
    let crop_h = y1 - y0;

    // Convert BGRX → RGBA for the cropped region.
    let src_row = (render_w * 4) as usize;
    let mut rgba: Vec<u8> = Vec::with_capacity((crop_w * crop_h * 4) as usize);
    for y in y0..y1 {
        let row = y as usize * src_row;
        for px in bgrx[row + x0 as usize * 4..row + x1 as usize * 4].chunks_exact(4) {
            rgba.extend_from_slice(&[px[2], px[1], px[0], 255u8]);
        }
    }

    let img = RgbaImage::from_raw(crop_w, crop_h, rgba)
        .expect("RgbaImage::from_raw failed");
    img.save(&output_path).expect("save PNG failed");
    eprintln!("wrote {} ({}x{})", output_path, crop_w, crop_h);
}
