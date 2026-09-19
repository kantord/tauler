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
use std::sync::Arc;

use clap::Parser;
use taulerbox::compose::place;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

/// taulerbox has no real monitor and does not resize: the window is exactly
/// this big, in physical pixels, matching the `fixtures/hello` panel plus
/// margin.
const WINDOW_WIDTH: u32 = 400;
const WINDOW_HEIGHT: u32 = 300;

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
    let evaluator = tauler::jsx::JsxEvaluator::new(&loaded.js_source, ctx_json, Some(base_dir))?;
    let eval_output = evaluator.eval(&HashMap::new())?;
    let specs = tauler::parse_root_node(&eval_output.layout).map_err(|e| anyhow::anyhow!(e))?;

    let framebuffer = render_framebuffer(&specs, WINDOW_WIDTH, WINDOW_HEIGHT);

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
