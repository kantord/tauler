use std::sync::{Arc, Mutex, OnceLock};

use cached::proc_macro::cached;
use parley::fontique::GenericFamily;
use takumi::{
    GlobalContext,
    layout::{Viewport, node::Node},
    rendering::{MeasuredNode, RenderOptions, measure_layout as takumi_measure_layout, render},
    resources::image::ImageSource,
};

use crate::config::FontConfig;
use crate::layout::parse_layout;

static GLOBAL_CTX: OnceLock<Mutex<GlobalContext>> = OnceLock::new();

/// Initialize the global rendering context. Must be called once at startup.
/// Loads fonts into the context before storing it.
pub fn init_global_ctx(font_config: FontConfig) {
    let mut ctx = GlobalContext::default();
    apply_font_config(&mut ctx, &font_config);
    GLOBAL_CTX.set(Mutex::new(ctx)).ok();
}

pub fn with_global_ctx<F, R>(f: F) -> R
where
    F: FnOnce(&GlobalContext) -> R,
{
    let g = GLOBAL_CTX.get().expect("call init_global_ctx before rendering").lock().unwrap();
    f(&g)
}

/// Render `content` into a BGRX framebuffer with an internal LRU cache (capacity 6).
///
/// `width` and `height` are **physical** pixels. `dpr` scales CSS `px` units.
/// The returned buffer is always `width × height × 4` bytes (BGRX).
/// Identical calls (same content + dimensions) return a cloned Arc — no re-render.
pub fn render_frame(content: &serde_json::Value, width: u32, height: u32, dpr: f32) -> Arc<Vec<u8>> {
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

/// Render `content` into a raw RGBA framebuffer (no channel swap, alpha preserved).
///
/// `width` and `height` are **physical** pixels. `dpr` scales CSS `px` units.
/// The returned buffer is always `width × height × 4` bytes (RGBA).
pub fn render_frame_rgba(content: &serde_json::Value, width: u32, height: u32, dpr: f32) -> Arc<Vec<u8>> {
    let canonical = json_canon::to_string(content).unwrap_or_default();
    let layout = serde_json::from_str::<serde_json::Value>(&canonical)
        .ok()
        .and_then(|v| parse_layout(&v).map_err(|e| tracing::error!(error = %e, "layout parse error")).ok());
    with_global_ctx(|global| {
        let node = layout.unwrap_or_else(|| takumi::layout::node::Node::container(vec![]));
        let options = RenderOptions::builder()
            .global(global)
            .viewport(takumi::layout::Viewport::new((Some(width), Some(height))).with_device_pixel_ratio(dpr))
            .node(node)
            .build();
        let rgba = render(options).expect("render").into_raw();
        Arc::new(rgba)
    })
}

pub(crate) fn apply_font_config(ctx: &mut GlobalContext, config: &FontConfig) {
    ctx.font_context.collection.load_system_fonts();

    let emoji_name: Option<&str> = match &config.emoji {
        Some(name) => Some(name.as_str()),
        None => {
            const KNOWN_EMOJI_FAMILY_NAMES: &[&str] =
                &["Noto Color Emoji", "Twemoji Mozilla", "Twitter Color Emoji"];
            KNOWN_EMOJI_FAMILY_NAMES
                .iter()
                .copied()
                .find(|&name| ctx.font_context.collection.family_by_name(name).is_some())
        }
    };

    if let Some(name) = emoji_name {
        if let Some(family_info) = ctx.font_context.collection.family_by_name(name) {
            ctx.font_context
                .collection
                .append_generic_families(GenericFamily::Emoji, std::iter::once(family_info.id()));
        }
    }
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
    use super::{apply_font_config, init_global_ctx, render_frame};
    use crate::config::FontConfig;
    use std::sync::Arc;

    #[test]
    fn render_frame_cache_hit_returns_same_arc() {
        init_global_ctx(FontConfig::default());
        let content = serde_json::json!({});
        let a1 = render_frame(&content, 10, 10, 1.0);
        let a2 = render_frame(&content, 10, 10, 1.0);
        assert!(Arc::ptr_eq(&a1, &a2), "second call with identical args must return same Arc (cache hit)");
    }

    #[test]
    fn render_frame_different_dims_returns_distinct_arc() {
        init_global_ctx(FontConfig::default());
        let content = serde_json::json!({});
        let a1 = render_frame(&content, 10, 10, 1.0);
        let a2 = render_frame(&content, 20, 20, 1.0);
        assert!(!Arc::ptr_eq(&a1, &a2), "different dims must return a distinct Arc (cache miss)");
    }

    #[test]
    fn apply_font_config_maps_emoji_generic_family_when_font_is_present() {
        const NOTO_COLOR_EMOJI: &str = "/usr/share/fonts/noto/NotoColorEmoji.ttf";
        if !std::path::Path::new(NOTO_COLOR_EMOJI).exists() {
            eprintln!("SKIP: {} not found on this system", NOTO_COLOR_EMOJI);
            return;
        }

        let mut ctx = takumi::GlobalContext::default();
        let config = FontConfig { emoji: Some("Noto Color Emoji".to_string()) };

        apply_font_config(&mut ctx, &config);

        let families: Vec<_> = ctx
            .font_context
            .collection
            .generic_families(parley::GenericFamily::Emoji)
            .collect();
        assert!(
            !families.is_empty(),
            "GenericFamily::Emoji should be mapped to at least one family after apply_font_config"
        );
    }
}
