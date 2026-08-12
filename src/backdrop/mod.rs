//! The desktop pixels behind a panel, so `backdrop-filter` has something to filter.
//!
//! Each panel is rasterized into its own isolated buffer, so by default there is
//! literally nothing behind it — `backdrop-blur` would blur transparent black.
//! This module closes that gap: when a `<wallpaper>` is rendered, its pixels are
//! published here; when a panel on the same output is rendered, the slice of
//! wallpaper it covers is handed to that render as the `root-bg` image, which the
//! layout references as `<image src="root-bg">`.
//!
//! The crop is *returned*, not installed into a global: `root-bg` is a single
//! key, so a module that installed it would make every render depend on which
//! surface rendered last. [`crop_for`] hands the caller a value and
//! [`crate::render`] binds it for exactly one rasterization.
//!
//! The pixels come from tauler's own wallpaper render rather than from a
//! `get_image` on the root pixmap. Reading X11 would race the presenter thread,
//! which paints the wallpaper asynchronously — a panel could sample the previous
//! frame and blur a stale image. Taking it in-process is exact and synchronous.
//! The cost is that a wallpaper set by another program (feh, xwallpaper) is not
//! visible here; only tauler's own `<wallpaper>` nodes are.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, RwLock};

use takumi::prelude::ImageSource;
use takumi_core::resources::image_buffer::ImageBuffer;

use crate::layout::{surface_origin, Rect, SurfaceKind, SurfaceSpec};

/// The image key a layout uses to reach its backdrop: `<image src="root-bg">`.
pub const ROOT_BG_KEY: &str = "root-bg";

const RGBA_BYTES: usize = 4;

/// The slice of wallpaper behind one surface, ready to render against.
#[derive(Clone)]
pub struct Backdrop {
    /// Which wallpaper frame this came from. Bumped on every wallpaper render,
    /// so it distinguishes "same panel, wallpaper moved underneath".
    pub generation: u64,
    /// Where the crop was taken from. Two panels over one wallpaper differ only
    /// here, so it must be part of any cache key that covers the backdrop.
    pub rect: Rect,
    pub image: ImageSource,
}

/// A rendered wallpaper, kept so panels over it can sample what they cover.
struct Wallpaper {
    rect: Rect,
    bgrx: Arc<Vec<u8>>,
    generation: u64,
}

fn registry() -> &'static RwLock<HashMap<String, Wallpaper>> {
    static REGISTRY: OnceLock<RwLock<HashMap<String, Wallpaper>>> = OnceLock::new();
    REGISTRY.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Monotonic counter. Panels fold it into their render cache key, so a wallpaper
/// change re-renders them even though their own content JSON is unchanged.
fn next_generation() -> u64 {
    static GENERATION: AtomicU64 = AtomicU64::new(1);
    GENERATION.fetch_add(1, Ordering::Relaxed)
}

/// Copy the `dst_w × dst_h` window at `(off_x, off_y)` out of a BGRX image,
/// converting to opaque RGBA as it goes.
///
/// Cropping and converting in one pass is what lets the registry hold the
/// wallpaper's own `Arc<Vec<u8>>` frame rather than a second, RGBA copy of it —
/// worth 32 MB at 4K.
///
/// Areas outside the source read as transparent, so a panel hanging off the edge
/// of its wallpaper degrades to "no backdrop there" instead of panicking.
pub fn crop_bgrx_to_rgba(
    src: &[u8],
    src_w: u32,
    src_h: u32,
    off_x: i32,
    off_y: i32,
    dst_w: u32,
    dst_h: u32,
) -> Vec<u8> {
    let mut dst = vec![0u8; (dst_w * dst_h) as usize * RGBA_BYTES];
    for row in 0..dst_h {
        let sy = off_y + row as i32;
        if sy < 0 || sy >= src_h as i32 {
            continue;
        }
        for col in 0..dst_w {
            let sx = off_x + col as i32;
            if sx < 0 || sx >= src_w as i32 {
                continue;
            }
            let s = ((sy as u32 * src_w + sx as u32) as usize) * RGBA_BYTES;
            let d = ((row * dst_w + col) as usize) * RGBA_BYTES;
            dst[d..d + RGBA_BYTES].copy_from_slice(&[src[s + 2], src[s + 1], src[s], 0xFF]);
        }
    }
    dst
}

/// Record a freshly rendered wallpaper so panels on its output can sample it.
///
/// Takes the frame's `Arc` rather than a slice: the buffer is shared with the
/// `SurfaceFrame` on its way to the display, so publishing costs a refcount
/// bump and no pixels.
pub fn publish_wallpaper(spec: &SurfaceSpec, bgrx: Arc<Vec<u8>>, width: u32, height: u32) {
    let Some(output) = spec.output.clone() else {
        return;
    };
    registry().write().unwrap().insert(
        output,
        Wallpaper {
            rect: Rect {
                x: spec.x as i16,
                y: spec.y as i16,
                width,
                height,
            },
            bgrx,
            generation: next_generation(),
        },
    );
}

/// The generation of the wallpaper currently behind `spec`, or 0 for none.
///
/// A registry lookup and nothing else — no cropping, no decode. The reconciler
/// calls this on every panel every tick to answer "did the wallpaper move under
/// a panel whose own content is unchanged?", which the spec diff cannot see.
pub fn generation_for(spec: &SurfaceSpec) -> u64 {
    if spec.kind == SurfaceKind::Wallpaper {
        return 0;
    }
    let Some(output) = spec.output.as_deref() else {
        return 0;
    };
    registry()
        .read()
        .unwrap()
        .get(output)
        .map(|w| w.generation)
        .unwrap_or(0)
}

/// The slice of wallpaper that `spec` covers.
///
/// `None` means there is nothing behind this surface — a wallpaper (which
/// samples nothing), a surface with no resolved output, or an output with no
/// `<wallpaper>` on it. Callers must treat `None` as "no `root-bg` at all"
/// rather than "leave whatever was there", or a panel on a bare output shows
/// the previous panel's pixels.
pub fn crop_for(spec: &SurfaceSpec, phys: (u32, u32)) -> Option<Backdrop> {
    if spec.kind == SurfaceKind::Wallpaper {
        return None;
    }
    let output = spec.output.as_deref()?;
    let guard = registry().read().unwrap();
    let wallpaper = guard.get(output)?;
    // The wallpaper spans its whole output, so its rect *is* the output rect —
    // enough to place an anchored panel without consulting the output map.
    let wall = wallpaper.rect;
    let (px, py) = surface_origin(spec, phys, wall);
    let generation = wallpaper.generation;
    let rect = Rect {
        x: px,
        y: py,
        width: phys.0,
        height: phys.1,
    };

    // Cropping a full-height panel out of a 4K wallpaper costs ~0.5ms, which
    // would otherwise be paid on every render — including the cache hits that
    // are supposed to cost nothing. The crop only changes when the wallpaper or
    // the panel's rect does, so keep the decoded image and hand it back again.
    // `ImageSource` is Arc-backed, so a cached one costs a refcount bump.
    if let Some(image) = cached_crop(&spec.id, generation, rect) {
        return Some(Backdrop {
            generation,
            rect,
            image,
        });
    }

    let slice = crop_bgrx_to_rgba(
        &wallpaper.bgrx,
        wall.width,
        wall.height,
        (px - wall.x) as i32,
        (py - wall.y) as i32,
        phys.0,
        phys.1,
    );
    drop(guard);

    let image = decode_rgba(slice, phys.0, phys.1)?;
    remember_crop(&spec.id, generation, rect, image.clone());
    Some(Backdrop {
        generation,
        rect,
        image,
    })
}

/// Drop everything held for a surface that has gone away.
///
/// Both maps are keyed by ids that live as long as the layout names them, so
/// without this a session that cycles through panels grows a crop per panel and
/// never gives one back.
pub fn forget(spec: &SurfaceSpec) {
    crops().write().unwrap().remove(&spec.id);
    if spec.kind == SurfaceKind::Wallpaper {
        if let Some(output) = spec.output.as_deref() {
            registry().write().unwrap().remove(output);
        }
    }
}

type CropKey = (u64, Rect);

fn crops() -> &'static RwLock<HashMap<String, (CropKey, ImageSource)>> {
    static CROPS: OnceLock<RwLock<HashMap<String, (CropKey, ImageSource)>>> = OnceLock::new();
    CROPS.get_or_init(|| RwLock::new(HashMap::new()))
}

fn cached_crop(id: &str, generation: u64, rect: Rect) -> Option<ImageSource> {
    let guard = crops().read().unwrap();
    let (key, image) = guard.get(id)?;
    (*key == (generation, rect)).then(|| image.clone())
}

fn remember_crop(id: &str, generation: u64, rect: Rect, image: ImageSource) {
    crops()
        .write()
        .unwrap()
        .insert(id.to_string(), ((generation, rect), image));
}

/// Decode an RGBA buffer into a takumi image source.
fn decode_rgba(rgba: Vec<u8>, width: u32, height: u32) -> Option<ImageSource> {
    ImageBuffer::from_rgba_bytes(rgba, width, height).map(ImageSource::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 3x2 BGRX image where each pixel's *red* channel encodes its index.
    /// Red is byte 2 in BGRX, so a crop that forgets to swizzle reads zeros.
    fn src() -> Vec<u8> {
        (0u8..6).flat_map(|i| [0, 0, i, 0]).collect()
    }

    #[test]
    fn crop_takes_the_window_at_the_given_offset() {
        // Bottom-right 2x1 of a 3x2 image is indices 4,5.
        let out = crop_bgrx_to_rgba(&src(), 3, 2, 1, 1, 2, 1);
        assert_eq!(out[0], 4, "first cropped pixel");
        assert_eq!(out[4], 5, "second cropped pixel");
    }

    #[test]
    fn crop_at_the_origin_covers_the_whole_source_and_swizzles_to_rgba() {
        let out = crop_bgrx_to_rgba(&src(), 3, 2, 0, 0, 3, 2);
        let expected: Vec<u8> = (0u8..6).flat_map(|i| [i, 0, 0, 0xFF]).collect();
        assert_eq!(out, expected);
    }

    #[test]
    fn crop_past_the_edge_reads_as_transparent_rather_than_panicking() {
        // Entirely off the right edge.
        let out = crop_bgrx_to_rgba(&src(), 3, 2, 5, 0, 2, 1);
        assert!(
            out.iter().all(|b| *b == 0),
            "out-of-bounds area must be transparent, got {out:?}"
        );
    }

    #[test]
    fn crop_partially_past_the_edge_keeps_the_overlapping_pixels() {
        // Starts at the last column, so one real pixel then one off-edge.
        let out = crop_bgrx_to_rgba(&src(), 3, 2, 2, 0, 2, 1);
        assert_eq!(out[0], 2, "the overlapping pixel is kept");
        assert_eq!(&out[4..8], &[0, 0, 0, 0], "the rest is transparent");
    }

    #[test]
    fn crop_swaps_channels_and_forces_opacity() {
        // One BGRX pixel: B=1, G=2, R=3 -> RGBA 3,2,1,255.
        assert_eq!(
            crop_bgrx_to_rgba(&[1, 2, 3, 0], 1, 1, 0, 0, 1, 1),
            vec![3, 2, 1, 0xFF]
        );
    }
}
