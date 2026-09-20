// taulerbox: the CLI over the library in lib.rs. Argument parsing and the
// window loop live here; layout placement math lives in `compose`, where it
// can be tested without a display server.
//
// The window is resizable (issue #582). A resize has two independent
// reactions, one instant and one debounced:
//
//   1. Panel placement is pure arithmetic over already-rasterized pixels
//      (`compose::place` + a cached RGBA buffer per panel), so it is redone
//      on every single `WindowEvent::Resized` with no debounce — there is no
//      subprocess round trip to protect against.
//   2. The compartment's nested Sway desktop is a live VM: telling it to
//      change its own output resolution is a slow `msb exec` round trip.
//      Doing that on every resize event fired mid-drag would be far too
//      slow, so it is debounced — run only once resizing has settled for
//      ~300ms (see `pending_resize` on [`App`]). Until the debounced step
//      completes, the stale VNC frame is immediately re-fit
//      (aspect-preserved, no stretch) into the new destination rect, so the
//      window never looks frozen mid-drag. No explicit re-fetch of a fresh
//      frame is needed once Sway applies the new resolution: the persistent
//      VNC connection (see [`spawn_vnc_worker`]) is already streaming and
//      picks up the server's own `SetResolution`/redraw on its own.
//
// The VNC connection itself is persistent for the program's lifetime, not a
// one-shot connect-grab-one-frame-disconnect (issue #582's live-mirror step):
// one dedicated OS thread (`spawn_vnc_worker`) owns a tokio runtime and the
// `vnc::VncClient`, continuously receiving framebuffer updates and
// forwarding host pointer events, connected to the window (`ApplicationHandler`
// on the main thread, driven by winit) through two channels — see
// `spawn_vnc_worker`'s doc comment for the full shape.
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{mpsc as std_mpsc, Arc, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use clap::Parser;
use taulerbox::compose::{place, PlacedPanel};
use tokio::sync::mpsc as tokio_mpsc;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowId};

/// The window's size when first created. Purely a starting point now that
/// the window is resizable — [`App::width`]/[`App::height`] track the live
/// size after that. Chosen the same way the old fixed size was: big enough
/// to show the `hello`/`compartment` fixtures' 300x200 panel alongside a
/// compartment's composited VNC view (see [`vnc_dest_rect`]).
const INITIAL_WINDOW_WIDTH: u32 = 900;
const INITIAL_WINDOW_HEIGHT: u32 = 700;

/// How long to wait, after the last `WindowEvent::Resized`, before treating
/// a resize as "settled" and running the slow compartment-resize step (see
/// module docs above and [`App::pending_resize`]).
const RESIZE_DEBOUNCE: Duration = Duration::from_millis(300);

/// Fixed gap, in pixels, used to derive the compartment's VNC destination
/// rect from the current window size and the width of whatever panel is
/// flush against the window's left edge (see [`vnc_dest_rect`]). Reverse-
/// engineered from the fixture's original hardcoded rect: a 900x700 window
/// with a 300px-wide left panel produced a 520x620 dest rect at (340, 40) —
/// i.e. `dest_x = panel_width + MARGIN`, `dest_y = MARGIN`,
/// `dest_width = window_width - dest_x - MARGIN`,
/// `dest_height = window_height - 2*MARGIN`, all with `MARGIN = 40`.
const VNC_DEST_MARGIN: u32 = 40;

/// A native window that shows a tauler layout's Panels as pixel buffers,
/// alongside a microVM compartment.
#[derive(Parser)]
#[command(name = "taulerbox")]
struct Args {
    /// The layout file to evaluate (e.g. `layout.op.mdx`).
    layout: PathBuf,

    /// Bind-mounted as the compartment's home directory.
    #[arg(long)]
    home: PathBuf,

    /// `HOST:PORT` of a live VNC server (e.g. a compartment's wayvnc) to
    /// connect to and composite into the window next to the panel (see
    /// [`vnc_dest_rect`]). Optional: when absent, taulerbox behaves exactly
    /// as it does without this flag.
    #[arg(long)]
    vnc: Option<String>,

    /// Name of the `msb` sandbox to resize on the compartment's side of a
    /// window resize (`swaymsg output <output> resolution <w>x<h>` run via
    /// `msb exec`). Only meaningful together with `--vnc`; without it, a
    /// resize still repositions panels and re-fits the existing VNC frame
    /// into the new destination rect, it just cannot make Sway itself
    /// change resolution. The `compartment` fixture's sandbox is named
    /// `taulerbox-verify-582` (see `fixtures/compartment/layout.op.mdx`).
    #[arg(long)]
    compartment_name: Option<String>,
}

/// Alpha-composites one RGBA pixel over an opaque `0RGB` background pixel and
/// packs the result the way softbuffer wants it: `0RGB` in a `u32`, top byte
/// ignored.
fn composite_over(bg: u32, r: u8, g: u8, b: u8, a: u8) -> u32 {
    let bg_r = ((bg >> 16) & 0xff) as u16;
    let bg_g = ((bg >> 8) & 0xff) as u16;
    let bg_b = (bg & 0xff) as u16;
    let a = a as u16;
    let blend = |bg: u16, fg: u8| -> u32 {
        (bg + (fg as u16).wrapping_sub(bg).wrapping_mul(a) / 255) as u32
    };
    // `wrapping_sub`/`wrapping_mul` on u16 stay in range here: fg/bg are both
    // 0..=255 and a is 0..=255, so bg + (fg-bg)*a/255 never leaves 0..=255,
    // it just has to get there through a signed-looking intermediate.
    let out_r = blend(bg_r, r);
    let out_g = blend(bg_g, g);
    let out_b = blend(bg_b, b);
    (out_r << 16) | (out_g << 8) | out_b
}

/// One panel's rasterized content, cached by id after the first (and only)
/// render. A panel's own pixel size depends only on its `SurfaceSpec`
/// (width/height/dpr), never on the window size, so a resize never needs to
/// re-rasterize this — only recompute where it goes (`compose::place`).
struct PanelPixels {
    width: u32,
    height: u32,
    rgba: Arc<Vec<u8>>,
}

/// Rasterizes every `Panel` spec exactly once, keyed by id, at its own
/// native physical size (independent of window size).
fn rasterize_panels(specs: &[tauler::layout::SurfaceSpec]) -> HashMap<String, PanelPixels> {
    specs
        .iter()
        .filter(|spec| spec.kind == tauler::layout::SurfaceKind::Panel)
        .map(|spec| {
            let width = (spec.width as f32 * spec.dpr).round().max(1.0) as u32;
            let height = (spec.height as f32 * spec.dpr).round().max(1.0) as u32;
            let rgba = tauler::render_frame_rgba(&spec.content, width, height, spec.dpr, None);
            (spec.id.clone(), PanelPixels { width, height, rgba })
        })
        .collect()
}

/// Blits one cached panel's pixels into `framebuffer` (a `width * height`
/// `0RGB` buffer) at `(dest_x, dest_y)`. Any part of the panel that falls
/// outside the framebuffer is simply skipped — this is what makes it safe to
/// always blit a panel's full native size even when `compose::place` has
/// clamped its visible rect to a smaller area near the window's far edge.
fn blit_panel(framebuffer: &mut [u32], fb_width: u32, fb_height: u32, pixels: &PanelPixels, dest_x: i32, dest_y: i32) {
    const BACKGROUND: u32 = 0x0000_0000;
    for row in 0..pixels.height {
        let dst_y = dest_y + row as i32;
        if dst_y < 0 || dst_y >= fb_height as i32 {
            continue;
        }
        for col in 0..pixels.width {
            let dst_x = dest_x + col as i32;
            if dst_x < 0 || dst_x >= fb_width as i32 {
                continue;
            }
            let src_idx = ((row * pixels.width + col) as usize) * 4;
            let Some(px) = pixels.rgba.get(src_idx..src_idx + 4) else {
                continue;
            };
            let (r, g, b, a) = (px[0], px[1], px[2], px[3]);
            let dest_idx = (dst_y as usize) * (fb_width as usize) + dst_x as usize;
            if let Some(dest) = framebuffer.get_mut(dest_idx) {
                *dest = composite_over(BACKGROUND, r, g, b, a);
            }
        }
    }
}

/// Where the compartment's composited VNC frame goes: to the right of
/// whichever panel is flush against the window's left edge, with a fixed
/// margin on every other side (see [`VNC_DEST_MARGIN`]).
fn vnc_dest_rect(window_width: u32, window_height: u32, placements: &[PlacedPanel]) -> (u32, u32, u32, u32) {
    let left_panel_width = placements
        .iter()
        .filter(|p| p.visible && p.x == 0)
        .map(|p| p.width)
        .max()
        .unwrap_or(0);

    let dest_x = left_panel_width + VNC_DEST_MARGIN;
    let dest_y = VNC_DEST_MARGIN;
    let dest_width = window_width.saturating_sub(dest_x + VNC_DEST_MARGIN);
    let dest_height = window_height.saturating_sub(2 * VNC_DEST_MARGIN);
    (dest_x, dest_y, dest_width, dest_height)
}

/// `true` when `(x, y)` (window-local physical pixels) falls inside `rect`
/// (`dest_x, dest_y, dest_width, dest_height`, the same tuple shape
/// [`vnc_dest_rect`] returns). Shared by pointer forwarding (is the host
/// cursor over the compartment at all) and host-cursor-visibility toggling
/// (issue #582) — both care about the destination rect's *outer* box, before
/// any aspect-fit letterboxing is taken into account.
fn point_in_rect(x: f64, y: f64, rect: (u32, u32, u32, u32)) -> bool {
    let (rx, ry, rw, rh) = rect;
    x >= rx as f64 && y >= ry as f64 && x < (rx + rw) as f64 && y < (ry + rh) as f64
}

/// The aspect-fit sub-rect that [`blit_vnc_frame`] paints a `frame_width x
/// frame_height` VNC frame into, centered within a `dest_width x
/// dest_height` box: `(offset_x, offset_y, fit_width, fit_height)`, all
/// relative to the box's own origin (add `dest_x`/`dest_y` to place it in
/// framebuffer space). `None` when there is nothing sensible to fit (a zero
/// dimension anywhere).
///
/// Pulled out of `blit_vnc_frame` (issue #582) so pointer-event forwarding
/// can map a cursor position into the VNC frame's own coordinate space using
/// the exact same arithmetic used to paint it — two independent
/// implementations of this rounding would drift the moment either one
/// changed.
fn vnc_fit_rect(
    frame_width: u32,
    frame_height: u32,
    dest_width: u32,
    dest_height: u32,
) -> Option<(u32, u32, u32, u32)> {
    if frame_width == 0 || frame_height == 0 || dest_width == 0 || dest_height == 0 {
        return None;
    }
    let scale =
        (dest_width as f64 / frame_width as f64).min(dest_height as f64 / frame_height as f64);
    let fit_width = ((frame_width as f64 * scale).round() as u32).max(1);
    let fit_height = ((frame_height as f64 * scale).round() as u32).max(1);
    let offset_x = (dest_width - fit_width) / 2;
    let offset_y = (dest_height - fit_height) / 2;
    Some((offset_x, offset_y, fit_width, fit_height))
}

/// Maps a window-local physical cursor position into the live VNC frame's
/// own pixel coordinate space, via [`vnc_fit_rect`] — the same aspect-fit
/// rect [`blit_vnc_frame`] paints into. Used to forward host pointer
/// movement/clicks into the compartment with correct coordinates (issue
/// #582).
///
/// A position inside `dest_rect`'s outer box but in the surrounding
/// letterboxed margin (above/below or left/right of the fitted picture) is
/// clamped to the nearest edge of the picture rather than dropped: the
/// pointer "sticks" to the edge, which reads more naturally than having it
/// vanish near the edge of the visible frame. Returns `None` only when there
/// is no fit rect to map into at all (see [`vnc_fit_rect`]).
fn map_cursor_to_vnc_frame(
    cursor_x: f64,
    cursor_y: f64,
    dest_rect: (u32, u32, u32, u32),
    frame_width: u32,
    frame_height: u32,
) -> Option<(u16, u16)> {
    let (dest_x, dest_y, dest_width, dest_height) = dest_rect;
    let (offset_x, offset_y, fit_width, fit_height) =
        vnc_fit_rect(frame_width, frame_height, dest_width, dest_height)?;
    let fit_x = (dest_x + offset_x) as f64;
    let fit_y = (dest_y + offset_y) as f64;
    let scale_x = frame_width as f64 / fit_width as f64;
    let scale_y = frame_height as f64 / fit_height as f64;
    let frame_x = ((cursor_x - fit_x) * scale_x).clamp(0.0, (frame_width - 1) as f64);
    let frame_y = ((cursor_y - fit_y) * scale_y).clamp(0.0, (frame_height - 1) as f64);
    Some((frame_x.round() as u16, frame_y.round() as u16))
}

/// RFB's standard pointer button-mask bit for a `winit` mouse button (RFC
/// 6143 §7.5.5: bit 0 left, bit 1 middle, bit 2 right — confirmed against
/// `vnc-rs`'s own `ClientMsg::PointerEvent` wire encoding, which sends
/// whatever mask it is given verbatim with no reinterpretation). `None` for
/// buttons RFB has no bit for (`Back`/`Forward`/vendor-specific), which are
/// simply not forwarded.
fn mouse_button_bit(button: MouseButton) -> Option<u8> {
    match button {
        MouseButton::Left => Some(0x01),
        MouseButton::Middle => Some(0x02),
        MouseButton::Right => Some(0x04),
        _ => None,
    }
}

/// Builds a fresh `width * height` `0RGB` framebuffer from already-rendered
/// pieces: recomputes panel placement against the current window size and
/// re-blits each panel's cached pixels, then (if a VNC frame is available)
/// aspect-fits it into the current [`vnc_dest_rect`]. Cheap enough to call on
/// every `WindowEvent::Resized` — no rasterization happens here, only
/// arithmetic and pixel copies.
fn build_framebuffer(
    specs: &[tauler::layout::SurfaceSpec],
    panel_pixels: &HashMap<String, PanelPixels>,
    width: u32,
    height: u32,
    vnc_frame: Option<&VncFrame>,
) -> Vec<u32> {
    const BACKGROUND: u32 = 0x0000_0000;
    let mut framebuffer = vec![BACKGROUND; (width as usize) * (height as usize)];

    let placements = place(specs, (width, height));
    for placement in &placements {
        if !placement.visible {
            continue;
        }
        let Some(pixels) = panel_pixels.get(&placement.id) else {
            continue;
        };
        blit_panel(&mut framebuffer, width, height, pixels, placement.x, placement.y);
    }

    if let Some(frame) = vnc_frame {
        let (dest_x, dest_y, dest_width, dest_height) = vnc_dest_rect(width, height, &placements);
        blit_vnc_frame(&mut framebuffer, width, height, frame, dest_x, dest_y, dest_width, dest_height);
    }

    framebuffer
}

/// One framebuffer's worth of pixel data pulled from a VNC server, tightly
/// packed row-major RGBA (matching `vnc::PixelFormat::rgba()`, requested
/// below so the wire bytes are already `[r, g, b, a]` per pixel — no channel
/// reordering needed before blitting).
struct VncFrame {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

/// Connects to `addr` and completes the RFB handshake, retrying every 2
/// seconds for up to 200 seconds: a compartment's `postCreate` bootstrap
/// (`apt-get install sway wayvnc` plus VM boot) is given up to 180 seconds
/// by its own `msb exec --timeout`, and a live end-to-end run measured a
/// cold boot taking the full ~120s — a 120s budget here raced that and lost
/// by seconds. 200s comfortably clears the fixture's own 180s ceiling. The
/// very first attempts are expected to fail regardless, so this is not an
/// error worth logging loudly until the whole budget is exhausted.
async fn connect_vnc_with_retry(addr: &str) -> anyhow::Result<vnc::VncClient> {
    let budget = Duration::from_secs(200);
    let retry_interval = Duration::from_secs(2);
    let start = Instant::now();
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        match try_connect_vnc_once(addr).await {
            Ok(client) => {
                tracing::info!(
                    "taulerbox: VNC connected to {addr} on attempt {attempt} (after {:.1}s)",
                    start.elapsed().as_secs_f32()
                );
                return Ok(client);
            }
            Err(e) => {
                let elapsed = start.elapsed();
                if elapsed >= budget {
                    anyhow::bail!(
                        "VNC connect to {addr} gave up after {attempt} attempts over {:.1}s: {e}",
                        elapsed.as_secs_f32()
                    );
                }
                tracing::info!(
                    "taulerbox: VNC connect attempt {attempt} to {addr} failed ({e}); retrying in {}s (elapsed {:.1}s)",
                    retry_interval.as_secs(),
                    elapsed.as_secs_f32()
                );
                tokio::time::sleep(retry_interval).await;
            }
        }
    }
}

/// One TCP-connect-plus-RFB-handshake attempt. `wayvnc`'s default config
/// runs with no authentication (RFB `SecurityType::None`); vnc-rs's
/// connector only ever polls the auth-method future when the server actually
/// offers a password-based security type (confirmed by reading
/// `vnc::client::connector`'s handshake code), so a callback that always
/// returns an empty password is safe to install unconditionally here and
/// simply never runs against wayvnc.
async fn try_connect_vnc_once(addr: &str) -> anyhow::Result<vnc::VncClient> {
    let tcp = tokio::time::timeout(Duration::from_secs(2), tokio::net::TcpStream::connect(addr))
        .await
        .map_err(|_| anyhow::anyhow!("TCP connect to {addr} timed out"))?
        .map_err(|e| anyhow::anyhow!("TCP connect to {addr} failed: {e}"))?;

    vnc::VncConnector::new(tcp)
        .set_auth_method(async move { Ok(String::new()) })
        .add_encoding(vnc::VncEncoding::Raw)
        // Without this, wayvnc kills the connection (and, observed live,
        // the whole wayvnc process) the moment the compositor's output
        // resolution changes underneath an already-connected client that
        // never declared it can handle a resize — exactly what happens
        // here every debounced resize (`set_sway_output_resolution` runs,
        // then this client reconnects to fetch a fresh frame while Sway is
        // still applying the new mode). Advertising this pseudo-encoding is
        // enough: `vnc-rs` translates a resize notification using it
        // straight into the `VncEvent::SetResolution` `grab_one_frame`
        // already handles, no extra decoding logic needed.
        .add_encoding(vnc::VncEncoding::DesktopSizePseudo)
        .allow_shared(true)
        // Requests pixel data as [r, g, b, a] per pixel on the wire, so it
        // can be copied straight into `VncFrame::rgba` with no channel
        // shuffling.
        .set_pixel_format(vnc::PixelFormat::rgba())
        .build()
        .map_err(|e| anyhow::anyhow!("VNC configuration rejected: {e}"))?
        .try_start()
        .await
        .map_err(|e| anyhow::anyhow!("RFB handshake failed: {e}"))?
        .finish()
        .map_err(|e| anyhow::anyhow!("VNC connector finished in an unconnected state: {e}"))
}

/// Copies one `RawImage` rect's tightly-packed RGBA bytes into `canvas` (a
/// `width * height * 4` byte buffer, same layout as [`VncFrame::rgba`]).
/// Shared by [`spawn_vnc_worker`]'s per-event handling — pulled out on its
/// own since it's the one piece of by-hand pixel-copy arithmetic in that
/// loop worth naming and testing in isolation from the async plumbing
/// around it.
fn copy_rect_into_canvas(canvas: &mut [u8], width: u32, height: u32, rect: &vnc::Rect, data: &[u8]) {
    let rect_w = rect.width as usize;
    let rect_h = rect.height as usize;
    for row in 0..rect_h {
        let dst_y = rect.y as usize + row;
        if dst_y >= height as usize {
            continue;
        }
        let src_off = row * rect_w * 4;
        let len = rect_w * 4;
        let dst_off = (dst_y * width as usize + rect.x as usize) * 4;
        if let (Some(src), Some(dst)) = (
            data.get(src_off..src_off + len),
            canvas.get_mut(dst_off..dst_off + len),
        ) {
            dst.copy_from_slice(src);
        }
    }
}

/// Commands sent from the window thread into the persistent VNC worker
/// thread (see [`spawn_vnc_worker`]). Only pointer events for now — issue
/// #582 explicitly excludes keyboard forwarding, and there is nothing else
/// the window thread needs to push into a live connection.
enum VncCommand {
    /// An RFB `PointerEvent`: `x`/`y` in the VNC frame's own coordinate
    /// space (see [`map_cursor_to_vnc_frame`]), `mask` the *full* current
    /// button state (RFB pointer events are absolute, not deltas).
    Pointer { x: u16, y: u16, mask: u8 },
}

/// How often the worker asks the server for the next framebuffer update
/// while a connection is open. `vnc-rs` does not do this on its own — per
/// RFB, a client must explicitly request each update after the previous one
/// arrives (confirmed against the crate's own README example, which does
/// exactly this on a 16ms tick after flushing a rendered frame). 33ms (~30
/// requests/sec) is plenty for a letterboxed mirror rather than a full
/// remote-desktop client.
///
/// The request sent on each tick is `X11Event::FullRefresh` (non-incremental
/// — see the call site), not `Refresh`. Measured live (issue #582): a raw
/// RFB probe talking to `wayvnc` directly, bypassing taulerbox entirely,
/// showed incremental requests answered in single-digit milliseconds when a
/// real update was pending, but averaging ~1 second/frame overall — an
/// *incremental* request only gets a response once the server has real
/// damage to report, and on an idle headless desktop the only thing
/// generating damage was Sway's own status-bar clock ticking once a second.
/// Mouse-only movement did not reliably count as damage either, which is
/// why forwarded pointer motion felt sluggish. `FullRefresh` makes the
/// server answer unconditionally on every tick, trading a small constant
/// bandwidth cost (a full frame's worth of Raw pixels, `VNC_REFRESH_INTERVAL`
/// times a second) for updates that track input at the tick rate instead of
/// the compositor's own idle redraw cadence.
const VNC_REFRESH_INTERVAL: Duration = Duration::from_millis(33);

/// Spawns the one dedicated OS thread that owns the VNC connection for the
/// rest of the program's life (issue #582's persistent-connection step,
/// replacing the old one-shot connect-grab-one-frame-disconnect). The
/// thread:
///
///  - connects with the existing retry budget (`connect_vnc_with_retry`),
///    and reconnects with that same budget if the connection genuinely
///    drops (a `recv_event`/`input` call failing) rather than on every
///    resize the way the old one-shot design did;
///  - loops receiving framebuffer updates (`client.recv_event()`) and, for
///    each `RawImage` rect, patches its own persistent canvas
///    (`copy_rect_into_canvas`) and publishes a fresh copy to `frame_tx` — a
///    plain `std::sync::mpsc::Sender`, because the window thread is
///    synchronous (winit's own event loop) and only ever wants "whatever
///    the newest frame is", which `Receiver::try_iter` on the other end
///    gives for free;
///  - on a `VNC_REFRESH_INTERVAL` tick, sends a non-incremental
///    `X11Event::FullRefresh` so the server answers on every tick rather
///    than waiting for its own idle redraw cadence (RFB requires this
///    explicit re-request either way; see `VNC_REFRESH_INTERVAL`'s doc for
///    why non-incremental specifically);
///  - drains `cmd_rx` (a `tokio::sync::mpsc::UnboundedReceiver`, chosen over
///    a plain `std::sync::mpsc` receiver here because this side needs an
///    `.await`-able recv to sit in the same `tokio::select!` as the other
///    two branches above; its `Sender` half has a plain synchronous `send`,
///    so the window thread does not need to be async to use it) and
///    forwards pointer events into the live connection.
///
/// After publishing a frame, wakes the window thread via `proxy`
/// (`EventLoopProxy::send_event`) since a new frame is not tied to any
/// `WindowEvent` winit would otherwise wake up for on its own.
fn spawn_vnc_worker(
    addr: String,
    frame_tx: std_mpsc::Sender<Arc<VncFrame>>,
    mut cmd_rx: tokio_mpsc::UnboundedReceiver<VncCommand>,
    proxy: EventLoopProxy<()>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let runtime = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                eprintln!("taulerbox: could not start the VNC worker's tokio runtime: {e}");
                return;
            }
        };
        runtime.block_on(async move {
            loop {
                let client = match connect_vnc_with_retry(&addr).await {
                    Ok(client) => client,
                    Err(e) => {
                        eprintln!("taulerbox: giving up on the VNC connection to {addr}: {e}");
                        return;
                    }
                };

                let mut width: u32 = 0;
                let mut height: u32 = 0;
                let mut canvas: Vec<u8> = Vec::new();
                let mut refresh = tokio::time::interval(VNC_REFRESH_INTERVAL);
                refresh.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

                'connection: loop {
                    tokio::select! {
                        event = client.recv_event() => {
                            match event {
                                Ok(vnc::VncEvent::SetResolution(screen)) => {
                                    width = screen.width as u32;
                                    height = screen.height as u32;
                                    canvas = vec![0u8; (width as usize) * (height as usize) * 4];
                                    tracing::info!(
                                        "taulerbox: VNC framebuffer resolution is {width}x{height}"
                                    );
                                }
                                Ok(vnc::VncEvent::RawImage(rect, data)) => {
                                    if width == 0 || height == 0 {
                                        continue;
                                    }
                                    copy_rect_into_canvas(&mut canvas, width, height, &rect, &data);
                                    let frame = Arc::new(VncFrame {
                                        width,
                                        height,
                                        rgba: canvas.clone(),
                                    });
                                    if frame_tx.send(frame).is_err() {
                                        // Window thread is gone; the program is shutting down.
                                        return;
                                    }
                                    let _ = proxy.send_event(());
                                }
                                Ok(other) => {
                                    tracing::debug!(
                                        "taulerbox: ignoring VNC event on the live connection: {other:?}"
                                    );
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        "taulerbox: VNC connection to {addr} dropped ({e}); reconnecting"
                                    );
                                    break 'connection;
                                }
                            }
                        }
                        _ = refresh.tick() => {
                            if let Err(e) = client.input(vnc::X11Event::FullRefresh).await {
                                tracing::warn!(
                                    "taulerbox: could not request the next VNC frame ({e}); reconnecting"
                                );
                                break 'connection;
                            }
                        }
                        cmd = cmd_rx.recv() => {
                            match cmd {
                                Some(VncCommand::Pointer { x, y, mask }) => {
                                    if let Err(e) = client
                                        .input(vnc::X11Event::PointerEvent((x, y, mask).into()))
                                        .await
                                    {
                                        tracing::warn!(
                                            "taulerbox: could not forward a pointer event ({e}); reconnecting"
                                        );
                                        break 'connection;
                                    }
                                }
                                None => return, // Window closed; the program is shutting down.
                            }
                        }
                    }
                }
            }
        });
    })
}

/// Nearest-neighbor-scales `frame` into the `dest_width * dest_height` box at
/// `dest_x, dest_y` within `framebuffer` (a `width * height` `0RGB` buffer,
/// same convention as `build_framebuffer`'s output), preserving `frame`'s
/// own aspect ratio rather than stretching it to fill the box.
///
/// A VNC source (commonly 16:9, e.g. Sway's default headless output) and a
/// destination box chosen for the window's layout are two independent
/// aspect ratios with no reason to match — stretching one onto the other
/// unconditionally produced a visibly squished picture (issue #582). The
/// fitted rect is centered in the box; any leftover strip (top/bottom or
/// left/right) is left untouched, which is the framebuffer's existing
/// background color, not explicitly painted here.
///
/// No alpha blending: VNC framebuffers are opaque, so the destination pixel
/// is simply overwritten.
fn blit_vnc_frame(
    framebuffer: &mut [u32],
    fb_width: u32,
    fb_height: u32,
    frame: &VncFrame,
    dest_x: u32,
    dest_y: u32,
    dest_width: u32,
    dest_height: u32,
) {
    let Some((offset_x, offset_y, fit_width, fit_height)) =
        vnc_fit_rect(frame.width, frame.height, dest_width, dest_height)
    else {
        return;
    };
    let fit_x = dest_x + offset_x;
    let fit_y = dest_y + offset_y;

    for row in 0..fit_height {
        let dst_y = fit_y + row;
        if dst_y >= fb_height {
            continue;
        }
        let src_y = ((row as u64 * frame.height as u64) / fit_height as u64) as u32;
        let src_y = src_y.min(frame.height - 1);

        for col in 0..fit_width {
            let dst_x = fit_x + col;
            if dst_x >= fb_width {
                continue;
            }
            let src_x = ((col as u64 * frame.width as u64) / fit_width as u64) as u32;
            let src_x = src_x.min(frame.width - 1);

            let src_idx = ((src_y * frame.width + src_x) * 4) as usize;
            let Some(px) = frame.rgba.get(src_idx..src_idx + 4) else {
                continue;
            };
            let (r, g, b) = (px[0], px[1], px[2]);
            let packed = ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);

            let dst_idx = (dst_y as usize) * (fb_width as usize) + dst_x as usize;
            if let Some(dest) = framebuffer.get_mut(dst_idx) {
                *dest = packed;
            }
        }
    }
}

/// Runs `bash -c script` inside the `msb` sandbox `name` and returns its
/// stdout. Used for the two debounced-resize steps that need to reach into
/// the compartment: discovering the real headless output name, and telling
/// Sway to change its resolution (see [`discover_sway_output`] and
/// [`set_sway_output_resolution`]).
fn msb_exec(name: &str, script: &str) -> anyhow::Result<String> {
    let output = Command::new("msb")
        .args(["exec", name, "--no-tty", "--timeout", "20", "--", "bash", "-c", script])
        .output()
        .map_err(|e| anyhow::anyhow!("failed to spawn `msb exec {name}`: {e}"))?;
    anyhow::ensure!(
        output.status.success(),
        "`msb exec {name}` failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// A `swaymsg`-and-friends command line, run inside the compartment, that
/// resolves `SWAYSOCK` itself first. Confirmed empirically: with only
/// `XDG_RUNTIME_DIR`/`WAYLAND_DISPLAY` set, `swaymsg` in this headless setup
/// fails with "Unable to retrieve socket path" — it does not glob
/// `$XDG_RUNTIME_DIR` for the IPC socket the way a full session would, so
/// `SWAYSOCK` has to be located and exported explicitly before calling it.
fn swaymsg_script(command: &str) -> String {
    format!(
        "SWAYSOCK=$(ls /run/user/0/sway-ipc.*.sock | head -1); \
         XDG_RUNTIME_DIR=/run/user/0 WAYLAND_DISPLAY=wayland-1 SWAYSOCK=$SWAYSOCK {command}"
    )
}

/// Discovers the compartment's real headless Sway output name (e.g.
/// `HEADLESS-1`) by asking Sway itself (`swaymsg -t get_outputs`) rather
/// than assuming it — a name chosen by `wlroots` at runtime, not guaranteed
/// stable across versions. Takes the first output reported; the fixture's
/// headless backend only ever creates one.
fn discover_sway_output(compartment_name: &str) -> anyhow::Result<String> {
    let out = msb_exec(compartment_name, &swaymsg_script("swaymsg -t get_outputs"))?;
    let outputs: serde_json::Value = serde_json::from_str(&out)
        .map_err(|e| anyhow::anyhow!("could not parse `swaymsg -t get_outputs` output as JSON: {e} (output was: {out:?})"))?;
    outputs
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|o| o.get("name"))
        .and_then(|n| n.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("`swaymsg -t get_outputs` reported no outputs (output was: {out:?})"))
}

/// Runs `swaymsg output <output> resolution <width>x<height>` inside the
/// compartment, changing Sway's own compositor output resolution — not just
/// the client-side destination rect a VNC frame gets fit into.
fn set_sway_output_resolution(compartment_name: &str, output: &str, width: u32, height: u32) -> anyhow::Result<()> {
    msb_exec(compartment_name, &swaymsg_script(&format!("swaymsg output {output} resolution {width}x{height}")))?;
    Ok(())
}

/// Owns the one window this build ever opens: the evaluated layout's specs,
/// each panel's cached pixels, the current window size, the last VNC frame
/// (if any), and enough state to run the debounced Sway-resize step
/// (`vnc_addr`/`compartment_name`/`vnc_output_name`/`pending_resize`) and to
/// forward host pointer input into the persistent VNC connection
/// (`vnc_cmd_tx`/`cursor_pos`/`cursor_buttons`/`host_cursor_hidden`). See the
/// module docs for the two-tier (instant reposition, debounced compartment
/// resize) resize story and for the worker-thread shape (`spawn_vnc_worker`).
struct App {
    width: u32,
    height: u32,
    specs: Vec<tauler::layout::SurfaceSpec>,
    panel_pixels: HashMap<String, PanelPixels>,
    vnc_addr: Option<String>,
    compartment_name: Option<String>,
    last_vnc_frame: Option<Arc<VncFrame>>,
    /// The window thread's end of the frame channel out of
    /// [`spawn_vnc_worker`]. `None` when `--vnc` was not given. Drained on
    /// every `user_event` wake-up (see [`ApplicationHandler::user_event`]),
    /// keeping only the newest frame with `Receiver::try_iter`.
    vnc_frame_rx: Option<std_mpsc::Receiver<Arc<VncFrame>>>,
    /// The window thread's end of the command channel into
    /// [`spawn_vnc_worker`], used to forward pointer events. `None` when
    /// `--vnc` was not given, in which case pointer forwarding and
    /// host-cursor hiding are both simply skipped.
    vnc_cmd_tx: Option<tokio_mpsc::UnboundedSender<VncCommand>>,
    /// Kept alive for the process's lifetime; the worker thread runs
    /// detached (dropping a `JoinHandle` does not stop it), this field just
    /// avoids an "unused" warning for the handle `spawn_vnc_worker` returns.
    _vnc_thread: Option<thread::JoinHandle<()>>,
    /// The most recent `CursorMoved` position (window-local physical
    /// pixels), or `None` while the cursor is outside the window
    /// (`CursorLeft`). Re-checked against the live `vnc_dest_rect` on every
    /// move and on every button/wheel event, since RFB pointer events are
    /// always sent with the cursor's current position, not just on move.
    cursor_pos: Option<(f64, f64)>,
    /// The RFB pointer button mask currently held down, built up from
    /// `WindowEvent::MouseInput` (see [`mouse_button_bit`]). Sent as part of
    /// every forwarded pointer event, per RFB's "always the full current
    /// state" convention.
    cursor_buttons: u8,
    /// Whether the host cursor is currently hidden (because the cursor is
    /// over the VNC destination rect). Tracked so `set_cursor_visible` is
    /// only called on an actual transition, not redundantly on every event.
    host_cursor_hidden: bool,
    /// Cached after the first discovery — the headless output's name never
    /// changes for the lifetime of the compartment, so there is no reason to
    /// re-run `swaymsg -t get_outputs` on every resize.
    vnc_output_name: Option<String>,
    /// Set by a `WindowEvent::Resized` and cleared once the debounce window
    /// (see `RESIZE_DEBOUNCE`) has elapsed with no further resize events —
    /// at which point the settled size drives the slow compartment-resize
    /// step. A later resize event simply overwrites this with a later
    /// deadline, which is the entire debounce mechanism.
    pending_resize: Option<Instant>,
    framebuffer: Vec<u32>,
    window: Option<Arc<Window>>,
    surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    // Unused after setup, but softbuffer ties the surface's lifetime to its
    // context, so this has to outlive `surface`.
    _context: Option<softbuffer::Context<Arc<Window>>>,
}

impl App {
    fn present(&mut self) {
        let Some(surface) = self.surface.as_mut() else {
            return;
        };
        match surface.buffer_mut() {
            Ok(mut buffer) => {
                buffer.copy_from_slice(&self.framebuffer);
                if let Err(e) = buffer.present() {
                    eprintln!("taulerbox: buffer present failed: {e}");
                }
            }
            Err(e) => eprintln!("taulerbox: buffer_mut failed: {e}"),
        }
    }

    /// Recomputes the framebuffer from current state (window size, cached
    /// panel pixels, last VNC frame) with no rasterization and no
    /// subprocess calls — cheap enough to run on every resize event.
    fn rebuild_framebuffer(&mut self) {
        self.framebuffer = build_framebuffer(
            &self.specs,
            &self.panel_pixels,
            self.width,
            self.height,
            self.last_vnc_frame.as_deref(),
        );
    }

    /// The slow half of a resize: tell the compartment's Sway to actually
    /// change its output resolution. Runs only after a resize has settled
    /// (see `pending_resize`). Best-effort: a failure here just leaves the
    /// instantly-refit stale frame on screen, logged rather than fatal — the
    /// same "must not stop the window from working" posture the initial
    /// `--vnc` connection already has in `main()`.
    ///
    /// No explicit re-fetch of a fresh frame follows this: the persistent
    /// VNC connection (`spawn_vnc_worker`, already running for the life of
    /// the program) advertises `DesktopSizePseudo` and so receives a fresh
    /// `VncEvent::SetResolution` plus repopulated `RawImage` rects on its
    /// own as soon as Sway applies the new mode — published to `last_vnc_frame`
    /// exactly like any other live update, via the usual `user_event` path.
    fn handle_settled_resize(&mut self) {
        if self.vnc_addr.is_none() {
            return;
        }
        let Some(compartment_name) = self.compartment_name.clone() else {
            return;
        };

        let placements = place(&self.specs, (self.width, self.height));
        let (_, _, dest_width, dest_height) = vnc_dest_rect(self.width, self.height, &placements);
        if dest_width == 0 || dest_height == 0 {
            return;
        }

        if self.vnc_output_name.is_none() {
            match discover_sway_output(&compartment_name) {
                Ok(name) => {
                    tracing::info!("taulerbox: discovered compartment output {name}");
                    self.vnc_output_name = Some(name);
                }
                Err(e) => {
                    eprintln!("taulerbox: could not discover compartment output: {e}");
                    return;
                }
            }
        }
        let output = self.vnc_output_name.clone().expect("just set above");

        if let Err(e) = set_sway_output_resolution(&compartment_name, &output, dest_width, dest_height) {
            eprintln!("taulerbox: could not resize compartment output {output}: {e}");
        }
    }

    /// Recomputes whether the host cursor is currently over the VNC
    /// destination rect and, if so, forwards its position and the current
    /// button state into the compartment. Called on every `CursorMoved` and
    /// on every button-state change, since RFB pointer events always carry
    /// the *full* current button mask, not just deltas (issue #582's mouse +
    /// click forwarding). A no-op when `--vnc` was not given.
    fn sync_pointer(&mut self) {
        if self.vnc_cmd_tx.is_none() {
            return;
        }

        let Some((cursor_x, cursor_y)) = self.cursor_pos else {
            self.set_host_cursor_hidden(false);
            return;
        };

        let placements = place(&self.specs, (self.width, self.height));
        let dest_rect = vnc_dest_rect(self.width, self.height, &placements);
        let over_vnc = point_in_rect(cursor_x, cursor_y, dest_rect);
        self.set_host_cursor_hidden(over_vnc);
        if !over_vnc {
            return;
        }

        let Some(frame) = &self.last_vnc_frame else {
            return;
        };
        let Some((frame_x, frame_y)) =
            map_cursor_to_vnc_frame(cursor_x, cursor_y, dest_rect, frame.width, frame.height)
        else {
            return;
        };
        self.send_vnc_pointer(frame_x, frame_y, self.cursor_buttons);
    }

    /// Forwards one wheel "click": RFB has no continuous scroll delta, so a
    /// scroll tick is modeled as a button-mask bit (4 = wheel up, 5 = wheel
    /// down) pressed and immediately released, the same way every RFB
    /// server expects it. Skipped (same as `sync_pointer`) when the host
    /// cursor is not currently over the VNC destination rect or no live
    /// frame is available to map its position into.
    fn send_vnc_wheel(&mut self, up: bool) {
        if self.vnc_cmd_tx.is_none() {
            return;
        }
        let Some((cursor_x, cursor_y)) = self.cursor_pos else {
            return;
        };
        let placements = place(&self.specs, (self.width, self.height));
        let dest_rect = vnc_dest_rect(self.width, self.height, &placements);
        if !point_in_rect(cursor_x, cursor_y, dest_rect) {
            return;
        }
        let Some(frame) = &self.last_vnc_frame else {
            return;
        };
        let Some((frame_x, frame_y)) =
            map_cursor_to_vnc_frame(cursor_x, cursor_y, dest_rect, frame.width, frame.height)
        else {
            return;
        };
        const WHEEL_UP: u8 = 0x08;
        const WHEEL_DOWN: u8 = 0x10;
        let wheel_bit = if up { WHEEL_UP } else { WHEEL_DOWN };
        self.send_vnc_pointer(frame_x, frame_y, self.cursor_buttons | wheel_bit);
        self.send_vnc_pointer(frame_x, frame_y, self.cursor_buttons);
    }

    fn send_vnc_pointer(&self, x: u16, y: u16, mask: u8) {
        if let Some(tx) = &self.vnc_cmd_tx {
            tracing::debug!("taulerbox: forwarding VNC pointer event x={x} y={y} mask={mask:#04x}");
            let _ = tx.send(VncCommand::Pointer { x, y, mask });
        }
    }

    /// Shows/hides the host-drawn cursor, but only on an actual transition —
    /// redundant `set_cursor_visible` calls on every event would be
    /// harmless but noisy. While hidden, the compartment's own
    /// Sway-rendered cursor (already visible in the composited VNC picture)
    /// is what the user sees over that region instead (issue #582).
    fn set_host_cursor_hidden(&mut self, hidden: bool) {
        if self.host_cursor_hidden == hidden {
            return;
        }
        if let Some(window) = &self.window {
            window.set_cursor_visible(!hidden);
        }
        self.host_cursor_hidden = hidden;
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let attrs = Window::default_attributes()
            .with_title("taulerbox")
            .with_inner_size(PhysicalSize::new(self.width, self.height))
            .with_resizable(true);
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                eprintln!("taulerbox: failed to create window: {e}");
                event_loop.exit();
                return;
            }
        };

        let context = match softbuffer::Context::new(window.clone()) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("taulerbox: softbuffer context failed: {e}");
                event_loop.exit();
                return;
            }
        };
        let mut surface = match softbuffer::Surface::new(&context, window.clone()) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("taulerbox: softbuffer surface failed: {e}");
                event_loop.exit();
                return;
            }
        };
        let (Some(w), Some(h)) = (NonZeroU32::new(self.width), NonZeroU32::new(self.height)) else {
            event_loop.exit();
            return;
        };
        if let Err(e) = surface.resize(w, h) {
            eprintln!("taulerbox: surface resize failed: {e}");
            event_loop.exit();
            return;
        }

        self.window = Some(window);
        self.surface = Some(surface);
        self._context = Some(context);
        self.present();
    }

    /// A new VNC frame arrived on `vnc_frame_rx` (issue #582's persistent
    /// connection): drain the channel, keeping only the newest frame (no
    /// point re-rendering intermediate ones the window thread hasn't gotten
    /// to yet), and if anything actually arrived, rebuild and present.
    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: ()) {
        let mut updated = false;
        if let Some(rx) = &self.vnc_frame_rx {
            for frame in rx.try_iter() {
                self.last_vnc_frame = Some(frame);
                updated = true;
            }
        }
        if !updated {
            return;
        }
        self.rebuild_framebuffer();
        self.present();
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => self.present(),
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_pos = Some((position.x, position.y));
                self.sync_pointer();
            }
            WindowEvent::CursorLeft { .. } => {
                self.cursor_pos = None;
                self.set_host_cursor_hidden(false);
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if let Some(bit) = mouse_button_bit(button) {
                    match state {
                        ElementState::Pressed => self.cursor_buttons |= bit,
                        ElementState::Released => self.cursor_buttons &= !bit,
                    }
                }
                self.sync_pointer();
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let vertical = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y as f64,
                    MouseScrollDelta::PixelDelta(pos) => pos.y,
                };
                if vertical > 0.0 {
                    self.send_vnc_wheel(true);
                } else if vertical < 0.0 {
                    self.send_vnc_wheel(false);
                }
            }
            WindowEvent::Resized(new_size) => {
                self.width = new_size.width.max(1);
                self.height = new_size.height.max(1);

                if let Some(surface) = self.surface.as_mut() {
                    if let (Some(w), Some(h)) = (NonZeroU32::new(self.width), NonZeroU32::new(self.height)) {
                        if let Err(e) = surface.resize(w, h) {
                            eprintln!("taulerbox: surface resize failed: {e}");
                        }
                    }
                }

                // Instant reaction: reposition panels and re-fit the stale
                // VNC frame into the new destination rect. No subprocess
                // calls, safe to do on every event fired mid-drag.
                self.rebuild_framebuffer();
                self.present();
                if let Some(window) = &self.window {
                    window.request_redraw();
                }

                // Debounced reaction: (re)schedule the slow compartment
                // resize for once resizing settles. A later Resized event
                // just overwrites this deadline, which is the debounce.
                let deadline = Instant::now() + RESIZE_DEBOUNCE;
                self.pending_resize = Some(deadline);
                event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(deadline) = self.pending_resize {
            if Instant::now() >= deadline {
                self.pending_resize = None;
                event_loop.set_control_flow(ControlFlow::Wait);
                self.handle_settled_resize();
            }
        }
    }
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    let args = Args::parse();

    anyhow::ensure!(
        args.layout.is_file(),
        "layout file not found: {}",
        args.layout.display()
    );
    anyhow::ensure!(
        args.home.is_dir(),
        "--home is not a directory: {}",
        args.home.display()
    );

    let source = tauler::layout_source::LayoutSource::Mdx(args.layout.clone());
    let loaded = tauler::layout_source::load(&source)?;

    tauler::render::init_global_ctx(loaded.config.fonts.clone());

    let ctx_json = serde_json::json!({
        "output": "taulerbox",
        "dpi": 1.0,
        "screen_width": INITIAL_WINDOW_WIDTH,
        "screen_height": INITIAL_WINDOW_HEIGHT,
    });
    let base_dir = args.layout.parent().unwrap();
    let evaluator =
        tauler::jsx::JsxEvaluator::new(&loaded.js_source, ctx_json.clone(), Some(base_dir))?;

    // The reconciler runtime: its own thread, its own QuickJS runtime, sweeping
    // forever for as long as this binding lives. `stream_values` starts empty —
    // taulerbox has no live Streams yet — and `pkg_ctx` is `None`, same as every
    // other call site that hasn't opted into `@gh/...` Package imports.
    let stream_values: tauler::units::SharedStreamValues = Arc::new(RwLock::new(HashMap::new()));
    let _reconciler = tauler::units::Reconciler::spawn(
        loaded.js_source.clone(),
        ctx_json,
        Some(base_dir.to_path_buf()),
        stream_values,
        evaluator.globals_handle(),
        None,
    );

    let eval_output = evaluator.eval(&HashMap::new())?;
    let specs = tauler::parse_root_node(&eval_output.layout).map_err(|e| anyhow::anyhow!(e))?;

    let panel_pixels = rasterize_panels(&specs);

    let framebuffer = build_framebuffer(&specs, &panel_pixels, INITIAL_WINDOW_WIDTH, INITIAL_WINDOW_HEIGHT, None);

    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Wait);

    // Best-effort and additive: the window must still open and show the
    // panel even if the compartment is slow to boot or unreachable, so
    // starting the persistent VNC worker never blocks or fails `main` — any
    // connection trouble is logged from the worker thread itself (issue
    // #582's persistent-connection step; see `spawn_vnc_worker`).
    let mut vnc_frame_rx = None;
    let mut vnc_cmd_tx = None;
    let mut vnc_thread = None;
    if let Some(addr) = &args.vnc {
        tracing::info!("taulerbox: --vnc given, starting a persistent connection to {addr}...");
        let (frame_tx, frame_rx) = std_mpsc::channel();
        let (cmd_tx, cmd_rx) = tokio_mpsc::unbounded_channel();
        let proxy = event_loop.create_proxy();
        vnc_thread = Some(spawn_vnc_worker(addr.clone(), frame_tx, cmd_rx, proxy));
        vnc_frame_rx = Some(frame_rx);
        vnc_cmd_tx = Some(cmd_tx);
    }

    let mut app = App {
        width: INITIAL_WINDOW_WIDTH,
        height: INITIAL_WINDOW_HEIGHT,
        specs,
        panel_pixels,
        vnc_addr: args.vnc,
        compartment_name: args.compartment_name,
        last_vnc_frame: None,
        vnc_frame_rx,
        vnc_cmd_tx,
        _vnc_thread: vnc_thread,
        cursor_pos: None,
        cursor_buttons: 0,
        host_cursor_hidden: false,
        vnc_output_name: None,
        pending_resize: None,
        framebuffer,
        window: None,
        surface: None,
        _context: None,
    };
    event_loop.run_app(&mut app)?;

    Ok(())
}
