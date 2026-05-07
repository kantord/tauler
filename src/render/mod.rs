use std::sync::{Arc, Mutex, OnceLock};

use takumi_incr::{PartialRenderCtx, PartialRenderScene};

use cached::proc_macro::cached;
use cached::Cached;
use parley::fontique::GenericFamily;
use takumi::{
    layout::{node::Node, Viewport},
    rendering::{measure_layout as takumi_measure_layout, render, MeasuredNode, RenderOptions},
    resources::image::ImageSource,
    GlobalContext,
};

use crate::config::FontConfig;
use crate::layout::parse_layout;

static GLOBAL_CTX: OnceLock<Mutex<GlobalContext>> = OnceLock::new();
static PARTIAL_CTX: OnceLock<Mutex<PartialRenderCtx>> = OnceLock::new();

/// Initialize the global rendering context. Must be called once at startup.
/// Loads fonts into the context before storing it.
pub fn init_global_ctx(font_config: FontConfig) {
    let mut ctx = GlobalContext::default();
    load_targeted_fonts(&mut ctx);
    apply_font_config(&mut ctx, &font_config);
    GLOBAL_CTX.set(Mutex::new(ctx)).ok();
    PARTIAL_CTX.set(Mutex::new(PartialRenderCtx::new())).ok();
}

pub fn with_global_ctx<F, R>(f: F) -> R
where
    F: FnOnce(&GlobalContext) -> R,
{
    let g = GLOBAL_CTX
        .get()
        .expect("call init_global_ctx before rendering")
        .lock()
        .unwrap();
    f(&g)
}

/// Update the global rendering context's font configuration at runtime.
/// Clears the render and layout caches so subsequent calls use the new fonts.
pub fn reload_font_config(font_config: FontConfig) {
    if let Some(mutex) = GLOBAL_CTX.get() {
        let mut ctx = mutex.lock().unwrap();
        apply_font_config(&mut ctx, &font_config);
        RENDER_FRAME_CACHED.lock().cache_clear();
        MEASURE_LAYOUT_CACHED.lock().cache_clear();
    }
}

/// Render `content` into a BGRX framebuffer with an internal LRU cache (capacity 6).
///
/// `width` and `height` are **physical** pixels. `dpr` scales CSS `px` units.
/// The returned buffer is always `width × height × 4` bytes (BGRX).
/// Identical calls (same content + dimensions) return a cloned Arc — no re-render.
pub fn render_frame(
    content: &serde_json::Value,
    width: u32,
    height: u32,
    dpr: f32,
) -> Arc<Vec<u8>> {
    let canonical = json_canon::to_string(content).unwrap_or_default();
    render_frame_cached(canonical, width, height, dpr.to_bits())
}

#[cached(size = 6)]
fn render_frame_cached(canonical: String, width: u32, height: u32, dpr_bits: u32) -> Arc<Vec<u8>> {
    let dpr = f32::from_bits(dpr_bits);
    let layout = serde_json::from_str::<serde_json::Value>(&canonical)
        .ok()
        .and_then(|v| {
            parse_layout(&v)
                .map_err(|e| tracing::error!(error = %e, "layout parse error"))
                .ok()
        });
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

/// Incrementally render `content` using per-panel `scene` state.
/// Falls back to a full render if the scene is not yet warm.
/// Returns pixels in BGRX format (same as `render_frame`).
pub fn render_frame_partial(
    scene: &mut PartialRenderScene,
    content: &serde_json::Value,
    width: u32,
    height: u32,
    dpr: f32,
) -> Arc<Vec<u8>> {
    let mut pctx = PARTIAL_CTX
        .get()
        .expect("render_frame_partial called before init_global_ctx")
        .lock()
        .expect("PARTIAL_CTX poisoned");
    let pixels = with_global_ctx(|global| {
        scene.render_frame(&mut pctx, global, content, width, height, dpr).to_vec()
    });
    let mut bgrx = Vec::with_capacity(pixels.len());
    for px in pixels.chunks_exact(4) {
        bgrx.extend_from_slice(&[px[2], px[1], px[0], 0x00]);
    }
    Arc::new(bgrx)
}

/// Render `content` into a raw RGBA framebuffer (no channel swap, alpha preserved).
///
/// `width` and `height` are **physical** pixels. `dpr` scales CSS `px` units.
/// The returned buffer is always `width × height × 4` bytes (RGBA).
pub fn render_frame_rgba(
    content: &serde_json::Value,
    width: u32,
    height: u32,
    dpr: f32,
) -> Arc<Vec<u8>> {
    let canonical = json_canon::to_string(content).unwrap_or_default();
    let layout = serde_json::from_str::<serde_json::Value>(&canonical)
        .ok()
        .and_then(|v| {
            parse_layout(&v)
                .map_err(|e| tracing::error!(error = %e, "layout parse error"))
                .ok()
        });
    with_global_ctx(|global| {
        let node = layout.unwrap_or_else(|| takumi::layout::node::Node::container(vec![]));
        let options = RenderOptions::builder()
            .global(global)
            .viewport(
                takumi::layout::Viewport::new((Some(width), Some(height)))
                    .with_device_pixel_ratio(dpr),
            )
            .node(node)
            .build();
        let rgba = render(options).expect("render").into_raw();
        Arc::new(rgba)
    })
}

/// Return the name of the first font family that has a font sourced from `path`.
fn family_name_for_path(
    collection: &mut parley::fontique::Collection,
    path: &std::path::Path,
) -> Option<String> {
    use parley::fontique::SourceKind;
    let names: Vec<String> = collection.family_names().map(|s| s.to_string()).collect();
    for name in &names {
        if let Some(info) = collection.family_by_name(name) {
            for font in info.fonts() {
                if let SourceKind::Path(p) = &font.source().kind {
                    if p.as_ref() == path {
                        return Some(name.clone());
                    }
                }
            }
        }
    }
    None
}

pub(crate) fn apply_font_config(ctx: &mut GlobalContext, config: &FontConfig) {
    let path_loaded_family: Option<String> = if let Some(path) = &config.primary_path {
        ctx.font_context
            .collection
            .load_fonts_from_paths(std::iter::once(path));
        // Find the family that owns a font at `path`, whether newly loaded or pre-existing.
        family_name_for_path(&mut ctx.font_context.collection, path)
    } else {
        None
    };

    let emoji_name: Option<&str> = config.emoji.as_deref();

    if let Some(name) = emoji_name {
        if let Some(family_info) = ctx.font_context.collection.family_by_name(name) {
            ctx.font_context
                .collection
                .set_generic_families(GenericFamily::Emoji, std::iter::once(family_info.id()));
        }
    }

    let primary_name = config.primary.as_deref().or(path_loaded_family.as_deref());
    if let Some(name) = primary_name {
        if let Some(family_info) = ctx.font_context.collection.family_by_name(name) {
            ctx.font_context
                .collection
                .set_generic_families(GenericFamily::SansSerif, std::iter::once(family_info.id()));
        }
    }
}

pub fn load_targeted_fonts(ctx: &mut GlobalContext) {
    use parley::fontique::{Collection, CollectionOptions, SourceKind};

    // Build a temporary full collection so fontique+fontconfig can resolve
    // which font file backs each generic family.
    let mut temp = Collection::new(CollectionOptions {
        shared: false,
        system_fonts: false,
    });
    temp.load_system_fonts();

    let targeted = [
        GenericFamily::SansSerif,
        GenericFamily::Monospace,
        GenericFamily::Emoji,
    ];

    let mut paths: Vec<(GenericFamily, std::path::PathBuf)> = Vec::new();
    for &generic in &targeted {
        let Some(id) = temp.generic_families(generic).next() else {
            continue;
        };
        let names: Vec<String> = temp.family_names().map(|s| s.to_string()).collect();
        let Some(name) = names
            .iter()
            .find(|n| temp.family_by_name(n).map(|i| i.id()) == Some(id))
        else {
            continue;
        };
        let Some(family) = temp.family_by_name(name) else {
            continue;
        };
        let Some(path) = family
            .fonts()
            .iter()
            .find_map(|font| match &font.source().kind {
                SourceKind::Path(p) => Some(p.as_ref().to_path_buf()),
                _ => None,
            })
        else {
            continue;
        };
        paths.push((generic, path));
    }

    if paths.is_empty() {
        ctx.font_context.collection.load_system_fonts();
        return;
    }

    for (_, path) in &paths {
        ctx.font_context
            .collection
            .load_fonts_from_paths(std::iter::once(path));
    }
    for (generic, path) in &paths {
        if let Some(name) = family_name_for_path(&mut ctx.font_context.collection, path) {
            if let Some(info) = ctx.font_context.collection.family_by_name(&name) {
                ctx.font_context
                    .collection
                    .set_generic_families(*generic, std::iter::once(info.id()));
            }
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
fn measure_layout_cached(
    canonical: String,
    width: u32,
    height: u32,
    dpr_bits: u32,
) -> Arc<MeasuredNode> {
    let dpr = f32::from_bits(dpr_bits);
    let layout = serde_json::from_str::<serde_json::Value>(&canonical)
        .ok()
        .and_then(|v| {
            parse_layout(&v)
                .map_err(|e| tracing::error!(error = %e, "layout parse error"))
                .ok()
        });
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

pub fn measure_layout_frame(
    content: &serde_json::Value,
    width: u32,
    height: u32,
    dpr: f32,
) -> Arc<MeasuredNode> {
    let canonical = json_canon::to_string(content).unwrap_or_default();
    measure_layout_cached(canonical, width, height, dpr.to_bits())
}

#[cfg(test)]
mod tests {
    use super::{apply_font_config, init_global_ctx, render_frame, GLOBAL_CTX};
    use crate::config::FontConfig;
    use std::sync::Arc;

    fn with_global_ctx_mut<F, R>(f: F) -> R
    where
        F: FnOnce(&mut takumi::GlobalContext) -> R,
    {
        let mut g = GLOBAL_CTX
            .get()
            .expect("call init_global_ctx before rendering")
            .lock()
            .unwrap();
        f(&mut g)
    }

    #[test]
    fn render_frame_cache_hit_returns_same_arc() {
        init_global_ctx(FontConfig::default());
        let content = serde_json::json!({});
        let a1 = render_frame(&content, 10, 10, 1.0);
        let a2 = render_frame(&content, 10, 10, 1.0);
        assert!(
            Arc::ptr_eq(&a1, &a2),
            "second call with identical args must return same Arc (cache hit)"
        );
    }

    #[test]
    fn render_frame_different_dims_returns_distinct_arc() {
        init_global_ctx(FontConfig::default());
        let content = serde_json::json!({});
        let a1 = render_frame(&content, 10, 10, 1.0);
        let a2 = render_frame(&content, 20, 20, 1.0);
        assert!(
            !Arc::ptr_eq(&a1, &a2),
            "different dims must return a distinct Arc (cache miss)"
        );
    }

    #[test]
    fn apply_font_config_maps_emoji_generic_family_when_font_is_present() {
        let mut ctx = takumi::GlobalContext::default();
        ctx.font_context.collection.load_system_fonts();
        if ctx
            .font_context
            .collection
            .family_by_name("Noto Color Emoji")
            .is_none()
        {
            eprintln!("SKIP: Noto Color Emoji not found on this system");
            return;
        }

        let config = FontConfig {
            emoji: Some("Noto Color Emoji".to_string()),
            primary: None,
            primary_path: None,
        };

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

    #[test]
    fn apply_font_config_maps_sans_serif_generic_family_when_primary_font_is_present() {
        let mut ctx = takumi::GlobalContext::default();
        ctx.font_context.collection.load_system_fonts();
        if ctx
            .font_context
            .collection
            .family_by_name("Adwaita Sans")
            .is_none()
        {
            eprintln!("SKIP: Adwaita Sans not found on this system");
            return;
        }
        apply_font_config(
            &mut ctx,
            &FontConfig {
                primary: Some("Adwaita Sans".to_string()),
                emoji: None,
                primary_path: None,
            },
        );
        let families: Vec<_> = ctx
            .font_context
            .collection
            .generic_families(parley::GenericFamily::SansSerif)
            .collect();
        assert!(!families.is_empty());
    }

    #[test]
    fn apply_font_config_updates_sans_serif_mapping_when_called_twice_with_different_primary_font()
    {
        let mut ctx = takumi::GlobalContext::default();

        apply_font_config(
            &mut ctx,
            &FontConfig {
                primary: Some("Adwaita Sans".to_string()),
                emoji: None,
                primary_path: None,
            },
        );
        let first_id = ctx
            .font_context
            .collection
            .generic_families(parley::GenericFamily::SansSerif)
            .next();
        if first_id.is_none() {
            eprintln!("SKIP: Adwaita Sans not found on this system");
            return;
        }

        apply_font_config(
            &mut ctx,
            &FontConfig {
                primary: Some("Liberation Serif".to_string()),
                emoji: None,
                primary_path: None,
            },
        );
        let second_id = ctx
            .font_context
            .collection
            .generic_families(parley::GenericFamily::SansSerif)
            .next();
        if second_id.is_none() {
            eprintln!("SKIP: Liberation Serif not found on this system");
            return;
        }

        assert_ne!(first_id, second_id);
    }

    #[test]
    fn reload_font_config_updates_global_ctx_sans_serif_mapping() {
        fn fc_match_path(pattern: &str) -> Option<std::path::PathBuf> {
            let out = std::process::Command::new("fc-match")
                .args(["--format", "%{file}", pattern])
                .output()
                .ok()?;
            let s = String::from_utf8(out.stdout).ok()?;
            let p = std::path::PathBuf::from(s.trim());
            p.exists().then_some(p)
        }

        let sans_path = fc_match_path("sans-serif");
        let mono_path = fc_match_path("monospace");

        let (first_path, second_path) = match (sans_path, mono_path) {
            (Some(s), Some(m)) if s != m => (s, m),
            _ => {
                eprintln!("SKIP: could not resolve two distinct font paths via fc-match");
                return;
            }
        };

        init_global_ctx(FontConfig {
            primary: None,
            emoji: None,
            primary_path: Some(first_path),
        });

        let first_id = with_global_ctx_mut(|ctx| {
            ctx.font_context
                .collection
                .generic_families(parley::GenericFamily::SansSerif)
                .next()
        });
        if first_id.is_none() {
            eprintln!("SKIP: first font not mapped");
            return;
        }

        super::reload_font_config(FontConfig {
            primary: None,
            emoji: None,
            primary_path: Some(second_path),
        });

        let second_id = with_global_ctx_mut(|ctx| {
            ctx.font_context
                .collection
                .generic_families(parley::GenericFamily::SansSerif)
                .next()
        });
        assert!(second_id.is_some());
        assert_ne!(first_id, second_id);
    }

    #[test]
    fn load_targeted_fonts_populates_only_targeted_families_and_maps_sans_serif() {
        let mut ctx = takumi::GlobalContext::default();
        super::load_targeted_fonts(&mut ctx);

        let count = ctx.font_context.collection.family_names().count();

        // If fontconfig isn't available nothing gets loaded — skip gracefully.
        if count == 0 {
            eprintln!("SKIP: no fonts loaded (fontconfig unavailable?)");
            return;
        }

        assert!(
            count < 20,
            "load_targeted_fonts should load only a small targeted set, got {count} families"
        );

        let sans_serif_mapped = ctx
            .font_context
            .collection
            .generic_families(parley::GenericFamily::SansSerif)
            .next();
        assert!(
            sans_serif_mapped.is_some(),
            "load_targeted_fonts must map GenericFamily::SansSerif to a real font"
        );
    }

    #[test]
    fn bench_system_fonts_vs_minimal() {
        use crate::layout::parse_layout;
        use std::time::Instant;
        use takumi::{
            layout::Viewport,
            rendering::{render, RenderOptions},
            GlobalContext,
        };

        fn fc_match(pattern: &str) -> Option<std::path::PathBuf> {
            let out = std::process::Command::new("fc-match")
                .args(["--format", "%{file}", pattern])
                .output()
                .ok()?;
            let s = String::from_utf8(out.stdout).ok()?;
            let p = std::path::PathBuf::from(s.trim());
            p.exists().then_some(p)
        }

        let (sans, mono, emoji) = match (
            fc_match("sans-serif"),
            fc_match("monospace"),
            fc_match("emoji"),
        ) {
            (Some(s), Some(m), Some(e)) => (s, m, e),
            _ => {
                eprintln!("SKIP: could not resolve font paths via fc-match");
                return;
            }
        };

        // Realistic bar scene: Latin + digits + emoji (stresses fallback path)
        let content = serde_json::json!({
            "type": "container",
            "style": { "flexDirection": "column", "width": 364, "height": 2159 },
            "children": [
                { "type": "text", "text": "Mon 5  09:42" },
                { "type": "text", "text": "main  fix/issue-113" },
                { "type": "text", "text": "👋  🎉  ✅  🔵  🔴" },
                { "type": "text", "text": "CPU 42%  MEM 8.1G" },
            ]
        });
        let node = parse_layout(&content).expect("parse");

        const N: usize = 30;

        let render_once = |ctx: &GlobalContext| -> u128 {
            let opts = RenderOptions::builder()
                .global(ctx)
                .viewport(Viewport::new((Some(364), Some(2159))))
                .node(node.clone())
                .build();
            let t = Instant::now();
            let _ = render(opts).expect("render");
            t.elapsed().as_micros()
        };

        // --- baseline: all system fonts ---
        let mut ctx_sys = GlobalContext::default();
        ctx_sys.font_context.collection.load_system_fonts();
        let family_count = ctx_sys.font_context.collection.family_names().count();
        let _ = render_once(&ctx_sys); // warm-up
        let mut times_sys: Vec<u128> = (0..N).map(|_| render_once(&ctx_sys)).collect();
        times_sys.sort_unstable();

        // --- candidate: minimal curated fonts ---
        let mut ctx_min = GlobalContext::default();
        for path in [&sans, &mono, &emoji] {
            ctx_min
                .font_context
                .collection
                .load_fonts_from_paths(std::iter::once(path));
        }
        let _ = render_once(&ctx_min); // warm-up
        let mut times_min: Vec<u128> = (0..N).map(|_| render_once(&ctx_min)).collect();
        times_min.sort_unstable();

        let p50 = |v: &[u128]| v[v.len() / 2];
        let p95 = |v: &[u128]| v[v.len() * 95 / 100];
        let p99 = |v: &[u128]| v[v.len() * 99 / 100];

        eprintln!("\n=== system fonts ({} families) ===", family_count);
        eprintln!(
            "  p50={:>6}µs  p95={:>6}µs  p99={:>6}µs",
            p50(&times_sys),
            p95(&times_sys),
            p99(&times_sys)
        );
        eprintln!("=== minimal fonts (3 families)  ===");
        eprintln!(
            "  p50={:>6}µs  p95={:>6}µs  p99={:>6}µs",
            p50(&times_min),
            p95(&times_min),
            p99(&times_min)
        );
        eprintln!(
            "speedup p50: {:.1}×",
            times_sys[N / 2] as f64 / times_min[N / 2].max(1) as f64
        );
    }
}
