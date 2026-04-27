use std::collections::HashMap;
use std::sync::Arc;

use x11rb::{
    connection::{Connection, RequestConnection},
    protocol::xproto::*,
    rust_connection::RustConnection,
    wrapper::ConnectionExt as _,
};

use crate::layout::{PanelSpecData, PanelAnchor, OutputInfo};
use crate::display_manager::DisplayManager;
use crate::presentation::PanelFrame;

const XRESOURCES_PROP_MAX_LEN: u32 = 65536;
const MM_PER_INCH: f32 = 25.4;
const FALLBACK_DPI: f32 = 96.0;
const PUT_IMAGE_HEADER_BYTES: usize = 28; // 24-byte standard header + 4-byte BigRequests field
use crate::render::init_global_ctx;
use crate::x11::strut_partial_values_for_anchor;

/// Send a BGRX pixel buffer via one or more PutImage requests, each within
/// the X server's maximum request size. Large panels (e.g. 4K) exceed the
/// ~16 MB default limit and must be sent as horizontal strips.
pub fn put_image_chunked(
    conn: &RustConnection,
    drawable: u32,
    gc: u32,
    width: u32,
    depth: u8,
    bgrx: &[u8],
) -> anyhow::Result<()> {
    let stride = width as usize * 4;
    if stride == 0 || bgrx.is_empty() {
        return Ok(());
    }
    let available = conn.maximum_request_bytes().saturating_sub(PUT_IMAGE_HEADER_BYTES);
    let rows_per_chunk = (available / stride).max(1);
    for (i, chunk) in bgrx.chunks(rows_per_chunk * stride).enumerate() {
        let chunk_rows = (chunk.len() / stride) as u16;
        conn.put_image(
            ImageFormat::Z_PIXMAP, drawable, gc,
            width as u16, chunk_rows,
            0, (i * rows_per_chunk) as i16,
            0, depth, chunk,
        ).map_err(|e| anyhow::anyhow!(e))?;
    }
    Ok(())
}

/// A live X11 panel window, created from a `PanelSpec` at runtime.
pub struct Panel {
    pub id: String,
    pub win_id: u32,
    pub gc: u32,
    pub win_x: i16,
    pub win_y: i16,
    pub phys_width: u32,
    pub phys_height: u32,
    pub bgrx: Arc<Vec<u8>>,
}


pub fn i3_dpi(conn: &RustConnection, root: Window, screen: &Screen) -> f32 {
    let from_xresources = (|| -> Option<f32> {
        let atom = conn.intern_atom(false, b"RESOURCE_MANAGER").ok()?.reply().ok()?.atom;
        let prop = conn
            .get_property(false, root, atom, AtomEnum::ANY, 0, XRESOURCES_PROP_MAX_LEN)
            .ok()?
            .reply()
            .ok()?;
        let data = String::from_utf8_lossy(&prop.value).into_owned();
        for line in data.lines() {
            if let Some(val) = line.strip_prefix("Xft.dpi:") {
                return val.trim().parse::<f32>().ok();
            }
        }
        None
    })();
    if let Some(dpi) = from_xresources {
        tracing::info!(dpi, "DPI detected (from Xft.dpi)");
        return dpi;
    }
    if screen.height_in_millimeters > 0 {
        let dpi = screen.height_in_pixels as f32 * MM_PER_INCH / screen.height_in_millimeters as f32;
        tracing::info!(dpi, "DPI detected (from screen physical dimensions)");
        return dpi;
    }
    tracing::warn!("DPI fallback to {FALLBACK_DPI}");
    FALLBACK_DPI
}

fn create_panel(
    spec: &PanelSpecData,
    frame: &PanelFrame,
    ctx: &PanelContext,
) -> anyhow::Result<Panel> {
    let phys_width = (spec.width as f32 * spec.dpr).round() as u32;
    let phys_height = (spec.height as f32 * spec.dpr).round() as u32;

    let (mon_x, mon_y, mon_width, mon_height) = spec.output.as_ref()
        .and_then(|name| ctx.output_map.get(name))
        .map(|o| (o.x, o.y, o.width, o.height))
        .unwrap_or((ctx.mon_x, ctx.mon_y, ctx.mon_width, ctx.mon_height));

    let (win_x, win_y) = match &spec.anchor {
        Some(PanelAnchor::Left) | Some(PanelAnchor::Top) => (mon_x, mon_y),
        Some(PanelAnchor::Right) => (mon_x + mon_width as i16 - phys_width as i16, mon_y),
        Some(PanelAnchor::Bottom) => (mon_x, mon_y + mon_height as i16 - phys_height as i16),
        None => (
            mon_x + (spec.x as f32 * spec.dpr).round() as i16,
            mon_y + (spec.y as f32 * spec.dpr).round() as i16,
        ),
    };

    let win_id = ctx.conn.generate_id()?;
    ctx.conn.create_window(
        x11rb::COPY_DEPTH_FROM_PARENT,
        win_id,
        ctx.root,
        win_x,
        win_y,
        phys_width as u16,
        phys_height as u16,
        0,
        WindowClass::INPUT_OUTPUT,
        ctx.root_visual,
        &CreateWindowAux::new()
            .background_pixel(ctx.black_pixel)
            .override_redirect(1)
            .event_mask(EventMask::EXPOSURE | EventMask::BUTTON_PRESS),
    )?;

    let stack_mode = if spec.above { StackMode::ABOVE } else { StackMode::BELOW };
    ctx.conn.configure_window(win_id, &ConfigureWindowAux::new().stack_mode(stack_mode))?;

    if let Some(anchor) = spec.anchor.clone() {
        let strut_vals = strut_partial_values_for_anchor(
            anchor, mon_x, mon_y, mon_width, mon_height, phys_width, phys_height,
        );
        ctx.conn.change_property32(PropMode::REPLACE, win_id, ctx.strut_atom, AtomEnum::CARDINAL, &strut_vals)?;
        ctx.conn.change_property32(PropMode::REPLACE, win_id, ctx.strut_legacy_atom, AtomEnum::CARDINAL, &strut_vals[..4])?;
    }

    let gc = ctx.conn.generate_id()?;
    ctx.conn.create_gc(gc, win_id, &CreateGCAux::new())?;

    ctx.conn.flush()?;

    let bgrx = frame.pixels.clone();
    put_image_chunked(&ctx.conn, win_id, gc, phys_width, ctx.depth, &bgrx[..])?;
    ctx.conn.map_window(win_id)?;
    ctx.conn.flush()?;

    Ok(Panel {
        id: spec.id.clone(),
        win_id,
        gc,
        win_x,
        win_y,
        phys_width,
        phys_height,
        bgrx,
    })
}

pub struct X11PanelContext {
    pub conn: Arc<RustConnection>,
    pub root: u32,
    pub depth: u8,
    pub root_visual: u32,
    pub black_pixel: u32,
    pub dpr: f32,
    pub mon_x: i16,
    pub mon_y: i16,
    pub mon_width: u32,
    pub mon_height: u32,
    pub xrootpmap_atom: Option<u32>,
    pub strut_atom: u32,
    pub strut_legacy_atom: u32,
    pub output_map: Arc<HashMap<String, OutputInfo>>,
    pub dpi: f32,
    pub output_name: String,
    pub screen_width_logical: u32,
    pub screen_height_logical: u32,
}

/// Backward-compatible alias so callers that import `x11::panel::PanelContext` still compile.
pub type PanelContext = X11PanelContext;

impl DisplayManager for X11PanelContext {
    type Panel = Panel;

    fn create_window(&mut self, spec: &PanelSpecData, frame: &PanelFrame) -> Result<Panel, anyhow::Error> {
        init_global_ctx();
        let panel = create_panel(spec, frame, self)?;
        Ok(panel)
    }

    fn delete_window(&mut self, panel: Panel) -> Result<(), anyhow::Error> {
        let _ = self.conn.free_gc(panel.gc);
        let _ = self.conn.destroy_window(panel.win_id);
        Ok(())
    }

    fn update_image(&mut self, panel: &mut Panel, bgrx: &[u8]) -> Result<(), anyhow::Error> {
        panel.bgrx = Arc::new(bgrx.to_vec());
        put_image_chunked(&self.conn, panel.win_id, panel.gc, panel.phys_width, self.depth, bgrx)?;
        self.conn.flush().map_err(|e| anyhow::anyhow!(e))?;
        Ok(())
    }

    fn update_position(&mut self, panel: &mut Panel, spec: &PanelSpecData) -> Result<(), anyhow::Error> {
        let (mon_x, mon_y, mon_width, mon_height) = spec.output.as_ref()
            .and_then(|name| self.output_map.get(name))
            .map(|o| (o.x, o.y, o.width, o.height))
            .unwrap_or((self.mon_x, self.mon_y, self.mon_width, self.mon_height));

        let (win_x, win_y) = match &spec.anchor {
            Some(PanelAnchor::Left) | Some(PanelAnchor::Top) => (mon_x, mon_y),
            Some(PanelAnchor::Right) => (mon_x + mon_width as i16 - panel.phys_width as i16, mon_y),
            Some(PanelAnchor::Bottom) => (mon_x, mon_y + mon_height as i16 - panel.phys_height as i16),
            None => (
                mon_x + (spec.x as f32 * spec.dpr).round() as i16,
                mon_y + (spec.y as f32 * spec.dpr).round() as i16,
            ),
        };

        if win_x != panel.win_x || win_y != panel.win_y {
            self.conn.configure_window(
                panel.win_id,
                &ConfigureWindowAux::new().x(win_x as i32).y(win_y as i32),
            ).map_err(|e| anyhow::anyhow!(e))?;
            panel.win_x = win_x;
            panel.win_y = win_y;
        }

        Ok(())
    }

    fn update_dimensions(&mut self, panel: &mut Panel, spec: &PanelSpecData) -> Result<(), anyhow::Error> {
        let new_phys_width = (spec.width as f32 * spec.dpr).round() as u32;
        let new_phys_height = (spec.height as f32 * spec.dpr).round() as u32;

        if new_phys_width != panel.phys_width || new_phys_height != panel.phys_height {
            self.conn.configure_window(
                panel.win_id,
                &ConfigureWindowAux::new()
                    .width(new_phys_width)
                    .height(new_phys_height),
            ).map_err(|e| anyhow::anyhow!(e))?;
            panel.phys_width = new_phys_width;
            panel.phys_height = new_phys_height;

            if let Some(anchor) = spec.anchor.clone() {
                let (mon_x, mon_y, mon_width, mon_height) = spec.output.as_ref()
                    .and_then(|name| self.output_map.get(name))
                    .map(|o| (o.x, o.y, o.width, o.height))
                    .unwrap_or((self.mon_x, self.mon_y, self.mon_width, self.mon_height));

                let strut_vals = strut_partial_values_for_anchor(
                    anchor, mon_x, mon_y, mon_width, mon_height, new_phys_width, new_phys_height,
                );
                self.conn.change_property32(PropMode::REPLACE, panel.win_id, self.strut_atom, AtomEnum::CARDINAL, &strut_vals)
                    .map_err(|e| anyhow::anyhow!(e))?;
                self.conn.change_property32(PropMode::REPLACE, panel.win_id, self.strut_legacy_atom, AtomEnum::CARDINAL, &strut_vals[..4])
                    .map_err(|e| anyhow::anyhow!(e))?;
            }

            self.conn.flush().map_err(|e| anyhow::anyhow!(e))?;
        }

        Ok(())
    }

    fn flush(&mut self) {
        let _ = self.conn.flush();
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    fn make_panel_ctx() -> Option<super::PanelContext> {
        use x11rb::rust_connection::RustConnection;
        use x11rb::connection::Connection as _;
        use x11rb::protocol::xproto::ConnectionExt as XprotoConnExt;

        let (conn, screen_num) = RustConnection::connect(None).ok()?;
        let screen = conn.setup().roots[screen_num].clone();
        let depth = screen.root_depth;
        let root_visual = screen.root_visual;
        let black_pixel = screen.black_pixel;
        let root = screen.root;

        let strut_atom = XprotoConnExt::intern_atom(&conn, false, b"_NET_WM_STRUT_PARTIAL")
            .ok()?.reply().ok()?.atom;
        let strut_legacy_atom = XprotoConnExt::intern_atom(&conn, false, b"_NET_WM_STRUT")
            .ok()?.reply().ok()?.atom;
        let xrootpmap_atom = XprotoConnExt::intern_atom(&conn, false, b"_XROOTPMAP_ID").ok()
            .and_then(|c: x11rb::cookie::Cookie<'_, _, x11rb::protocol::xproto::InternAtomReply>| c.reply().ok())
            .map(|r| r.atom);

        // Use screen pixel dimensions as monitor size.
        let mon_width = screen.width_in_pixels as u32;
        let mon_height = screen.height_in_pixels as u32;

        Some(super::PanelContext {
            conn: Arc::new(conn),
            root,
            depth,
            root_visual,
            black_pixel,
            dpr: 1.0,
            mon_x: 0,
            mon_y: 0,
            mon_width,
            mon_height,
            xrootpmap_atom,
            strut_atom,
            strut_legacy_atom,
            output_map: Arc::new(HashMap::new()),
            dpi: 96.0,
            output_name: String::new(),
            screen_width_logical: mon_width,
            screen_height_logical: mon_height,
        })
    }

    /// Build a minimal PanelSpecData with the given id/dimensions.
    fn make_spec(id: &str, width: u32, height: u32) -> crate::layout::PanelSpecData {
        crate::layout::PanelSpecData {
            id: id.to_string(),
            anchor: None,
            width,
            height,
            x: 0,
            y: 0,
            outer_gap: 0,
            output: None,
            above: false,
            content: serde_json::Value::Null,
            dpr: 1.0,
        }
    }

    fn blank_frame(w: u32, h: u32) -> crate::presentation::PanelFrame {
        use std::sync::Arc;
        crate::presentation::PanelFrame {
            pixels: Arc::new(vec![0u8; (w * h * 4) as usize]),
            width: w,
            height: h,
        }
    }

    // ---------------------------------------------------------------------------
    // Claim F: X11PanelContext implements DisplayManager — create_window returns
    // a Panel with phys_width > 0 and phys_height > 0.
    // ---------------------------------------------------------------------------
    #[test]
    fn display_manager_create_window_returns_panel_with_positive_dimensions() {
        use crate::display_manager::DisplayManager;

        let mut ctx = match make_panel_ctx() {
            Some(c) => c,
            None => {
                println!("SKIP: no X11 display available");
                return;
            }
        };

        let spec = make_spec("dm-create", 200, 30);
        let panel = <super::X11PanelContext as DisplayManager>::create_window(&mut ctx, &spec, &blank_frame(200, 30))
            .expect("create_window should succeed when X11 is available");

        assert!(panel.phys_width > 0, "phys_width must be > 0");
        assert!(panel.phys_height > 0, "phys_height must be > 0");

        // Cleanup
        let _ = <super::X11PanelContext as DisplayManager>::delete_window(&mut ctx, panel);
    }

    // ---------------------------------------------------------------------------
    // Claim G: X11PanelContext implements DisplayManager — delete_window destroys
    // the X11 window (get_geometry returns an error afterwards).
    // ---------------------------------------------------------------------------
    #[test]
    fn display_manager_delete_window_destroys_x11_window() {
        use crate::display_manager::DisplayManager;
        use x11rb::connection::Connection as _;
        use x11rb::protocol::xproto::ConnectionExt as XprotoExt;

        let mut ctx = match make_panel_ctx() {
            Some(c) => c,
            None => {
                println!("SKIP: no X11 display available");
                return;
            }
        };

        let spec = make_spec("dm-delete", 200, 30);
        let panel = <super::X11PanelContext as DisplayManager>::create_window(&mut ctx, &spec, &blank_frame(200, 30))
            .expect("create_window must succeed for delete_window test");

        let win_id = panel.win_id;

        // Sanity: window should exist before delete_window.
        ctx.conn.flush().ok();
        let before = XprotoExt::get_geometry(&*ctx.conn, win_id)
            .ok()
            .and_then(|c| c.reply().ok());
        assert!(before.is_some(), "window should exist before delete_window");

        <super::X11PanelContext as DisplayManager>::delete_window(&mut ctx, panel)
            .expect("delete_window should succeed");
        ctx.conn.flush().ok();

        // After delete_window the window must no longer exist.
        let after = XprotoExt::get_geometry(&*ctx.conn, win_id)
            .ok()
            .and_then(|c| c.reply().ok());
        assert!(after.is_none(), "get_geometry should fail after delete_window (window destroyed)");
    }

    // ---------------------------------------------------------------------------
    // Claim H: X11PanelContext implements DisplayManager — update_image does not
    // panic and flushes to the connection without error.
    // ---------------------------------------------------------------------------
    #[test]
    fn display_manager_update_image_does_not_panic_and_flushes() {
        use crate::display_manager::DisplayManager;

        let mut ctx = match make_panel_ctx() {
            Some(c) => c,
            None => {
                println!("SKIP: no X11 display available");
                return;
            }
        };

        let spec = make_spec("dm-update-image", 10, 10);
        let mut panel = <super::X11PanelContext as DisplayManager>::create_window(&mut ctx, &spec, &blank_frame(10, 10))
            .expect("create_window must succeed for update_image test");

        // Build a minimal BGRX buffer: 10 * 10 * 4 bytes, all zeros.
        let bgrx = vec![0u8; 10 * 10 * 4];
        <super::X11PanelContext as DisplayManager>::update_image(&mut ctx, &mut panel, &bgrx)
            .expect("update_image should not return an error");

        // Cleanup
        let _ = <super::X11PanelContext as DisplayManager>::delete_window(&mut ctx, panel);
    }

    // ---------------------------------------------------------------------------
    // R1: DisplayManager::update_dimensions resizes the window and updates state.
    // ---------------------------------------------------------------------------
    #[test]
    fn display_manager_update_dimensions_resizes_window_and_updates_state() {
        use crate::display_manager::DisplayManager;
        use x11rb::connection::Connection as _;
        use x11rb::protocol::xproto::ConnectionExt as XprotoExt;

        let mut ctx = match make_panel_ctx() {
            Some(c) => c,
            None => {
                println!("SKIP: no X11 display available");
                return;
            }
        };

        let mut panel = <super::X11PanelContext as DisplayManager>::create_window(
            &mut ctx,
            &make_spec("test-dm-dims", 200, 30),
            &blank_frame(200, 30),
        )
        .expect("create_window should succeed when X11 is available");

        <super::X11PanelContext as DisplayManager>::update_dimensions(
            &mut ctx,
            &mut panel,
            &make_spec("test-dm-dims", 300, 30),
        )
        .expect("update_dimensions should succeed");

        assert_eq!(panel.phys_width, 300, "phys_width in state should be updated to 300");

        ctx.conn.flush().ok();
        let geom = XprotoExt::get_geometry(&*ctx.conn, panel.win_id)
            .ok()
            .and_then(|c| c.reply().ok())
            .expect("get_geometry should succeed after update_dimensions");
        assert_eq!(geom.width, 300u16, "X11 window width should be 300 after update_dimensions");

        // Cleanup
        let _ = <super::X11PanelContext as DisplayManager>::delete_window(&mut ctx, panel);
    }

    // ---------------------------------------------------------------------------
    // Claim I: create_window stores the pre-rendered frame pixels in panel.bgrx.
    // The frame is now passed in by the reconciler layer (not computed inside DM).
    // ---------------------------------------------------------------------------
    #[test]
    fn create_window_with_non_null_content_renders_spec_content_not_null() {
        use crate::display_manager::DisplayManager;
        use crate::presentation::PanelFrame;
        use crate::render::render_frame;

        let mut ctx = match make_panel_ctx() {
            Some(c) => c,
            None => {
                println!("SKIP: no X11 display available");
                return;
            }
        };

        // This content renders a solid red background — visually distinct from the
        // null/empty-container fallback that the old create_panel used.
        let content = serde_json::json!({
            "type": "container",
            "tw": "w-full h-full",
            "style": {"backgroundColor": "red"},
            "children": []
        });
        let spec = crate::layout::PanelSpecData {
            id: "test-content-render".to_string(),
            anchor: None,
            width: 100,
            height: 30,
            x: 0,
            y: 0,
            outer_gap: 0,
            output: None,
            above: false,
            content: content.clone(),
            dpr: 1.0,
        };

        let phys_width = (spec.width as f32 * spec.dpr).round() as u32;
        let phys_height = (spec.height as f32 * spec.dpr).round() as u32;
        let pixels = render_frame(&content, phys_width, phys_height, spec.dpr);
        let expected = pixels.clone();
        let frame = PanelFrame { pixels: pixels.clone(), width: phys_width, height: phys_height };

        let panel = <super::X11PanelContext as DisplayManager>::create_window(&mut ctx, &spec, &frame)
            .expect("create_window should succeed when X11 is available");

        assert_eq!(
            panel.bgrx, expected,
            "panel.bgrx should equal the pre-rendered frame pixels passed to create_window"
        );

        // Cleanup
        let _ = <super::X11PanelContext as DisplayManager>::delete_window(&mut ctx, panel);
    }

    // ---------------------------------------------------------------------------
    // R2: DisplayManager::update_position moves the window and updates state.
    // ---------------------------------------------------------------------------
    #[test]
    fn display_manager_update_position_moves_window_and_updates_state() {
        use crate::display_manager::DisplayManager;

        let mut ctx = match make_panel_ctx() {
            Some(c) => c,
            None => {
                println!("SKIP: no X11 display available");
                return;
            }
        };

        let mut panel = <super::X11PanelContext as DisplayManager>::create_window(
            &mut ctx,
            &make_spec("test-dm-pos", 100, 30),
            &blank_frame(100, 30),
        )
        .expect("create_window should succeed when X11 is available");

        let new_spec = crate::layout::PanelSpecData {
            id: "test-dm-pos".to_string(),
            x: 50,
            y: 20,
            width: 100,
            height: 30,
            anchor: None,
            outer_gap: 0,
            output: None,
            above: false,
            content: serde_json::Value::Null,
            dpr: 1.0,
        };

        <super::X11PanelContext as DisplayManager>::update_position(&mut ctx, &mut panel, &new_spec)
            .expect("update_position should succeed");

        // With dpr=1.0, mon_x=0, mon_y=0: win_x = 0 + (50 * 1.0) = 50, win_y = 0 + (20 * 1.0) = 20
        assert_eq!(panel.win_x, 50, "win_x should be updated to 50 after update_position");
        assert_eq!(panel.win_y, 20, "win_y should be updated to 20 after update_position");

        // Cleanup
        let _ = <super::X11PanelContext as DisplayManager>::delete_window(&mut ctx, panel);
    }

}
