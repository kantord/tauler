//! End-to-end check of the X11 `<wallpaper>` path against a nested X server.
//!
//! Runs in Xvfb rather than the live display: painting the desktop background
//! is not something a test suite gets to do to the machine it runs on.

use std::collections::HashMap;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use tauler::display_manager::DisplayManager;
use tauler::layout::{OutputInfo, SurfaceKind, SurfaceSpec};
use tauler::presentation::SurfaceFrame;
use tauler::x11::panel::X11PanelContext;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt, ImageFormat};
use x11rb::rust_connection::RustConnection;

const SCREEN: (u32, u32) = (1280, 1024);
/// The pretend monitor: the bottom-right quadrant, so the test can tell a
/// correctly offset blit from one that starts at the screen origin.
const MONITOR: (i16, i16, u32, u32) = (640, 512, 640, 512);
const RED_BGRX: [u8; 4] = [0, 0, 255, 0];

struct Xvfb {
    child: Child,
    display: String,
}

impl Drop for Xvfb {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Start Xvfb on a free display at or above `base`, or `None` if it is not
/// installed. Tests pass distinct bases: connecting to a display another test
/// already owns would let the two stomp on each other's root background.
fn start_xvfb(base: u32) -> Option<(Xvfb, RustConnection, usize)> {
    for n in base..base + 5 {
        let display = format!(":{n}");
        let Ok(child) = Command::new("Xvfb")
            .args([
                &display,
                "-screen",
                "0",
                &format!("{}x{}x24", SCREEN.0, SCREEN.1),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            return None; // Xvfb not installed
        };
        let mut server = Xvfb { child, display };
        for _ in 0..100 {
            std::thread::sleep(std::time::Duration::from_millis(50));
            if matches!(server.child.try_wait(), Ok(Some(_))) {
                break; // display already taken; that server is not ours
            }
            if let Ok((conn, screen_num)) = RustConnection::connect(Some(&server.display)) {
                return Some((server, conn, screen_num));
            }
        }
    }
    None
}

fn make_ctx(conn: RustConnection, screen_num: usize) -> X11PanelContext {
    let screen = conn.setup().roots[screen_num].clone();
    let (mon_x, mon_y, mon_w, mon_h) = MONITOR;
    let mut output_map = HashMap::new();
    output_map.insert(
        "VIRT-1".to_string(),
        OutputInfo {
            name: "VIRT-1".to_string(),
            x: mon_x,
            y: mon_y,
            width: mon_w,
            height: mon_h,
            dpr: 1.0,
        },
    );
    X11PanelContext {
        root: screen.root,
        depth: screen.root_depth,
        root_visual: screen.root_visual,
        black_pixel: screen.black_pixel,
        conn: Arc::new(conn),
        dpr: 1.0,
        xrootpmap_atom: None,
        output_map: Arc::new(output_map),
        dpi: 96.0,
        output_name: "VIRT-1".to_string(),
        screen_width_logical: mon_w,
        screen_height_logical: mon_h,
        root_screen_width: SCREEN.0,
        root_screen_height: SCREEN.1,
        root_bg: None,
    }
}

fn wallpaper_spec() -> SurfaceSpec {
    let (_, _, w, h) = MONITOR;
    SurfaceSpec {
        id: "bg".to_string(),
        kind: SurfaceKind::Wallpaper,
        anchor: None,
        width: w,
        height: h,
        x: 0,
        y: 0,
        outer_gap: 0,
        output: Some("VIRT-1".to_string()),
        above: false,
        content: serde_json::Value::Null,
        dpr: 1.0,
    }
}

fn solid_frame(pixel: [u8; 4]) -> SurfaceFrame {
    let (_, _, w, h) = MONITOR;
    SurfaceFrame {
        pixels: Arc::new(pixel.repeat((w * h) as usize)),
        width: w,
        height: h,
    }
}

/// The pixmap the root window's background now points at.
fn published_root_pixmap(conn: &RustConnection, root: u32) -> u32 {
    let atom = conn
        .intern_atom(false, b"_XROOTPMAP_ID")
        .unwrap()
        .reply()
        .unwrap()
        .atom;
    let reply = conn
        .get_property(false, root, atom, AtomEnum::PIXMAP, 0, 1)
        .unwrap()
        .reply()
        .unwrap();
    reply
        .value32()
        .and_then(|mut v| v.next())
        .expect("_XROOTPMAP_ID must name a pixmap after a wallpaper is created")
}

/// The BGRX value of one pixel of a drawable.
fn pixel_at(conn: &RustConnection, drawable: u32, x: i16, y: i16) -> Vec<u8> {
    conn.get_image(ImageFormat::Z_PIXMAP, drawable, x, y, 1, 1, !0)
        .unwrap()
        .reply()
        .unwrap()
        .data
}

#[test]
fn wallpaper_paints_only_its_output_into_the_root_background() {
    let Some((_xvfb, conn, screen_num)) = start_xvfb(90) else {
        println!("SKIP: Xvfb not available");
        return;
    };
    let mut ctx = make_ctx(conn, screen_num);
    let root = ctx.root;
    let (mon_x, mon_y, mon_w, mon_h) = MONITOR;

    ctx.paint_wallpaper(&wallpaper_spec(), &solid_frame(RED_BGRX))
        .expect("paint_wallpaper should succeed");

    let pixmap = published_root_pixmap(&ctx.conn, root);
    let geometry = ctx.conn.get_geometry(pixmap).unwrap().reply().unwrap();
    assert_eq!(
        (geometry.width as u32, geometry.height as u32),
        SCREEN,
        "the root background pixmap must span the whole X screen"
    );

    assert_eq!(
        pixel_at(&ctx.conn, pixmap, mon_x, mon_y)[..3],
        RED_BGRX[..3],
        "the monitor's top-left corner must carry the frame"
    );
    assert_eq!(
        pixel_at(
            &ctx.conn,
            pixmap,
            mon_x + mon_w as i16 - 1,
            mon_y + mon_h as i16 - 1
        )[..3],
        RED_BGRX[..3],
        "the monitor's bottom-right corner must carry the frame"
    );
    assert_eq!(
        pixel_at(&ctx.conn, pixmap, mon_x - 1, mon_y - 1)[..3],
        [0, 0, 0],
        "a wallpaper must not bleed outside its own output"
    );
}

#[test]
fn wallpaper_update_replaces_the_painted_region() {
    let Some((_xvfb, conn, screen_num)) = start_xvfb(95) else {
        println!("SKIP: Xvfb not available");
        return;
    };
    let mut ctx = make_ctx(conn, screen_num);
    let root = ctx.root;
    let (mon_x, mon_y, _, _) = MONITOR;

    ctx.paint_wallpaper(&wallpaper_spec(), &solid_frame(RED_BGRX))
        .expect("paint_wallpaper should succeed");
    let pixmap = published_root_pixmap(&ctx.conn, root);

    let blue: [u8; 4] = [255, 0, 0, 0];
    ctx.paint_wallpaper(&wallpaper_spec(), &solid_frame(blue))
        .expect("repaint should succeed");

    assert_eq!(
        pixel_at(&ctx.conn, pixmap, mon_x, mon_y)[..3],
        blue[..3],
        "a repaint must land in the same pixmap, not a new one"
    );
    assert_eq!(
        published_root_pixmap(&ctx.conn, root),
        pixmap,
        "repainting must reuse the published pixmap so nothing leaks per tick"
    );
}
