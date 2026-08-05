use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use cached::macros::cached;
use cached::Cached;
use takumi::prelude::{
    FontResource, Fonts, GenericFamily, ImageSource, MeasuredNode, Node, RenderOptions, Viewport,
};
use takumi::{measure as takumi_measure_layout, render};

use crate::config::FontConfig;
use crate::layout::parse_layout;

/// Everything a render needs that outlives a single frame.
///
/// takumi has no global context of its own: fonts live in a [`Fonts`] handle and
/// decoded images are handed to each render individually, so tauler owns both and
/// passes them down per frame.
#[derive(Default)]
pub struct RenderContext {
    pub fonts: Fonts,
    /// Pre-decoded images keyed by the `src` a node references them by.
    pub images: HashMap<Arc<str>, ImageSource>,
}

static GLOBAL_CTX: OnceLock<Mutex<RenderContext>> = OnceLock::new();

/// Initialize the global rendering context. Must be called once at startup.
/// Loads fonts into the context before storing it.
pub fn init_global_ctx(font_config: FontConfig) {
    let mut ctx = RenderContext::default();
    rebuild_font_context(&mut ctx, &font_config);
    GLOBAL_CTX.set(Mutex::new(ctx)).ok();
}

/// Rebuild `ctx`'s fonts from scratch for `config`.
///
/// Registration order *is* priority order: [`Fonts::register`] appends to a
/// generic family's list and nothing can replace or remove an entry afterwards.
/// So the configured font is registered before the fontconfig defaults it is
/// meant to override, not after. The symbol fallback is exempt — it registers as
/// a last resort, which sorts behind every normal family however early it lands.
pub(crate) fn rebuild_font_context(ctx: &mut RenderContext, config: &FontConfig) -> FontLoad {
    ctx.fonts = Fonts::default();
    apply_font_config(&mut ctx.fonts, config);
    let load = load_targeted_fonts(&mut ctx.fonts);
    append_symbol_fallback(&mut ctx.fonts);
    load
}

pub fn with_global_ctx<F, R>(f: F) -> R
where
    F: FnOnce(&RenderContext) -> R,
{
    let g = GLOBAL_CTX
        .get()
        .expect("call init_global_ctx before rendering")
        .lock()
        .unwrap();
    f(&g)
}

/// As [`with_global_ctx`], for the paths that add images to the context.
pub fn with_global_ctx_mut<F, R>(f: F) -> R
where
    F: FnOnce(&mut RenderContext) -> R,
{
    let mut g = GLOBAL_CTX
        .get()
        .expect("call init_global_ctx before rendering")
        .lock()
        .unwrap();
    f(&mut g)
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
        let options = frame_options(global, node, width, height, dpr);
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
        let node = layout.unwrap_or_else(|| Node::container(vec![]));
        let options = frame_options(global, node, width, height, dpr);
        let rgba = render(options).expect("render").into_raw();
        Arc::new(rgba)
    })
}

/// The options every frame is rendered or measured with: the shared fonts and
/// decoded images, at `width × height` physical pixels with `dpr` scaling CSS `px`.
///
/// Cloning `images` is a per-entry `Arc` bump, not a pixel copy.
fn frame_options(
    ctx: &RenderContext,
    node: Node,
    width: u32,
    height: u32,
    dpr: f32,
) -> RenderOptions<'_> {
    RenderOptions::builder()
        .fonts(&ctx.fonts)
        .images(ctx.images.clone())
        .viewport(Viewport::new((Some(width), Some(height))).with_device_pixel_ratio(dpr))
        .node(node)
        .build()
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

/// Register each file's faces, so every weight and style resolves to a real font
/// rather than a synthesised one. Reports whether any face was actually taken.
///
/// A file that cannot be read or decoded is skipped rather than fatal: font sets
/// are discovered from the system, so one bad entry must not cost us the rest.
fn register_files(
    fonts: &mut Fonts,
    files: &[PathBuf],
    generic: Option<GenericFamily>,
    last_resort: bool,
) -> bool {
    let mut registered = false;
    for path in files {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let mut resource = FontResource::new(bytes);
        if let Some(generic) = generic {
            resource = resource.generic_family(generic);
        }
        if last_resort {
            resource = resource.last_resort();
        }
        match fonts.register(resource) {
            Ok(families) => registered |= !families.is_empty(),
            Err(error) => {
                tracing::warn!(path = %path.display(), ?error, "could not register font")
            }
        }
    }
    registered
}

/// Register every face of `family` under `generic`.
fn register_family(fonts: &mut Fonts, family: &str, generic: Option<GenericFamily>) -> bool {
    register_files(fonts, &matching_files(family), generic, false)
}

/// Register the configured fonts. Runs before [`load_targeted_fonts`] so that it
/// takes precedence — see [`rebuild_font_context`] on why order is priority.
pub(crate) fn apply_font_config(fonts: &mut Fonts, config: &FontConfig) {
    // `primary_path` names one exact file; `primary` names a family, which is
    // registered with all of its faces.
    if let Some(path) = &config.primary_path {
        register_files(
            fonts,
            std::slice::from_ref(path),
            Some(GenericFamily::SANS_SERIF),
            false,
        );
    } else if let Some(primary) = config.primary.as_deref() {
        register_family(fonts, primary, Some(GenericFamily::SANS_SERIF));
    }

    if let Some(emoji) = config.emoji.as_deref() {
        register_family(fonts, emoji, Some(GenericFamily::EMOJI));
    }
}

/// Put a symbol font behind every other family. Parley picks a font per cluster
/// by coverage, so this only catches codepoints no other font can render.
fn append_symbol_fallback(fonts: &mut Fonts) {
    // The symbols-only font carries icon ranges alone, so it can never shadow a
    // text glyph; failing that, take whatever covers the probe icon.
    let file = first_matching_file("Symbols Nerd Font Mono")
        .or_else(|| first_matching_file(&format!(":charset={SYMBOL_PROBE_CODEPOINT}")));
    let Some(file) = file else {
        return;
    };
    // Registered as a last resort, which sorts it behind every normal family in
    // fallback selection regardless of when it was registered.
    register_files(fonts, std::slice::from_ref(&file), None, true);
}

/// Which font set `load_targeted_fonts` ended up installing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontLoad {
    /// fontconfig resolved the generic families; the font set stays tiny.
    Targeted,
    /// No fontconfig, so every system font was registered instead.
    SystemFallback,
}

/// Register the few families the bar draws with and map each to its generic.
pub fn load_targeted_fonts(fonts: &mut Fonts) -> FontLoad {
    let mut loaded_any = false;
    for (generic, alias) in [
        (GenericFamily::SANS_SERIF, "sans-serif"),
        (GenericFamily::MONOSPACE, "monospace"),
        (GenericFamily::EMOJI, "emoji"),
    ] {
        let Some(family) = generic_family(alias) else {
            continue;
        };
        loaded_any |= register_family(fonts, &family, Some(generic));
    }

    // Where there is no fontconfig — macOS, a bare container — fall back to every
    // font on the system: slower to start and per render, but it renders.
    if !loaded_any {
        register_system_fonts(fonts);
        return FontLoad::SystemFallback;
    }
    FontLoad::Targeted
}

/// Font directories searched when fontconfig is unavailable.
const SYSTEM_FONT_DIRS: &[&str] = &[
    "/System/Library/Fonts",
    "/Library/Fonts",
    "/usr/share/fonts",
    "/usr/local/share/fonts",
];

/// As [`SYSTEM_FONT_DIRS`], relative to the user's home directory.
const HOME_FONT_DIRS: &[&str] = &["Library/Fonts", ".local/share/fonts", ".fonts"];

/// Extensions parley can decode from a plain byte blob.
const FONT_EXTENSIONS: &[&str] = &["ttf", "otf", "ttc", "otc"];

/// Register every font in the platform's font directories.
///
/// takumi has no notion of a system font source — a font is invisible until it is
/// registered from its bytes — so the directories are walked here. Only reached
/// when fontconfig answered nothing, where the alternative is rendering no text.
fn register_system_fonts(fonts: &mut Fonts) {
    let mut dirs: Vec<PathBuf> = SYSTEM_FONT_DIRS.iter().map(PathBuf::from).collect();
    if let Some(home) = std::env::var_os("HOME") {
        dirs.extend(HOME_FONT_DIRS.iter().map(|dir| Path::new(&home).join(dir)));
    }

    let mut files = Vec::new();
    for dir in &dirs {
        collect_font_files(dir, &mut files);
    }
    register_files(fonts, &files, None, false);
}

/// Every font file at or below `dir`. A missing or unreadable directory yields
/// nothing — the list is a union of candidates, most of which will not exist.
fn collect_font_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_font_files(&path, out);
        } else if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| FONT_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
        {
            out.push(path);
        }
    }
}

pub fn preload_layout_images(layout: &serde_json::Value) {
    with_global_ctx_mut(|global| preload_layout_images_impl(layout, global));
}

fn preload_layout_images_impl(layout: &serde_json::Value, global: &mut RenderContext) {
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
                global.images.insert(src.into(), image);
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
        let options = frame_options(global, node, width, height, dpr);
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
    use super::{
        apply_font_config, init_global_ctx, render_frame, FontConfig, Fonts, RenderContext,
    };
    use std::sync::Arc;

    // -----------------------------------------------------------------------
    // Font coverage: the registered set is deliberately tiny (see
    // load_targeted_fonts), so these tests guard the things that smallness can
    // silently break — symbol/PUA fallback, real weight faces, and honouring the
    // configured family.
    //
    // They assert on rendered output rather than on the font set itself: takumi
    // exposes no way to enumerate what a `Fonts` holds, and pixels are the
    // behaviour we actually care about.
    // -----------------------------------------------------------------------

    /// A nerd-font glyph (Private Use Area) — no normal text font covers these.
    const NERD_HOME: &str = "\u{f015}";
    const NERD_FOLDER: &str = "\u{e5ff}";

    fn ink(pixels: &[u8]) -> usize {
        pixels.chunks_exact(4).filter(|px| px[3] != 0).count()
    }

    /// Ink for `text` rendered against an explicit font set. Tests can't rely on
    /// the process-global GLOBAL_CTX: it is a OnceLock shared by every test in
    /// this binary, so whichever test runs first fixes its font config.
    fn ink_of(fonts: &Fonts, text: &str, weight: u32) -> usize {
        use takumi::prelude::{RenderOptions, Viewport};
        use takumi::render;

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
            .fonts(fonts)
            .viewport(Viewport::new((Some(120), Some(48))))
            .node(node)
            .build();
        ink(&render(options).expect("render").into_raw())
    }

    /// Only the fontconfig-resolved generics, as `load_targeted_fonts` installs them.
    fn targeted_fonts() -> Fonts {
        let mut fonts = Fonts::default();
        super::load_targeted_fonts(&mut fonts);
        fonts
    }

    /// The full set the bar renders with, exactly as startup builds it.
    fn fonts_for(config: &FontConfig) -> Fonts {
        let mut ctx = RenderContext::default();
        super::rebuild_font_context(&mut ctx, config);
        ctx.fonts
    }

    fn config_with_primary(primary: &str) -> FontConfig {
        FontConfig {
            primary: Some(primary.to_string()),
            emoji: None,
            primary_path: None,
        }
    }

    /// True when two different PUA codepoints render as two different shapes.
    /// A missing-glyph box is identical for every codepoint, so equal ink means
    /// both fell back to tofu.
    fn renders_distinct_symbol_glyphs(fonts: &Fonts) -> bool {
        let home = ink_of(fonts, NERD_HOME, 400);
        let folder = ink_of(fonts, NERD_FOLDER, 400);
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
        let fonts = fonts_for(&FontConfig::default());
        assert!(
            renders_distinct_symbol_glyphs(&fonts),
            "U+F015 and U+E5FF rendered identically (or blank) — the targeted font \
             set has no symbol fallback, so PUA glyphs come out as tofu"
        );
    }

    #[test]
    fn targeted_fonts_provide_distinct_regular_and_bold_faces() {
        let fonts = targeted_fonts();
        let regular = ink_of(&fonts, "ABC", 400);
        if regular == 0 {
            eprintln!("SKIP: no fonts loaded (fontconfig unavailable?)");
            return;
        }
        let bold = ink_of(&fonts, "ABC", 700);
        assert_ne!(
            regular, bold,
            "weight 400 and 700 rendered identically — only one face of the family \
             was registered, so font-weight has no effect"
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
        let default_fonts = fonts_for(&FontConfig::default());
        let configured = fonts_for(&config_with_primary(&family));

        assert!(
            ink_of(&configured, "ABC", 400) > 0,
            "configured primary family {family:?} was never registered"
        );
        assert_ne!(
            ink_of(&default_fonts, "ABC", 400),
            ink_of(&configured, "ABC", 400),
            "text renders identically after switching primary to {family:?} — \
             the configured family is being ignored"
        );
    }

    /// Guards the choice of `fc-list` over `fc-match` for family lookups:
    /// `fc-match` answers an unknown name with an arbitrary substitute.
    #[test]
    fn unknown_primary_family_name_is_ignored_not_silently_substituted() {
        let fonts = fonts_for(&FontConfig::default());
        let before = ink_of(&fonts, "ABC", 400);
        if before == 0 {
            eprintln!("SKIP: no fonts loaded (fontconfig unavailable?)");
            return;
        }

        let unknown = fonts_for(&config_with_primary("Totally Fake Font XYZ"));

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
        let mut ctx = RenderContext::default();
        super::rebuild_font_context(&mut ctx, &FontConfig::default());
        let before = ink_of(&ctx.fonts, "ABC", 400);
        if before == 0 {
            eprintln!("SKIP: no fonts loaded (fontconfig unavailable?)");
            return;
        }

        super::rebuild_font_context(&mut ctx, &config_with_primary(&family));

        assert_ne!(
            before,
            ink_of(&ctx.fonts, "ABC", 400),
            "reloading the font config had no visible effect — the new fonts never \
             replaced the old ones"
        );
        assert!(
            renders_distinct_symbol_glyphs(&ctx.fonts) || !any_font_covers("f015"),
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
        let Some(family) = installed_family(&["Noto Color Emoji", "Apple Color Emoji"]) else {
            eprintln!("SKIP: no colour emoji font found on this system");
            return;
        };
        let mut fonts = Fonts::default();
        apply_font_config(
            &mut fonts,
            &FontConfig {
                emoji: Some(family.clone()),
                primary: None,
                primary_path: None,
            },
        );
        assert!(
            ink_of(&fonts, "👋", 400) > 0,
            "an emoji drew nothing after {family:?} was configured — the emoji \
             generic family was never mapped"
        );
    }

    #[test]
    fn apply_font_config_maps_sans_serif_generic_family_when_primary_font_is_present() {
        let Some(family) = installed_family(&["Adwaita Sans", "DejaVu Sans", "Liberation Sans"])
        else {
            eprintln!("SKIP: none of the candidate sans-serif fonts are installed");
            return;
        };
        let mut fonts = Fonts::default();
        apply_font_config(&mut fonts, &config_with_primary(&family));
        assert!(
            ink_of(&fonts, "ABC", 400) > 0,
            "text drew nothing with only {family:?} registered — the sans-serif \
             generic family was never mapped"
        );
    }

    /// `Fonts::register` appends and nothing can replace an entry, so switching
    /// the primary font only takes effect through a *rebuild*. This guards that
    /// `rebuild_font_context` really does start from an empty set.
    #[test]
    fn rebuilding_with_a_different_primary_font_remaps_sans_serif() {
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

        let mut ctx = RenderContext::default();
        super::rebuild_font_context(&mut ctx, &config_with_primary(first_family));
        let first_ink = ink_of(&ctx.fonts, "ABC", 400);

        super::rebuild_font_context(&mut ctx, &config_with_primary(second_family));
        let second_ink = ink_of(&ctx.fonts, "ABC", 400);

        assert!(first_ink > 0, "{first_family} should map sans-serif");
        assert_ne!(
            first_ink, second_ink,
            "rebuilding with {second_family} should remap sans-serif away from {first_family}"
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

        let first_ink = super::with_global_ctx(|ctx| ink_of(&ctx.fonts, "ABC", 400));
        if first_ink == 0 {
            eprintln!("SKIP: first font not mapped");
            return;
        }

        super::reload_font_config(FontConfig {
            primary: None,
            emoji: None,
            primary_path: Some(second_path),
        });

        let second_ink = super::with_global_ctx(|ctx| ink_of(&ctx.fonts, "ABC", 400));
        assert!(second_ink > 0);
        assert_ne!(first_ink, second_ink);
    }

    /// The old form of this test counted families in the collection to prove we
    /// had not accidentally loaded everything. takumi no longer exposes that, but
    /// `FontLoad` reports the same fact directly: `Targeted` means the fontconfig
    /// path ran and `register_system_fonts` was never reached.
    #[test]
    fn load_targeted_fonts_stays_targeted_and_maps_sans_serif() {
        let mut fonts = Fonts::default();
        let load = super::load_targeted_fonts(&mut fonts);

        if load == super::FontLoad::SystemFallback {
            // Without fontconfig — macOS, a bare container — registering every
            // system font is the documented fallback, not a failure.
            assert!(
                ink_of(&fonts, "ABC", 400) > 0,
                "the system-font fallback must leave a usable font set"
            );
            return;
        }

        assert_eq!(load, super::FontLoad::Targeted);
        assert!(
            ink_of(&fonts, "ABC", 400) > 0,
            "load_targeted_fonts must map the sans-serif generic to a real font"
        );
    }

    #[test]
    fn bench_system_fonts_vs_minimal() {
        use crate::layout::parse_layout;
        use std::time::Instant;
        use takumi::prelude::{RenderOptions, Viewport};
        use takumi::render;

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

        let render_once = |fonts: &Fonts| -> u128 {
            let opts = RenderOptions::builder()
                .fonts(fonts)
                .viewport(Viewport::new((Some(364), Some(2159))))
                .node(node.clone())
                .build();
            let t = Instant::now();
            let _ = render(opts).expect("render");
            t.elapsed().as_micros()
        };

        // --- baseline: every system font ---
        // Registration cost matters on its own here: this is the no-fontconfig
        // startup path, and it reads every font file's bytes.
        let mut fonts_sys = Fonts::default();
        let t = Instant::now();
        super::register_system_fonts(&mut fonts_sys);
        eprintln!("\nregister_system_fonts: {}µs", t.elapsed().as_micros());
        let _ = render_once(&fonts_sys); // warm-up
        let mut times_sys: Vec<u128> = (0..N).map(|_| render_once(&fonts_sys)).collect();
        times_sys.sort_unstable();

        // --- candidate: minimal curated fonts ---
        let mut fonts_min = Fonts::default();
        for path in [&sans, &mono, &emoji] {
            super::register_files(&mut fonts_min, std::slice::from_ref(path), None, false);
        }
        let _ = render_once(&fonts_min); // warm-up
        let mut times_min: Vec<u128> = (0..N).map(|_| render_once(&fonts_min)).collect();
        times_min.sort_unstable();

        let p50 = |v: &[u128]| v[v.len() / 2];
        let p95 = |v: &[u128]| v[v.len() * 95 / 100];
        let p99 = |v: &[u128]| v[v.len() * 99 / 100];

        eprintln!("\n=== system fonts ===");
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
