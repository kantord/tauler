use std::sync::{Arc, Mutex, OnceLock};

use cached::proc_macro::cached;
use takumi::{
    GlobalContext,
    layout::{Viewport, node::Node},
    rendering::{MeasuredNode, RenderOptions, measure_layout as takumi_measure_layout, render},
    resources::{font::FontResource, image::ImageSource},
};

use crate::layout::parse_layout;

static GLOBAL_CTX: OnceLock<Mutex<GlobalContext>> = OnceLock::new();

/// Initialize the global rendering context. Must be called once at startup.
/// Loads fonts into the context before storing it.
pub fn init_global_ctx() {
    let mut ctx = GlobalContext::default();
    load_fonts_impl(&mut ctx);
    GLOBAL_CTX.set(Mutex::new(ctx)).ok();
}

pub fn with_global_ctx<F, R>(f: F) -> R
where
    F: FnOnce(&GlobalContext) -> R,
{
    let g = GLOBAL_CTX.get().expect("GlobalContext not initialized").lock().unwrap();
    f(&g)
}

/// Render `content` into a BGRX framebuffer with an internal LRU cache (capacity 6).
///
/// `width` and `height` are **physical** pixels. `dpr` scales CSS `px` units.
/// The returned buffer is always `width × height × 4` bytes (BGRX).
/// Identical calls (same content + dimensions) return a cloned Arc — no re-render.
pub fn render_frame(content: &serde_json::Value, width: u32, height: u32, dpr: f32) -> Arc<Vec<u8>> {
    GLOBAL_CTX.get_or_init(|| {
        let mut ctx = GlobalContext::default();
        load_fonts_impl(&mut ctx);
        Mutex::new(ctx)
    });
    let canonical = json_canon::to_string(content).unwrap_or_default();
    render_frame_cached(canonical, width, height, dpr.to_bits())
}

#[cached(size = 6)]
fn render_frame_cached(canonical: String, width: u32, height: u32, dpr_bits: u32) -> Arc<Vec<u8>> {
    let dpr = f32::from_bits(dpr_bits);
    let layout = serde_json::from_str::<serde_json::Value>(&canonical)
        .ok()
        .and_then(|v| parse_layout(&v).map_err(|e| tracing::error!(error = %e, "layout parse error")).ok());
    with_global_ctx(|global| {
        let node = layout.unwrap_or_else(|| Node::container(vec![]));
        let options = RenderOptions::builder()
            .global(global)
            .viewport(Viewport::new((Some(width), Some(height))).with_device_pixel_ratio(dpr))
            .node(node)
            .build();
        let rgba = render(options).expect("render").into_raw();
        let mut bgrx = Vec::with_capacity(rgba.len());
        for px in rgba.chunks_exact(4) {
            bgrx.extend_from_slice(&[px[2], px[1], px[0], 0x00]);
        }
        Arc::new(bgrx)
    })
}

fn load_fonts_impl(global: &mut GlobalContext) {
    let home = std::env::var("HOME").unwrap_or_default();
    let local_fonts = format!("{home}/.local/share/fonts");
    let dot_fonts = format!("{home}/.fonts");

    let candidate_dirs: Vec<std::path::PathBuf> = vec![
        std::path::PathBuf::from("/usr/share/fonts/TTF"),
        std::path::PathBuf::from("/usr/share/fonts/truetype"),
        std::path::PathBuf::from("/usr/share/fonts/OTF"),
        std::path::PathBuf::from("/usr/share/fonts/opentype"),
        std::path::PathBuf::from(&local_fonts),
        std::path::PathBuf::from(&dot_fonts),
    ];

    for path in find_font_files(&candidate_dirs) {
        if let Ok(bytes) = std::fs::read(&path) {
            let _ = global.font_context.load_and_store(FontResource::new(bytes));
        }
    }
}

pub fn find_font_files<P: AsRef<std::path::Path>>(dirs: &[P]) -> Vec<std::path::PathBuf> {
    let mut results = Vec::new();
    for dir in dirs {
        let Ok(read_dir) = std::fs::read_dir(dir.as_ref()) else { continue; };
        for entry in read_dir.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                let ext_lower = ext.to_ascii_lowercase();
                if ext_lower == "ttf" || ext_lower == "otf" {
                    results.push(path);
                }
            }
        }
    }
    results
}

pub fn preload_layout_images(layout: &serde_json::Value) {
    with_global_ctx(|global| preload_layout_images_impl(layout, global));
}

fn preload_layout_images_impl(layout: &serde_json::Value, global: &GlobalContext) {
    fn walk(value: &serde_json::Value, srcs: &mut Vec<String>) {
        match value {
            serde_json::Value::Object(map) => {
                if map.get("type").and_then(|t| t.as_str()) == Some("image") {
                    if let Some(src) = map.get("src").and_then(|s| s.as_str()) {
                        srcs.push(src.to_string());
                    }
                    return; // image nodes are terminal
                }
                for v in map.values() {
                    walk(v, srcs);
                }
            }
            serde_json::Value::Array(arr) => {
                for v in arr {
                    walk(v, srcs);
                }
            }
            _ => {}
        }
    }

    let mut srcs = Vec::new();
    walk(layout, &mut srcs);

    for src in srcs {
        if src.starts_with("http://") || src.starts_with("https://") || src.starts_with("data:") {
            continue;
        }
        if let Ok(bytes) = std::fs::read(&src) {
            if let Ok(image) = ImageSource::from_bytes(&bytes) {
                global.persistent_image_store.insert(src, image);
            }
        }
    }
}

/// Cached layout-only pass (no rasterization). Same cache key as `render_frame`
/// so click handling gets a warm cache hit after any render.
#[cached(size = 6)]
fn measure_layout_cached(canonical: String, width: u32, height: u32, dpr_bits: u32) -> Arc<MeasuredNode> {
    let dpr = f32::from_bits(dpr_bits);
    let layout = serde_json::from_str::<serde_json::Value>(&canonical)
        .ok()
        .and_then(|v| parse_layout(&v).map_err(|e| tracing::error!(error = %e, "layout parse error")).ok());
    with_global_ctx(|global| {
        let node = layout.unwrap_or_else(|| Node::container(vec![]));
        let options = RenderOptions::builder()
            .global(global)
            .viewport(Viewport::new((Some(width), Some(height))).with_device_pixel_ratio(dpr))
            .node(node)
            .build();
        Arc::new(takumi_measure_layout(options).expect("measure_layout"))
    })
}

pub fn measure_layout_frame(content: &serde_json::Value, width: u32, height: u32, dpr: f32) -> Arc<MeasuredNode> {
    let canonical = json_canon::to_string(content).unwrap_or_default();
    measure_layout_cached(canonical, width, height, dpr.to_bits())
}

#[cfg(test)]
mod tests {
    use super::{find_font_files, render_frame};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;

    #[test]
    fn render_frame_cache_hit_returns_same_arc() {
        let content = serde_json::json!({});
        let a1 = render_frame(&content, 10, 10, 1.0);
        let a2 = render_frame(&content, 10, 10, 1.0);
        assert!(Arc::ptr_eq(&a1, &a2), "second call with identical args must return same Arc (cache hit)");
    }

    #[test]
    fn render_frame_different_dims_returns_distinct_arc() {
        let content = serde_json::json!({});
        let a1 = render_frame(&content, 10, 10, 1.0);
        let a2 = render_frame(&content, 20, 20, 1.0);
        assert!(!Arc::ptr_eq(&a1, &a2), "different dims must return a distinct Arc (cache miss)");
    }

    /// Create a uniquely-named subdirectory inside `std::env::temp_dir()` and
    /// return its path. The caller is responsible for cleanup via `fs::remove_dir_all`.
    fn make_temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("costae_find_font_files_{}", name));
        let _ = fs::remove_dir_all(&dir); // clean up leftovers from previous runs
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn empty_dirs_slice_returns_empty_vec() {
        let result = find_font_files::<&std::path::Path>(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn ttf_file_is_returned() {
        let dir = make_temp_dir("ttf");
        let font_path = dir.join("MyFont.ttf");
        fs::write(&font_path, b"fake ttf").unwrap();

        let result = find_font_files(&[dir.as_path()]);

        fs::remove_dir_all(&dir).ok();
        assert_eq!(result, vec![font_path]);
    }

    #[test]
    fn otf_file_is_returned() {
        let dir = make_temp_dir("otf");
        let font_path = dir.join("MyFont.otf");
        fs::write(&font_path, b"fake otf").unwrap();

        let result = find_font_files(&[dir.as_path()]);

        fs::remove_dir_all(&dir).ok();
        assert_eq!(result, vec![font_path]);
    }

    #[test]
    fn non_font_extensions_are_ignored() {
        let dir = make_temp_dir("nonfont");
        fs::write(dir.join("readme.txt"), b"text").unwrap();
        fs::write(dir.join("icon.woff"), b"woff").unwrap();

        let result = find_font_files(&[dir.as_path()]);

        fs::remove_dir_all(&dir).ok();
        assert!(result.is_empty());
    }

    #[test]
    fn nonexistent_directory_is_skipped_silently() {
        let missing = std::env::temp_dir().join("costae_find_font_files_does_not_exist_xyz123");
        let result = find_font_files(&[missing.as_path()]);
        assert!(result.is_empty());
    }

    #[test]
    fn files_from_multiple_directories_are_combined() {
        let dir_a = make_temp_dir("multi_a");
        let dir_b = make_temp_dir("multi_b");
        let font_a = dir_a.join("A.ttf");
        let font_b = dir_b.join("B.otf");
        fs::write(&font_a, b"ttf").unwrap();
        fs::write(&font_b, b"otf").unwrap();

        let mut result = find_font_files(&[dir_a.as_path(), dir_b.as_path()]);
        result.sort();

        let mut expected = vec![font_a, font_b];
        expected.sort();

        fs::remove_dir_all(&dir_a).ok();
        fs::remove_dir_all(&dir_b).ok();
        assert_eq!(result, expected);
    }

    #[test]
    fn subdirectory_fonts_are_not_included() {
        let dir = make_temp_dir("subdir");
        let subdir = dir.join("nested");
        fs::create_dir_all(&subdir).unwrap();
        fs::write(subdir.join("deep.ttf"), b"ttf").unwrap();

        let result = find_font_files(&[dir.as_path()]);

        fs::remove_dir_all(&dir).ok();
        assert!(result.is_empty(), "expected no files from subdirectory, got {:?}", result);
    }

    #[test]
    fn extension_match_is_case_insensitive() {
        let dir = make_temp_dir("case");
        let upper_ttf = dir.join("UPPER.TTF");
        let mixed_otf = dir.join("Mixed.OtF");
        fs::write(&upper_ttf, b"ttf").unwrap();
        fs::write(&mixed_otf, b"otf").unwrap();

        let mut result = find_font_files(&[dir.as_path()]);
        result.sort();

        let mut expected = vec![upper_ttf, mixed_otf];
        expected.sort();

        fs::remove_dir_all(&dir).ok();
        assert_eq!(result, expected);
    }
}
