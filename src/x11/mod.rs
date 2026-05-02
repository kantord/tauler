use crate::render::with_global_ctx;
use takumi::resources::image::ImageSource;
use tiny_skia::{IntSize, Pixmap};

pub use crate::layout::PanelAnchor;

const RGBA_BYTES_PER_PIXEL: usize = 4;

/// Convert X11 ZPixmap BGRX bytes (4 bytes per pixel, X padding ignored) to RGBA
/// with alpha=255 (wallpaper is always fully opaque).
pub fn x11_bgrx_to_rgba(bgrx: &[u8]) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(bgrx.len());
    for px in bgrx.chunks_exact(4) {
        rgba.push(px[2]); // R
        rgba.push(px[1]); // G
        rgba.push(px[0]); // B
        rgba.push(0xFF); // A
    }
    rgba
}

/// Store a cropped RGBA wallpaper slice in the image store under `"root-bg"`.
///
/// Layout nodes can reference it via `backgroundImage: "url(root-bg)"`.
/// Because takumi treats this as real pixel content, `backdrop-filter: blur()`
/// will correctly blur the wallpaper — the same effect a compositor would produce.
pub fn inject_root_bg(rgba: Vec<u8>, width: u32, height: u32) {
    if let Some(size) = IntSize::from_wh(width, height) {
        if let Some(pixmap) = Pixmap::from_vec(rgba, size) {
            with_global_ctx(|global| {
                global
                    .persistent_image_store
                    .insert("root-bg".to_string(), ImageSource::from(pixmap.clone()));
            });
        }
    }
}

/// Compute `_NET_WM_STRUT_PARTIAL` values for a panel anchored to a screen edge.
///
/// The 12-element array follows the EWMH spec:
///   [0] left, [1] right, [2] top, [3] bottom,
///   [4] left_start_y,  [5] left_end_y,
///   [6] right_start_y, [7] right_end_y,
///   [8] top_start_x,   [9] top_end_x,
///   [10] bottom_start_x, [11] bottom_end_x
///
/// All values are in physical pixels, absolute from the screen origin.
pub const fn strut_partial_values_for_anchor(
    anchor: PanelAnchor,
    mon_x: i16,
    mon_y: i16,
    _mon_width: u32,
    mon_height: u32,
    phys_panel_width: u32,
    phys_panel_height: u32,
) -> [u32; 12] {
    let mut v = [0u32; 12];
    match anchor {
        PanelAnchor::Left => {
            v[0] = mon_x as u32 + phys_panel_width;
            v[4] = mon_y as u32;
            v[5] = mon_y as u32 + mon_height.saturating_sub(1);
        }
        PanelAnchor::Right => {
            v[1] = phys_panel_width; // measured from right screen edge
            v[6] = mon_y as u32;
            v[7] = mon_y as u32 + mon_height.saturating_sub(1);
        }
        PanelAnchor::Top => {
            v[2] = mon_y as u32 + phys_panel_height;
            v[8] = mon_x as u32;
            v[9] = mon_x as u32 + _mon_width.saturating_sub(1);
        }
        PanelAnchor::Bottom => {
            v[3] = phys_panel_height; // measured from bottom screen edge
            v[10] = mon_x as u32;
            v[11] = mon_x as u32 + _mon_width.saturating_sub(1);
        }
    }
    v
}

/// Convert an X11 TrueColor pixel (`0x00RRGGBB`) to a solid-color RGBA buffer.
///
/// Fills `width × height` pixels with the same colour.
/// Used as a fallback when no wallpaper pixmap is set (e.g. i3 solid background).
pub fn solid_color_rgba(pixel: u32, width: u32, height: u32) -> Vec<u8> {
    let r = ((pixel >> 16) & 0xFF) as u8;
    let g = ((pixel >> 8) & 0xFF) as u8;
    let b = (pixel & 0xFF) as u8;
    let count = (width * height) as usize;
    let mut rgba = Vec::with_capacity(count * RGBA_BYTES_PER_PIXEL);
    for _ in 0..count {
        rgba.extend_from_slice(&[r, g, b, 0xFF]);
    }
    rgba
}

pub mod click;
pub mod outputs;
pub mod panel;
