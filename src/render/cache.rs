//! The frames already drawn, kept so they need not be drawn again.
//!
//! Owned by the render worker rather than reachable from anywhere: it is the only
//! thing that draws, so a cache it owns needs no lock, and the entries cannot be
//! evicted by a caller that has nothing to do with drawing.
//!
//! Keyed by what the pixels are, never by which surface asked for them (ADR
//! 0011). Two structurally identical trees at the same size are the same picture,
//! whoever wanted it — which is what lets every tick re-render everything (ADR
//! 0007) at no cost when nothing changed.
//!
//! The backdrop widens the key to `(generation, rect)`. The generation stops a
//! stale hit once the wallpaper moves underneath, and the rect keeps two
//! same-size, same-content panels from colliding on one entry and being served
//! each other's slice of wallpaper — see [`crate::backdrop`].

use std::sync::Arc;

use cached::{Cached, LruCache};

use super::worker::RenderRequest;
use crate::layout::Rect;

/// How many frames to keep. A bar has a handful of panels and each entry is a
/// whole framebuffer, so this trades memory for the repaints that repeat.
const CAPACITY: usize = 6;

/// What makes two renders the same render.
#[derive(Clone, PartialEq, Eq, Hash)]
struct FrameKey {
    content: String,
    width: u32,
    height: u32,
    /// `f32` is not `Hash`; its bits are.
    dpr_bits: u32,
    backdrop: Option<(u64, Rect)>,
}

impl FrameKey {
    fn of(request: &RenderRequest) -> Self {
        FrameKey {
            content: json_canon::to_string(&request.content).unwrap_or_default(),
            width: request.width,
            height: request.height,
            dpr_bits: request.dpr.to_bits(),
            backdrop: request.backdrop.as_ref().map(|b| (b.generation, b.rect)),
        }
    }
}

pub struct FrameCache {
    store: LruCache<FrameKey, Arc<Vec<u8>>>,
}

impl Default for FrameCache {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameCache {
    pub fn new() -> Self {
        FrameCache {
            store: LruCache::builder()
                .max_size(CAPACITY)
                .build()
                .expect("capacity is set"),
        }
    }

    /// The frame for `request`, drawing it only if it is not already here.
    pub fn frame(&mut self, request: &RenderRequest) -> Arc<Vec<u8>> {
        let key = FrameKey::of(request);
        if let Some(hit) = self.store.cache_get(&key) {
            return Arc::clone(hit);
        }
        let pixels = super::render_frame_keyed(
            &request.content,
            request.width,
            request.height,
            request.dpr,
            request.backdrop.as_ref(),
        );
        self.store.cache_set(key, Arc::clone(&pixels));
        pixels
    }

    /// Drop everything. The fonts changed, so every frame here was drawn with
    /// the wrong ones.
    pub fn clear(&mut self) {
        self.store.cache_clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backdrop::Backdrop;
    use crate::config::FontConfig;
    use takumi::prelude::ImageSource;
    use takumi_core::resources::image_buffer::ImageBuffer;

    fn request(width: u32, height: u32) -> RenderRequest {
        RenderRequest {
            id: "p1".into(),
            content: serde_json::json!({}),
            width,
            height,
            dpr: 1.0,
            backdrop: None,
        }
    }

    fn cache() -> FrameCache {
        crate::render::init_global_ctx(FontConfig::default());
        FrameCache::new()
    }

    /// A hit hands back the very same buffer — the point of the whole module.
    #[test]
    fn the_same_request_twice_is_drawn_once() {
        let mut cache = cache();
        let first = cache.frame(&request(10, 10));
        let second = cache.frame(&request(10, 10));
        assert!(
            Arc::ptr_eq(&first, &second),
            "an identical request must be served from the cache, not drawn again"
        );
    }

    #[test]
    fn a_different_size_is_a_different_frame() {
        let mut cache = cache();
        let small = cache.frame(&request(10, 10));
        let large = cache.frame(&request(20, 20));
        assert!(
            !Arc::ptr_eq(&small, &large),
            "size is part of what makes a frame; these must not share an entry"
        );
    }

    /// Two panels can be the same size and hold the same tree while sitting over
    /// different parts of the wallpaper. Without the rect in the key they collide
    /// on one entry, and the second panel is served the first one's slice.
    #[test]
    fn same_content_over_a_different_slice_of_wallpaper_is_a_different_frame() {
        let mut cache = cache();
        let pixels = vec![0u8; 10 * 10 * 4];
        let image: ImageSource = ImageBuffer::from_rgba_bytes(pixels, 10, 10)
            .map(ImageSource::from)
            .expect("test image");
        let at = |x: i16| RenderRequest {
            backdrop: Some(Backdrop {
                generation: 1,
                rect: crate::layout::Rect {
                    x,
                    y: 0,
                    width: 10,
                    height: 10,
                },
                image: image.clone(),
            }),
            ..request(10, 10)
        };
        let left = cache.frame(&at(0));
        let right = cache.frame(&at(50));
        assert!(
            !Arc::ptr_eq(&left, &right),
            "two panels over different slices of wallpaper must not share an entry"
        );
    }

    /// A newer wallpaper under an otherwise-identical panel is a different
    /// picture, and the generation is the only thing that says so.
    #[test]
    fn a_newer_wallpaper_generation_is_a_different_frame() {
        let mut cache = cache();
        let pixels = vec![0u8; 10 * 10 * 4];
        let image: ImageSource = ImageBuffer::from_rgba_bytes(pixels, 10, 10)
            .map(ImageSource::from)
            .expect("test image");
        let rect = crate::layout::Rect {
            x: 0,
            y: 0,
            width: 10,
            height: 10,
        };
        let at = |generation: u64| RenderRequest {
            backdrop: Some(Backdrop {
                generation,
                rect,
                image: image.clone(),
            }),
            ..request(10, 10)
        };
        let before = cache.frame(&at(1));
        let after = cache.frame(&at(2));
        assert!(
            !Arc::ptr_eq(&before, &after),
            "a moved wallpaper must not be served the frame drawn over the old one"
        );
    }

    #[test]
    fn clearing_forgets_everything() {
        let mut cache = cache();
        let first = cache.frame(&request(10, 10));
        cache.clear();
        let second = cache.frame(&request(10, 10));
        assert!(
            !Arc::ptr_eq(&first, &second),
            "a cleared cache must draw again; the fonts it drew with are gone"
        );
    }
}
