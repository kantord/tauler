//! Pictures: the pixels of one D3Figure's drawing ([`crate::d3_figure`]), kept
//! under the figure's key and placed with `<img src="tauler:picture/<key>">`.
//! Layout files never name a Picture; they write `<D3Figure>`.
//!
//! The registry holds each Picture's SVG markup, as the Draw runtime sent it, with
//! a generation of its own, taken when it was stored. Markup never travels through the tree
//! — a short key does — because as an SVG data URI it would cost eval time on
//! every Tick and a re-parse on every repaint (ADR 0044 has the numbers).
//!
//! # Binding
//!
//! For every frame, `bind` (called from `frame_options`) binds a bitmap under
//! the full src of each Picture the content names, the way `tauler:root-bg` is
//! bound. The bitmap is tauler's own renderer drawing the SVG as an `<img>` at the
//! `<img>`'s style size × dpr, rasterized once per (Picture, physical width,
//! height) and kept in a raster cache that drops a bitmap as soon as its Picture
//! is replaced or removed. A key with no Picture binds nothing, and takumi draws
//! nothing there — no panic, no log per frame. Markup that is not an SVG document
//! — a `draw` is the author's code — draws nothing the same way: a broken figure
//! costs its own pixels, never the renderer.
//!
//! # Fallbacks
//!
//! A key with no Picture of its own yet can show another key's ([`set_fallback`])
//! — in binding and in `frame_stamp` alike — until [`set_picture`] gives it its
//! own. This is how a D3Figure whose key changed keeps showing its previous
//! Picture. [`retain_pictures`] keeps a Picture that a kept key shows as its
//! fallback, and drops the bitmaps of what it drops.
//!
//! # The frame cache
//!
//! The tree is the same before and after a Picture arrives; the pixels are not.
//! So the frame cache (`render/cache.rs`) widens its key by `frame_stamp`: a
//! hash of what each key the content names resolves to — the key itself or its
//! fallback, and that Picture's entry generation. A frame misses exactly when one
//! of *its* Pictures appears, changes or goes; a frame that names none is never
//! drawn again on a Picture's account.
//!
//! # Why a static
//!
//! The registry is its own process-wide static, not a field of `RenderContext`.
//! A `RenderContext` is copy-on-write: storing up to megabytes of markup there
//! would make every `with_global_ctx_mut` clone it, and the raster cache is
//! written *during* a render, which a snapshot cannot be. Consistency with the
//! frame cache comes from ordering instead: the frame key reads the stamp before
//! the frame is drawn, so a stored frame is never older than the stamp it is
//! filed under.
//!
//! # Known limitation
//!
//! A Picture's first rasterization runs on the Render worker, inside whichever
//! frame first names it (12–101 ms for a large map), so it can delay every
//! other surface's next repaint once — noticeable only if a new key lands in the
//! middle of a drag. Moving it to a side thread is the fix if a trace ever shows
//! it on an interactive path (ADR 0042).

use std::collections::HashMap;
use std::fmt::Write;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use takumi::prelude::ImageSource;
use takumi_core::resources::image_buffer::ImageBuffer;

/// The `src` prefix an `<img>` names a Picture by.
pub const PICTURE_SRC_PREFIX: &str = "tauler:picture/";

/// One stored Picture: its markup, and the generation it was stored with.
/// The entry generation is what tells a replaced Picture from its predecessor
/// under the same key — in the frame key and in the raster cache alike.
struct Entry {
    svg: Arc<str>,
    generation: u64,
}

/// The Pictures by key, and the generation the latest one was stored with.
#[derive(Default)]
pub struct Pictures {
    entries: HashMap<String, Entry>,
    /// Keys with no Picture of their own yet, and the key whose Picture they
    /// show meanwhile ([`set_fallback`]).
    fallbacks: HashMap<String, String>,
    /// Increased on every [`Self::set`], so each stored entry has its own.
    generation: u64,
}

impl Pictures {
    /// Store `svg` under `key`, replacing whatever was there, and end any
    /// fallback `key` had.
    pub fn set(&mut self, key: &str, svg: String) {
        self.fallbacks.remove(key);
        self.generation += 1;
        self.entries.insert(
            key.to_string(),
            Entry {
                svg: svg.into(),
                generation: self.generation,
            },
        );
    }

    /// Until `key` has a Picture of its own, show `from`'s wherever `key` is
    /// named. The frame stamp of content naming `key` changes on its own
    /// (`frame_stamp` hashes what the key resolves to).
    ///
    /// When `from` has no Picture yet but shows a fallback of its own (the key
    /// changed twice before the first drawing arrived), `key` takes that
    /// fallback instead, so lookups need only one level.
    pub fn set_fallback(&mut self, key: &str, from: &str) {
        let from = match self.fallbacks.get(from) {
            Some(further) if !self.entries.contains_key(from) => further.clone(),
            _ => from.to_string(),
        };
        if key != from {
            self.fallbacks.insert(key.to_string(), from);
        }
    }

    /// Drop every Picture whose key `keep` rejects, unless a kept key shows it
    /// as its fallback; drop the fallbacks of keys `keep` rejects.
    pub fn retain(&mut self, keep: impl Fn(&str) -> bool) {
        self.fallbacks.retain(|k, _| keep(k));
        let shown: std::collections::HashSet<&str> =
            self.fallbacks.values().map(String::as_str).collect();
        self.entries
            .retain(|k, _| keep(k) || shown.contains(k.as_str()));
    }

    /// The key whose Picture `key` shows: itself when it has one, else its
    /// fallback's when that has one, else `None`.
    ///
    /// One level only: [`Self::set_fallback`] collapses a chain when it is
    /// made, so a fallback is a key with its own Picture or one with none.
    fn resolve<'a>(&'a self, key: &'a str) -> Option<(&'a str, &'a Entry)> {
        if let Some(entry) = self.entries.get(key) {
            return Some((key, entry));
        }
        let from = self.fallbacks.get(key)?;
        self.entries.get(from).map(|e| (from.as_str(), e))
    }

    /// The key whose Picture `key` shows ([`Self::resolve`]), its markup and
    /// its entry generation.
    fn get(&self, key: &str) -> Option<(String, Arc<str>, u64)> {
        self.resolve(key)
            .map(|(k, e)| (k.to_string(), Arc::clone(&e.svg), e.generation))
    }
}

static PICTURES: OnceLock<RwLock<Pictures>> = OnceLock::new();

fn registry() -> &'static RwLock<Pictures> {
    PICTURES.get_or_init(Default::default)
}

/// Store `svg` as the Picture for `key` in the process-wide registry.
pub fn set_picture(key: &str, svg: String) {
    registry().write().unwrap().set(key, svg);
}

/// Until `key` has a Picture of its own, draw `from`'s Picture wherever `key`
/// is named — binding and `frame_stamp` alike. [`set_picture`] for `key`
/// ends it.
///
/// This is how a D3Figure whose key changed shows its previous key's Picture
/// until its own drawing arrives.
///
/// Frames that name `key` miss, because `frame_stamp` hashes the Picture `key`
/// resolves to.
pub fn set_fallback(key: &str, from: &str) {
    registry().write().unwrap().set_fallback(key, from);
}

/// Drop every Picture in the process-wide registry whose key `keep` rejects,
/// except one that a kept key still shows as its fallback ([`set_fallback`]),
/// and the bitmaps of the Pictures it drops with them.
pub fn retain_pictures(keep: impl Fn(&str) -> bool) {
    let mut pictures = registry().write().unwrap();
    pictures.retain(keep);
    drop_stale_rasters(&pictures, &mut rasters().lock().unwrap());
}

/// One `<img>` naming a Picture: the key, and its style size in CSS px when it
/// has one.
pub(crate) struct Named<'a> {
    pub(crate) key: &'a str,
    pub(crate) size: Option<(f64, f64)>,
}

/// Every `<img>` in `content` whose src names a Picture, in tree order.
pub(crate) fn named_pictures(content: &serde_json::Value) -> Vec<Named<'_>> {
    fn walk<'a>(value: &'a serde_json::Value, out: &mut Vec<Named<'a>>) {
        match value {
            serde_json::Value::Object(map) => {
                if map.get("type").and_then(|t| t.as_str()) == Some("img") {
                    let key = map
                        .get("src")
                        .and_then(|s| s.as_str())
                        .and_then(|s| s.strip_prefix(PICTURE_SRC_PREFIX));
                    if let Some(key) = key {
                        let style = map.get("style");
                        let dim = |name: &str| style?.get(name)?.as_f64();
                        let size = dim("width").zip(dim("height"));
                        out.push(Named { key, size });
                    }
                    return; // <img> is terminal
                }
                for v in map.values() {
                    walk(v, out);
                }
            }
            serde_json::Value::Array(arr) => {
                for v in arr {
                    walk(v, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(content, &mut out);
    out
}

/// What the frame cache files `content` under, beyond the tree itself: 0 when
/// it names no Picture, otherwise a hash of the `(key, shown key, entry
/// generation)` of every key it names — the shown key being the key itself or
/// its fallback ([`set_fallback`]), and the generation 0 for a key that shows
/// nothing yet.
///
/// Scoped to the content's own Pictures, so a frame that shows none never
/// misses because some other Picture changed, and a frame misses exactly when
/// one of *its* Pictures appears, changes or disappears.
pub(crate) fn frame_stamp(content: &serde_json::Value) -> u64 {
    let named = named_pictures(content);
    if named.is_empty() {
        return 0;
    }
    let mut keys: Vec<&str> = named.iter().map(|n| n.key).collect();
    keys.sort_unstable();
    keys.dedup();
    let pictures = registry().read().unwrap();
    let mut hasher = DefaultHasher::new();
    for key in keys {
        let shown = pictures.resolve(key).map(|(k, e)| (k, e.generation));
        (key, shown).hash(&mut hasher);
    }
    hasher.finish()
}

/// `(key, entry generation, physical width, physical height)`.
type RasterKey = (String, u64, u32, u32);

/// The bitmaps already rasterized, so a Picture is rasterized once per size.
/// Written during a render, which is why it lives here and not in a snapshot.
static RASTERS: OnceLock<Mutex<HashMap<RasterKey, ImageSource>>> = OnceLock::new();

fn rasters() -> &'static Mutex<HashMap<RasterKey, ImageSource>> {
    RASTERS.get_or_init(Default::default)
}

/// Bind a bitmap for every Picture `content` names into `images`, under its
/// full `tauler:picture/<key>` src, at the `<img>`'s style size × `dpr`.
///
/// A key with no Picture of its own binds its fallback's ([`set_fallback`]);
/// one with neither (or an `<img>` with no numeric style size) is left unbound,
/// and takumi draws nothing there. The raster cache files a fallback's bitmap
/// under the fallback's own key, so it is shared with that key.
pub(super) fn bind(
    content: &serde_json::Value,
    dpr: f32,
    images: &mut HashMap<Arc<str>, ImageSource>,
) {
    let named = named_pictures(content);
    if named.is_empty() {
        return;
    }
    // Binding is by src, so one key holds one bitmap per frame. If the same key
    // is shown at two sizes, keep the largest; takumi scales it into the
    // smaller box, which is good enough.
    let mut wanted: HashMap<&str, (u32, u32, f64, f64)> = HashMap::new();
    for Named { key, size } in named {
        let Some((w, h)) = size else { continue };
        let physical = |css: f64| ((css * f64::from(dpr)).round() as u32).max(1);
        let (pw, ph) = (physical(w), physical(h));
        let entry = wanted.entry(key).or_insert((pw, ph, w, h));
        if u64::from(pw) * u64::from(ph) > u64::from(entry.0) * u64::from(entry.1) {
            *entry = (pw, ph, w, h);
        }
    }
    for (key, (pw, ph, w, h)) in wanted {
        let Some((shown, svg, generation)) = registry().read().unwrap().get(key) else {
            continue;
        };
        let raster_key = (shown, generation, pw, ph);
        let cached = rasters().lock().unwrap().get(&raster_key).cloned();
        let image = match cached {
            Some(image) => image,
            None => {
                let Some(image) = rasterize(&svg, w, h, pw, ph, dpr) else {
                    continue;
                };
                store_raster(raster_key, image.clone());
                image
            }
        };
        images.insert(format!("{PICTURE_SRC_PREFIX}{key}").into(), image);
    }
}

/// Keep `image`, and drop every bitmap whose Picture has since been replaced or
/// removed — that is what bounds the cache to the live Pictures' sizes.
fn store_raster(key: RasterKey, image: ImageSource) {
    let pictures = registry().read().unwrap();
    let mut rasters = rasters().lock().unwrap();
    drop_stale_rasters(&pictures, &mut rasters);
    rasters.insert(key, image);
}

/// Drop every bitmap whose Picture is no longer in `pictures` at the generation
/// it was rasterized from. Callers lock the registry before the raster cache.
fn drop_stale_rasters(pictures: &Pictures, rasters: &mut HashMap<RasterKey, ImageSource>) {
    rasters.retain(|(k, generation, _, _), _| {
        pictures
            .entries
            .get(k)
            .is_some_and(|e| e.generation == *generation)
    });
}

/// `svg` drawn by tauler's own renderer, as an `<img>` of `css_w × css_h` at
/// `pw × ph` physical pixels. The inner content names no Picture, so this does
/// not come back through [`bind`] with anything to do.
fn rasterize(svg: &str, css_w: f64, css_h: f64, pw: u32, ph: u32, dpr: f32) -> Option<ImageSource> {
    let content = serde_json::json!({
        "type": "img",
        "src": format!("data:image/svg+xml;utf8,{}", percent_encode(svg)),
        "style": { "width": css_w, "height": css_h }
    });
    let rgba = super::raster(&content, pw, ph, dpr, None);
    // Only `None` if the buffer is not `pw × ph × 4`, which `raster` guarantees.
    ImageBuffer::from_rgba_bytes(rgba, pw, ph).map(ImageSource::from)
}

/// Percent-encode everything but the characters a data URI body can carry
/// verbatim. Not a dependency of its own: this is the only caller.
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + s.len() / 4);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~!$&'()*+,;=:@/?".contains(&b) {
            out.push(b as char);
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10" fill="#0000ff"/></svg>"##;

    /// The process-wide registry is shared with every other test in this
    /// binary, so this one only touches keys under `stamp-fallback/`.
    #[test]
    fn the_frame_stamp_follows_what_a_key_shows_through_its_fallback_and_then_its_own_picture() {
        let content = serde_json::json!({
            "type": "img",
            "src": format!("{PICTURE_SRC_PREFIX}stamp-fallback/new"),
            "style": { "width": 10, "height": 10 }
        });
        set_picture("stamp-fallback/old", SVG.to_string());
        let showing_nothing = frame_stamp(&content);

        set_fallback("stamp-fallback/new", "stamp-fallback/old");
        let showing_fallback = frame_stamp(&content);
        assert_ne!(
            showing_fallback, showing_nothing,
            "the key now shows the fallback's Picture, so its frame must be drawn again"
        );

        set_picture("stamp-fallback/new", SVG.replace("0000ff", "ff0000"));
        assert_ne!(
            frame_stamp(&content),
            showing_fallback,
            "the key's own Picture replaced the fallback on screen, so its frame must be drawn again"
        );
    }

    /// tauler-core's web renderer cannot depend on tauler, so it spells the
    /// prefix itself; this holds the two together.
    #[test]
    fn the_web_renderer_marks_an_image_named_by_this_prefix_as_a_picture() {
        let root = serde_json::json!({
            "type": "dom",
            "children": [{ "type": "img", "src": format!("{PICTURE_SRC_PREFIX}cpu/1") }]
        });
        let tauler_core::dom::Output::Dom { dom } =
            tauler_core::dom::render_output(&root).expect("render the page");
        assert!(dom.contains(r#"data-tauler-picture="cpu/1""#), "got {dom}");
    }

    /// Whether the raster cache holds any bitmap filed under `key`.
    fn has_raster(key: &str) -> bool {
        rasters()
            .lock()
            .unwrap()
            .keys()
            .any(|(k, _, _, _)| k == key)
    }

    /// The registry and raster cache are process-wide, so this one only
    /// touches keys under `raster-retain/` and asserts per key.
    #[test]
    fn retaining_pictures_drops_the_bitmaps_of_the_pictures_it_drops() {
        crate::render::init_global_ctx(crate::config::FontConfig::default());
        let key = "raster-retain/gone";
        set_picture(key, SVG.to_string());
        let content = serde_json::json!({
            "type": "img",
            "src": format!("{PICTURE_SRC_PREFIX}{key}"),
            "style": { "width": 10, "height": 10 }
        });
        bind(&content, 1.0, &mut HashMap::new());
        assert!(
            has_raster(key),
            "precondition: binding rasterized the Picture"
        );

        retain_pictures(|k| !k.starts_with("raster-retain/"));

        assert!(
            !has_raster(key),
            "a dropped Picture's bitmap must leave the raster cache with it, \
             or every superseded key keeps its pixels forever"
        );
    }
}
