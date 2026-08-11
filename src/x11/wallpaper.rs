//! Painting layout subtrees into the X11 desktop background.
//!
//! A `<wallpaper>` node is rasterized exactly like a `<panel>` — takumi hands
//! back a BGRX buffer the size of the target display. The only difference is
//! the destination: instead of an override-redirect window, the buffer is
//! blitted into the root window's background pixmap.
//!
//! There is one such pixmap for the whole X screen (that is what X11 gives us),
//! sized to the root screen and shared by every wallpaper node. Each node owns
//! the rectangle of its own output, clipped so a node can never bleed onto a
//! neighbouring monitor.
//!
//! `_XROOTPMAP_ID` / `ESETROOT_PMAP_ID` are published the way `feh` and
//! `hsetroot` do it, so pseudo-transparent clients (and tauler's own `root-bg`
//! image source) can find the wallpaper.

use std::sync::Arc;

use x11rb::{
    connection::Connection, protocol::xproto::*, rust_connection::RustConnection,
    wrapper::ConnectionExt as _,
};

use crate::layout::SurfaceSpec;
use crate::presentation::SurfaceFrame;
use crate::x11::panel::{put_image_chunked_at, X11PanelContext};

/// The shared root-window background pixmap, sized to the whole X screen.
pub struct RootBackground {
    pixmap: Pixmap,
    gc: Gcontext,
    width: u32,
    height: u32,
}

fn intern(conn: &RustConnection, name: &[u8]) -> Option<Atom> {
    conn.intern_atom(false, name)
        .ok()?
        .reply()
        .ok()
        .map(|r| r.atom)
}

/// The physical rectangle of the output a spec targets, in root-screen coordinates.
fn output_rect(ctx: &X11PanelContext, output: Option<&str>) -> anyhow::Result<Rectangle> {
    let name = output.unwrap_or(&ctx.output_name);
    let info = ctx
        .output_map
        .get(name)
        .ok_or_else(|| anyhow::anyhow!("output '{}' not in map", name))?;
    Ok(Rectangle {
        x: info.x,
        y: info.y,
        width: info.width as u16,
        height: info.height as u16,
    })
}

/// Read the pixmap another wallpaper setter left behind, if any.
///
/// Seeding from it means a `<wallpaper>` node covering one monitor leaves the
/// other monitors showing whatever was already there, instead of blanking them.
fn existing_root_pixmap(conn: &RustConnection, root: Window, atom: Atom) -> Option<Pixmap> {
    let reply = conn
        .get_property(false, root, atom, AtomEnum::PIXMAP, 0, 1)
        .ok()?
        .reply()
        .ok()?;
    let pixmap = reply.value32()?.next()?;
    // The property can outlive the pixmap it names; only use it if it is still real.
    conn.get_geometry(pixmap).ok()?.reply().ok()?;
    Some(pixmap)
}

impl X11PanelContext {
    /// The root background pixmap, creating (or re-creating, after a screen
    /// resize) and publishing it as needed. Returns its pixmap and GC.
    fn ensure_root_background(&mut self) -> anyhow::Result<(Pixmap, Gcontext)> {
        let (width, height) = (self.root_screen_width, self.root_screen_height);
        if let Some(bg) = &self.root_bg {
            if bg.width == width && bg.height == height {
                return Ok((bg.pixmap, bg.gc));
            }
        }
        let conn: Arc<RustConnection> = Arc::clone(&self.conn);
        let pixmap = conn.generate_id()?;
        conn.create_pixmap(self.depth, pixmap, self.root, width as u16, height as u16)?;
        let gc = conn.generate_id()?;
        conn.create_gc(gc, pixmap, &CreateGCAux::new().foreground(self.black_pixel))?;

        let xrootpmap = self
            .xrootpmap_atom
            .or_else(|| intern(&conn, b"_XROOTPMAP_ID"));
        // Seed before releasing the outgoing pixmap, so a screen resize carries
        // the old contents over instead of blanking every monitor.
        let previous =
            self.root_bg.as_ref().map(|bg| bg.pixmap).or_else(|| {
                xrootpmap.and_then(|atom| existing_root_pixmap(&conn, self.root, atom))
            });
        if let Some(old) = previous {
            conn.copy_area(old, pixmap, gc, 0, 0, 0, 0, width as u16, height as u16)?;
        } else {
            conn.poly_fill_rectangle(
                pixmap,
                gc,
                &[Rectangle {
                    x: 0,
                    y: 0,
                    width: width as u16,
                    height: height as u16,
                }],
            )?;
        }

        for atom in [xrootpmap, intern(&conn, b"ESETROOT_PMAP_ID")]
            .into_iter()
            .flatten()
        {
            conn.change_property32(
                PropMode::REPLACE,
                self.root,
                atom,
                AtomEnum::PIXMAP,
                &[pixmap],
            )?;
        }
        conn.change_window_attributes(
            self.root,
            &ChangeWindowAttributesAux::new().background_pixmap(pixmap),
        )?;
        conn.clear_area(false, self.root, 0, 0, 0, 0)?;
        conn.flush()?;

        if let Some(old) = self.root_bg.take() {
            let _ = conn.free_gc(old.gc);
            let _ = conn.free_pixmap(old.pixmap);
        }
        self.root_bg = Some(RootBackground {
            pixmap,
            gc,
            width,
            height,
        });
        Ok((pixmap, gc))
    }

    /// Blit `frame` into `rect` of the root background and repaint that region.
    ///
    /// The GC is clipped to `rect`, so a frame that does not divide evenly by
    /// the output's DPR (and so comes out a pixel too wide) is cut off at the
    /// monitor edge rather than bleeding onto its neighbour.
    fn blit_to_root(&mut self, rect: Rectangle, frame: &SurfaceFrame) -> anyhow::Result<()> {
        let (pixmap, gc) = self.ensure_root_background()?;
        let conn: Arc<RustConnection> = Arc::clone(&self.conn);

        conn.set_clip_rectangles(
            ClipOrdering::UNSORTED,
            gc,
            rect.x,
            rect.y,
            &[Rectangle {
                x: 0,
                y: 0,
                width: rect.width,
                height: rect.height,
            }],
        )?;
        put_image_chunked_at(
            &conn,
            pixmap,
            gc,
            frame.width,
            self.depth,
            &frame.pixels[..],
            rect.x,
            rect.y,
        )?;
        conn.set_clip_rectangles(ClipOrdering::UNSORTED, gc, 0, 0, &[])?;

        conn.clear_area(false, self.root, rect.x, rect.y, rect.width, rect.height)?;
        conn.flush()?;
        Ok(())
    }

    pub(crate) fn paint_wallpaper_impl(
        &mut self,
        spec: &SurfaceSpec,
        frame: &SurfaceFrame,
    ) -> anyhow::Result<()> {
        let rect = output_rect(self, spec.output.as_deref())?;
        self.blit_to_root(rect, frame)
    }
}
