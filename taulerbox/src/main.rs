// taulerbox: the CLI over the library in lib.rs. Argument parsing and the
// window loop live here; layout placement math lives in `compose`, where it
// can be tested without a display server.
//
// This first version is a static single render: the layout is evaluated
// once (no live streams, no hot reload), each visible `Panel` is rasterized
// once, and the result is blitted into a fixed-size window that stays open
// until closed. See issue #582.

use std::collections::HashMap;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use clap::Parser;
use taulerbox::compose::place;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

/// taulerbox has no real monitor and does not resize: the window is exactly
/// this big, in physical pixels. Enlarged from the original 400x300 (sized
/// only for the `hello`/`compartment` fixtures' single 300x200 panel) to
/// leave room for a compartment's composited VNC view alongside the panel
/// (see `VNC_DEST_*` below and `--vnc` in [`Args`]).
const WINDOW_WIDTH: u32 = 900;
const WINDOW_HEIGHT: u32 = 700;

/// Hardcoded destination rect for the compartment's composited VNC frame,
/// to the right of the `hello`/`compartment` fixtures' left-anchored 300px
/// panel. Deliberately not derived from the layout file: declaring a
/// compartment's on-screen position from layout is future scope (issue
/// #582's task notes), this task only proves one frame can be pulled from a
/// live VNC session and blitted in next to the panel.
const VNC_DEST_X: u32 = 340;
const VNC_DEST_Y: u32 = 40;
const VNC_DEST_WIDTH: u32 = 520;
const VNC_DEST_HEIGHT: u32 = 620;

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
    /// connect to and pull exactly one frame from, composited into a
    /// hardcoded rect (`VNC_DEST_*`) alongside the panel. Optional: when
    /// absent, taulerbox behaves exactly as it does without this flag. Not a
    /// continuously-updating stream yet — see issue #582's task notes.
    #[arg(long)]
    vnc: Option<String>,
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

/// Renders every visible panel and blits it into a `width * height` `0RGB`
/// framebuffer, ready to hand straight to a softbuffer surface of the same
/// size.
fn render_framebuffer(
    specs: &[tauler::layout::SurfaceSpec],
    width: u32,
    height: u32,
) -> Vec<u32> {
    const BACKGROUND: u32 = 0x0000_0000;
    let mut framebuffer = vec![BACKGROUND; (width as usize) * (height as usize)];

    let panel_specs: HashMap<&str, &tauler::layout::SurfaceSpec> = specs
        .iter()
        .filter(|spec| spec.kind == tauler::layout::SurfaceKind::Panel)
        .map(|spec| (spec.id.as_str(), spec))
        .collect();

    let placements = place(specs, (width, height));

    for placement in &placements {
        if !placement.visible {
            continue;
        }
        let Some(spec) = panel_specs.get(placement.id.as_str()) else {
            continue;
        };
        if placement.width == 0 || placement.height == 0 {
            continue;
        }

        let rgba = tauler::render_frame_rgba(&spec.content, placement.width, placement.height, spec.dpr, None);

        for row in 0..placement.height {
            let dest_y = placement.y + row as i32;
            if dest_y < 0 || dest_y >= height as i32 {
                continue;
            }
            for col in 0..placement.width {
                let dest_x = placement.x + col as i32;
                if dest_x < 0 || dest_x >= width as i32 {
                    continue;
                }
                let src_idx = ((row * placement.width + col) as usize) * 4;
                let Some(px) = rgba.get(src_idx..src_idx + 4) else {
                    continue;
                };
                let (r, g, b, a) = (px[0], px[1], px[2], px[3]);
                let dest_idx = (dest_y as usize) * (width as usize) + dest_x as usize;
                if let Some(dest) = framebuffer.get_mut(dest_idx) {
                    *dest = composite_over(BACKGROUND, r, g, b, a);
                }
            }
        }
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

/// Waits for a `SetResolution` event (always sent first, per the RFB
/// handshake) and then accumulates `RawImage` rects into one full-screen
/// buffer, stopping once enough pixels have been received to cover the
/// whole screen once (or, failing that, once no new data has arrived for a
/// while but at least something was received). vnc-rs's connector already
/// requested one full, non-incremental framebuffer update as part of
/// connecting, so no explicit `X11Event::Refresh` is needed here.
async fn grab_one_frame(client: &vnc::VncClient) -> anyhow::Result<VncFrame> {
    let mut width: u32 = 0;
    let mut height: u32 = 0;
    let mut canvas: Vec<u8> = Vec::new();
    let mut covered_pixels: u64 = 0;
    let deadline = Instant::now() + Duration::from_secs(15);

    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining.min(Duration::from_secs(5)), client.recv_event()).await {
            Ok(Ok(vnc::VncEvent::SetResolution(screen))) => {
                width = screen.width as u32;
                height = screen.height as u32;
                canvas = vec![0u8; (width as usize) * (height as usize) * 4];
                covered_pixels = 0;
                tracing::info!("taulerbox: VNC framebuffer resolution is {width}x{height}");
            }
            Ok(Ok(vnc::VncEvent::RawImage(rect, data))) => {
                if width == 0 || height == 0 {
                    continue;
                }
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
                covered_pixels += (rect_w * rect_h) as u64;
                if covered_pixels >= (width as u64) * (height as u64) {
                    break;
                }
            }
            Ok(Ok(other)) => {
                tracing::debug!("taulerbox: ignoring VNC event while grabbing a frame: {other:?}");
            }
            Ok(Err(e)) => {
                anyhow::bail!("VNC event stream failed: {e}");
            }
            Err(_) => {
                if canvas.is_empty() {
                    anyhow::bail!("timed out waiting for any VNC framebuffer data");
                }
                tracing::warn!(
                    "taulerbox: VNC frame grab timed out before covering the whole screen; using the partial frame received so far"
                );
                break;
            }
        }
    }

    anyhow::ensure!(
        width > 0 && height > 0 && !canvas.is_empty(),
        "no VNC framebuffer data received"
    );
    let _ = client.close().await;
    Ok(VncFrame {
        width,
        height,
        rgba: canvas,
    })
}

/// Connects to `addr` (retrying while the compartment boots), grabs exactly
/// one frame, then tears the connection down. Runs its own single-threaded
/// async step inside an otherwise synchronous `main()`, per issue #582's
/// task notes: taulerbox does not become an async program for this.
fn fetch_one_vnc_frame(addr: &str) -> anyhow::Result<VncFrame> {
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        let client = connect_vnc_with_retry(addr).await?;
        grab_one_frame(&client).await
    })
}

/// Nearest-neighbor-scales `frame` into `dest_x, dest_y, dest_width,
/// dest_height` within `framebuffer` (a `width * height` `0RGB` buffer, same
/// convention as `render_framebuffer`'s output). No alpha blending: VNC
/// framebuffers are opaque, so the destination pixel is simply overwritten.
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
    if frame.width == 0 || frame.height == 0 || dest_width == 0 || dest_height == 0 {
        return;
    }

    for row in 0..dest_height {
        let dst_y = dest_y + row;
        if dst_y >= fb_height {
            continue;
        }
        let src_y = ((row as u64 * frame.height as u64) / dest_height as u64) as u32;
        let src_y = src_y.min(frame.height - 1);

        for col in 0..dest_width {
            let dst_x = dest_x + col;
            if dst_x >= fb_width {
                continue;
            }
            let src_x = ((col as u64 * frame.width as u64) / dest_width as u64) as u32;
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

/// Owns the one window this build ever opens, and the static framebuffer it
/// was told to show. `resumed` runs once, `RedrawRequested` re-presents the
/// same framebuffer, and `CloseRequested` ends the event loop.
struct App {
    width: u32,
    height: u32,
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
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let attrs = Window::default_attributes()
            .with_title("taulerbox")
            .with_inner_size(PhysicalSize::new(self.width, self.height))
            .with_resizable(false);
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

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => self.present(),
            _ => {}
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
        "screen_width": WINDOW_WIDTH,
        "screen_height": WINDOW_HEIGHT,
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

    let mut framebuffer = render_framebuffer(&specs, WINDOW_WIDTH, WINDOW_HEIGHT);

    // Best-effort and additive: the window must still open and show the
    // panel even if the compartment is slow to boot or unreachable, so a
    // VNC failure here is logged and swallowed rather than propagated.
    if let Some(addr) = &args.vnc {
        tracing::info!("taulerbox: --vnc given, connecting to compartment at {addr}...");
        match fetch_one_vnc_frame(addr) {
            Ok(frame) => {
                tracing::info!(
                    "taulerbox: got one {}x{} VNC frame from {addr}, compositing into the window",
                    frame.width,
                    frame.height
                );
                blit_vnc_frame(
                    &mut framebuffer,
                    WINDOW_WIDTH,
                    WINDOW_HEIGHT,
                    &frame,
                    VNC_DEST_X,
                    VNC_DEST_Y,
                    VNC_DEST_WIDTH,
                    VNC_DEST_HEIGHT,
                );
            }
            Err(e) => {
                eprintln!(
                    "taulerbox: VNC frame from {addr} unavailable, showing the panel without it: {e}"
                );
            }
        }
    }

    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Wait);

    let mut app = App {
        width: WINDOW_WIDTH,
        height: WINDOW_HEIGHT,
        framebuffer,
        window: None,
        surface: None,
        _context: None,
    };
    event_loop.run_app(&mut app)?;

    Ok(())
}
