use std::sync::{Arc, Mutex, OnceLock};

use cached::macros::cached;
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

/// Initialize the global rendering context. Must be called once at startup.
/// Loads fonts into the context before storing it.
pub fn init_global_ctx(font_config: FontConfig) {
    let mut ctx = GlobalContext::default();
    rebuild_font_context(&mut ctx, &font_config);
    GLOBAL_CTX.set(Mutex::new(ctx)).ok();
}

/// Rebuild `ctx`'s fonts from scratch for `config`.
///
/// The context is *replaced* rather than mutated in place: takumi memoises a
/// clone of the font context per thread, keyed by its id and a version counter
/// that only its own loader bumps (`FontContext::load_and_store`). Mutating the
/// collection through `DerefMut`, as we do, leaves that counter untouched, so
/// every thread that has already rendered would keep using its stale clone and
/// never see the new fonts. A fresh context carries a fresh id, which misses
/// those caches by construction.
pub(crate) fn rebuild_font_context(ctx: &mut GlobalContext, config: &FontConfig) {
    ctx.font_context = Default::default();
    let _ = load_targeted_fonts(ctx);
    apply_font_config(ctx, config);
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
        rebuild_font_context(&mut ctx, &font_config);
        RENDER_FRAME_CACHED.write().cache_clear();
        MEASURE_LAYOUT_CACHED.write().cache_clear();
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

#[cached(max_size = 6)]
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
        let t = std::time::Instant::now();
        let rgba = render(options).expect("render").into_raw();
        tracing::debug!(full_render_us = t.elapsed().as_micros(), "full render");
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

// ---------------------------------------------------------------------------
// Font resolution.
//
// takumi hands us a collection with no system font source, so a font is unusable
// until its files are loaded explicitly. Loading every system family makes each
// render several times slower, so we ask fontconfig for the few files we need.
// ---------------------------------------------------------------------------

/// A nerd-font icon. These live in the Private Use Area, which no text font
/// covers, so they render only if a symbol font is in the fallback chain.
const SYMBOL_PROBE_CODEPOINT: &str = "f015";

fn fc_query(program: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(program)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Font files matching `pattern` — a family name, or a constraint such as
/// `:charset=f015`. Matching is strict: an unknown family yields nothing, where
/// `fc-match` would answer with an arbitrary substitute.
fn matching_files(pattern: &str) -> Vec<std::path::PathBuf> {
    let Some(listing) = fc_query("fc-list", &[pattern, "file"]) else {
        return Vec::new();
    };
    listing
        .lines()
        .filter_map(|line| line.split(':').next())
        .map(|path| std::path::PathBuf::from(path.trim()))
        .filter(|path| path.exists())
        .collect()
}

fn first_matching_file(pattern: &str) -> Option<std::path::PathBuf> {
    matching_files(pattern).into_iter().next()
}

/// The family behind a generic alias like `sans-serif`. Substitution is the
/// point here, which is why this one asks `fc-match`; `fc-list` does not resolve
/// aliases at all.
fn generic_family(alias: &str) -> Option<String> {
    fc_query("fc-match", &["--format", "%{family[0]}", alias])
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
}

/// Id of the family owning the font file at `path`.
fn family_id_for_path(
    collection: &mut parley::fontique::Collection,
    path: &std::path::Path,
) -> Option<parley::fontique::FamilyId> {
    use parley::fontique::SourceKind;
    let names: Vec<String> = collection.family_names().map(|s| s.to_string()).collect();
    for name in &names {
        let Some(info) = collection.family_by_name(name) else {
            continue;
        };
        let owns_path = info
            .fonts()
            .iter()
            .any(|font| matches!(&font.source().kind, SourceKind::Path(p) if p.as_ref() == path));
        if owns_path {
            return Some(info.id());
        }
    }
    None
}

/// Load every face of `family`, so each weight and style resolves to a real font
/// rather than a synthesised one.
fn load_family(
    collection: &mut parley::fontique::Collection,
    family: &str,
) -> Option<parley::fontique::FamilyId> {
    let files = matching_files(family);
    let first = files.first()?.clone();
    collection.load_fonts_from_paths(files.iter());
    family_id_for_path(collection, &first)
}

pub(crate) fn apply_font_config(ctx: &mut GlobalContext, config: &FontConfig) {
    let collection = &mut ctx.font_context.collection;

    if let Some(id) = configured_primary(collection, config) {
        collection.set_generic_families(GenericFamily::SansSerif, std::iter::once(id));
    }
    if let Some(id) = config
        .emoji
        .as_deref()
        .and_then(|family| load_family(collection, family))
    {
        collection.set_generic_families(GenericFamily::Emoji, std::iter::once(id));
    }

    // Last: `set_generic_families` replaces a generic's family list, so a
    // fallback appended before those calls would be dropped again.
    append_symbol_fallback(collection);
}

/// The configured primary family. `primary_path` names one exact file;
/// `primary` names a family, which is loaded with all of its faces.
fn configured_primary(
    collection: &mut parley::fontique::Collection,
    config: &FontConfig,
) -> Option<parley::fontique::FamilyId> {
    if let Some(path) = &config.primary_path {
        collection.load_fonts_from_paths(std::iter::once(path));
        return family_id_for_path(collection, path);
    }
    load_family(collection, config.primary.as_deref()?)
}

/// Put a symbol font at the end of the family list for the generics that text
/// resolves through. Parley picks a font per cluster by coverage, so this only
/// catches codepoints the primary font cannot render.
fn append_symbol_fallback(collection: &mut parley::fontique::Collection) {
    // The symbols-only font carries icon ranges alone, so it can never shadow a
    // text glyph; failing that, take whatever covers the probe icon.
    let file = first_matching_file("Symbols Nerd Font Mono")
        .or_else(|| first_matching_file(&format!(":charset={SYMBOL_PROBE_CODEPOINT}")));
    let Some(file) = file else {
        return;
    };
    collection.load_fonts_from_paths(std::iter::once(&file));
    let Some(id) = family_id_for_path(collection, &file) else {
        return;
    };
    for generic in [GenericFamily::SansSerif, GenericFamily::Monospace] {
        collection.append_generic_families(generic, std::iter::once(id));
    }
}

/// Load the few families the bar draws with and map each to its generic.
/// Which font set `load_targeted_fonts` ended up installing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontLoad {
    /// fontconfig resolved the generic families; the collection stays tiny.
    Targeted,
    /// No fontconfig, so the whole system collection was loaded instead.
    SystemFallback,
}

pub fn load_targeted_fonts(ctx: &mut GlobalContext) -> FontLoad {
    let collection = &mut ctx.font_context.collection;
    let mut loaded_any = false;
    for (generic, alias) in [
        (GenericFamily::SansSerif, "sans-serif"),
        (GenericFamily::Monospace, "monospace"),
        (GenericFamily::Emoji, "emoji"),
    ] {
        let Some(id) = generic_family(alias).and_then(|family| load_family(collection, &family))
        else {
            continue;
        };
        collection.set_generic_families(generic, std::iter::once(id));
        loaded_any = true;
    }

    // Where there is no fontconfig — macOS, a bare container — fall back to the
    // whole system collection: slower per render, but it renders.
    if !loaded_any {
        collection.load_system_fonts();
        return FontLoad::SystemFallback;
    }
    FontLoad::Targeted
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
#[cached(max_size = 6)]
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

    // -----------------------------------------------------------------------
    // Font coverage: the collection is deliberately tiny (see load_targeted_fonts),
    // so these tests guard the things that smallness can silently break —
    // symbol/PUA fallback, real weight faces, and honouring the configured family.
    // -----------------------------------------------------------------------

    /// A nerd-font glyph (Private Use Area) — no normal text font covers these.
    const NERD_HOME: &str = "\u{f015}";
    const NERD_FOLDER: &str = "\u{e5ff}";

    fn ink(pixels: &[u8]) -> usize {
        pixels.chunks_exact(4).filter(|px| px[3] != 0).count()
    }

    /// Ink for `text` rendered against an explicit context. Tests can't rely on
    /// the process-global GLOBAL_CTX: it is a OnceLock shared by every test in
    /// this binary, so whichever test runs first fixes its font config.
    fn ink_of(ctx: &takumi::GlobalContext, text: &str, weight: u32) -> usize {
        use takumi::{
            layout::Viewport,
            rendering::{render, RenderOptions},
        };
        let content = serde_json::json!({
            "type": "container",
            "style": {"width": 120, "height": 48},
            "children": [{
                "type": "text",
                "text": text,
                "style": {"fontSize": 28, "color": "white", "fontWeight": weight}
            }]
        });
        let node = crate::layout::parse_layout(&content).expect("probe layout should parse");
        let options = RenderOptions::builder()
            .global(ctx)
            .viewport(Viewport::new((Some(120), Some(48))))
            .node(node)
            .build();
        ink(&render(options).expect("render").into_raw())
    }

    fn targeted_ctx() -> takumi::GlobalContext {
        let mut ctx = takumi::GlobalContext::default();
        super::load_targeted_fonts(&mut ctx);
        ctx
    }

    /// True when two different PUA codepoints render as two different shapes.
    /// A missing-glyph box is identical for every codepoint, so equal ink means
    /// both fell back to tofu.
    fn renders_distinct_symbol_glyphs(ctx: &takumi::GlobalContext) -> bool {
        let home = ink_of(ctx, NERD_HOME, 400);
        let folder = ink_of(ctx, NERD_FOLDER, 400);
        home > 0 && folder > 0 && home != folder
    }

    /// Ask fontconfig whether any installed font actually covers a codepoint.
    /// `fc-list` filters strictly on charset (unlike `fc-match`, which always
    /// answers with *something*).
    ///
    /// Dot-prefixed families are skipped: macOS's `.LastResort` claims to cover
    /// everything but draws one placeholder box for every codepoint.
    fn any_font_covers(codepoint_hex: &str) -> bool {
        std::process::Command::new("fc-list")
            .args([&format!(":charset={codepoint_hex}"), "family"])
            .output()
            .map(|o| {
                String::from_utf8_lossy(&o.stdout).lines().any(|family| {
                    !family.trim().is_empty() && !family.trim_start().starts_with('.')
                })
            })
            .unwrap_or(false)
    }

    /// First of `candidates` that fontconfig reports as genuinely installed.
    fn installed_family(candidates: &[&str]) -> Option<String> {
        candidates
            .iter()
            .find(|family| {
                std::process::Command::new("fc-list")
                    .args([family, "family"])
                    .output()
                    .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
                    .unwrap_or(false)
            })
            .map(|f| (*f).to_string())
    }

    #[test]
    fn symbol_glyphs_outside_the_targeted_fonts_still_render() {
        if !any_font_covers("f015") {
            eprintln!("SKIP: no installed font covers U+F015 (no nerd font on this system)");
            return;
        }
        let mut ctx = targeted_ctx();
        apply_font_config(&mut ctx, &FontConfig::default());
        assert!(
            renders_distinct_symbol_glyphs(&ctx),
            "U+F015 and U+E5FF rendered identically (or blank) — the targeted font \
             collection has no symbol fallback, so PUA glyphs come out as tofu"
        );
    }

    #[test]
    fn targeted_fonts_provide_distinct_regular_and_bold_faces() {
        let ctx = targeted_ctx();
        let regular = ink_of(&ctx, "ABC", 400);
        if regular == 0 {
            eprintln!("SKIP: no fonts loaded (fontconfig unavailable?)");
            return;
        }
        let bold = ink_of(&ctx, "ABC", 700);
        assert_ne!(
            regular, bold,
            "weight 400 and 700 rendered identically — only one face of the family \
             was loaded, so font-weight has no effect"
        );
    }

    #[test]
    fn configured_primary_family_name_is_applied() {
        let Some(family) =
            installed_family(&["JetBrains Mono", "Liberation Serif", "DejaVu Serif"])
        else {
            eprintln!("SKIP: none of the candidate primary fonts are installed");
            return;
        };
        let mut default_ctx = takumi::GlobalContext::default();
        super::rebuild_font_context(&mut default_ctx, &FontConfig::default());

        let mut configured = takumi::GlobalContext::default();
        super::rebuild_font_context(
            &mut configured,
            &FontConfig {
                primary: Some(family.clone()),
                emoji: None,
                primary_path: None,
            },
        );
        assert!(
            configured
                .font_context
                .collection
                .family_by_name(&family)
                .is_some(),
            "configured primary family {family:?} was never loaded into the collection"
        );
        assert_ne!(
            ink_of(&default_ctx, "ABC", 400),
            ink_of(&configured, "ABC", 400),
            "text renders identically after switching primary to {family:?} — \
             the configured family is being ignored"
        );
    }

    /// Guards the choice of `fc-list` over `fc-match` for family lookups:
    /// `fc-match` answers an unknown name with an arbitrary substitute.
    #[test]
    fn unknown_primary_family_name_is_ignored_not_silently_substituted() {
        let mut ctx = takumi::GlobalContext::default();
        super::rebuild_font_context(&mut ctx, &FontConfig::default());
        let before = ink_of(&ctx, "ABC", 400);
        if before == 0 {
            eprintln!("SKIP: no fonts loaded (fontconfig unavailable?)");
            return;
        }

        let mut unknown = takumi::GlobalContext::default();
        super::rebuild_font_context(
            &mut unknown,
            &FontConfig {
                primary: Some("Totally Fake Font XYZ".to_string()),
                emoji: None,
                primary_path: None,
            },
        );

        assert_eq!(
            before,
            ink_of(&unknown, "ABC", 400),
            "an unknown family name must leave rendering untouched"
        );
    }

    #[test]
    fn reloading_the_font_config_applies_it_and_keeps_symbol_glyphs() {
        let Some(family) =
            installed_family(&["JetBrains Mono", "Liberation Serif", "DejaVu Serif"])
        else {
            eprintln!("SKIP: none of the candidate primary fonts are installed");
            return;
        };
        let mut ctx = takumi::GlobalContext::default();
        super::rebuild_font_context(&mut ctx, &FontConfig::default());
        // Rendering first populates takumi's per-thread font-context cache, which
        // is what a reload has to invalidate.
        let before = ink_of(&ctx, "ABC", 400);
        if before == 0 {
            eprintln!("SKIP: no fonts loaded (fontconfig unavailable?)");
            return;
        }

        super::rebuild_font_context(
            &mut ctx,
            &FontConfig {
                primary: Some(family.clone()),
                emoji: None,
                primary_path: None,
            },
        );

        assert_ne!(
            before,
            ink_of(&ctx, "ABC", 400),
            "reloading the font config had no visible effect — the new fonts were \
             masked by takumi's cached clone of the font context"
        );
        assert!(
            renders_distinct_symbol_glyphs(&ctx) || !any_font_covers("f015"),
            "the symbol fallback was lost when the font config was reloaded"
        );
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

    fn sans_serif_id_for_primary(
        ctx: &mut takumi::GlobalContext,
        primary: &str,
    ) -> Option<parley::fontique::FamilyId> {
        apply_font_config(
            ctx,
            &FontConfig {
                primary: Some(primary.to_string()),
                emoji: None,
                primary_path: None,
            },
        );
        ctx.font_context
            .collection
            .generic_families(parley::GenericFamily::SansSerif)
            .next()
    }

    #[test]
    fn apply_font_config_updates_sans_serif_mapping_when_called_twice_with_different_primary_font()
    {
        // An unknown family leaves the previous mapping untouched, so both
        // candidates must actually be installed or this compares a value to itself.
        let installed: Vec<String> = ["Adwaita Sans", "Liberation Serif", "Helvetica", "Georgia"]
            .iter()
            .filter_map(|f| installed_family(&[f]))
            .collect();
        let (Some(first_family), Some(second_family)) = (installed.first(), installed.get(1))
        else {
            eprintln!(
                "SKIP: need two installed candidate families, found {}",
                installed.len()
            );
            return;
        };

        let mut ctx = takumi::GlobalContext::default();
        let first_id = sans_serif_id_for_primary(&mut ctx, first_family);
        let second_id = sans_serif_id_for_primary(&mut ctx, second_family);

        assert!(first_id.is_some(), "{first_family} should map sans-serif");
        assert_ne!(
            first_id, second_id,
            "re-applying the config with {second_family} should remap sans-serif away from {first_family}"
        );
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
        let load = super::load_targeted_fonts(&mut ctx);

        let count = ctx.font_context.collection.family_names().count();

        match load {
            // Without fontconfig — macOS, a bare container — loading the whole
            // system collection is the documented fallback, not a failure.
            super::FontLoad::SystemFallback => assert!(
                count > 0,
                "the system-font fallback must leave a usable collection"
            ),
            super::FontLoad::Targeted => assert!(
                count < 20,
                "load_targeted_fonts should load only a small targeted set, got {count} families"
            ),
        }

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
