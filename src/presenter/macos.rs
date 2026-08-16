//! macOS presenter. AppKit may only be driven from the main thread, so the
//! event loop runs there and `App` lives on a worker thread, connected by the
//! same `SurfaceCommand` / `PresenterEvent` channels the other backends use.

use std::collections::HashMap;
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use tauler::data::data_loop::{DataLoop, StreamItem};
use tauler::layout::{PanelAnchor, SurfaceSpec};
use tauler::presentation::{
    PointerEvent, PointerPhase, PresenterEvent, PresenterEvents, SurfaceCommand, SurfaceFrame,
};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::{MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId, WindowLevel};

use crate::app::{App, MacInit, ModuleEventTxs, SharedWatcher, TickReceivers};
use crate::presenter::drain_commands;

/// How often the main thread wakes to drain `SurfaceCommand`s.
const COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(8);

const FALLBACK_SCREEN: (u32, u32) = (1920, 1080);

/// Monitor geometry in logical pixels, the unit `SurfaceSpec` uses.
#[derive(Debug, Clone, Copy)]
struct MonitorRect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl MonitorRect {
    fn fallback() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: FALLBACK_SCREEN.0 as f64,
            height: FALLBACK_SCREEN.1 as f64,
        }
    }
}

pub(crate) struct MacBoot {
    pub(crate) data_loop: DataLoop,
    pub(crate) handle: tauler::data::data_loop::DataLoopHandle,
    pub(crate) rx: TickReceivers,
    pub(crate) item_tx: mpsc::Sender<((String, Option<String>), String)>,
    pub(crate) layout_jsx_path: std::path::PathBuf,
    pub(crate) config_yaml_path: std::path::PathBuf,
    pub(crate) module_event_txs: ModuleEventTxs,
    pub(crate) stop: Arc<AtomicBool>,
    pub(crate) last_tick: Arc<AtomicU64>,
    pub(crate) watcher: SharedWatcher,
}

struct MacPanel {
    window: Arc<Window>,
    // Unused, but softbuffer ties the surface's lifetime to its context.
    _context: softbuffer::Context<Arc<Window>>,
    surface: softbuffer::Surface<Arc<Window>, Arc<Window>>,
    /// Retained so an AppKit-initiated redraw can re-present it.
    last_frame: Option<SurfaceFrame>,
    cursor: (f64, f64),
    dpr: f32,
}

/// Convert rasterized BGRX bytes to the `0RGB` words softbuffer presents.
///
/// Mismatched lengths are clipped, not panicked: a frame can arrive either side
/// of a resize.
fn write_0rgb(dst: &mut [u32], bgrx: &[u8]) {
    for (word, px) in dst.iter_mut().zip(bgrx.chunks_exact(4)) {
        *word = (u32::from(px[2]) << 16) | (u32::from(px[1]) << 8) | u32::from(px[0]);
    }
}

/// Top-left corner of a panel window in logical screen coordinates.
fn window_origin(spec: &SurfaceSpec, mon: MonitorRect) -> (f64, f64) {
    match spec.anchor {
        Some(PanelAnchor::Left) | Some(PanelAnchor::Top) => (mon.x, mon.y),
        Some(PanelAnchor::Right) => (mon.x + mon.width - spec.width as f64, mon.y),
        Some(PanelAnchor::Bottom) => (mon.x, mon.y + mon.height - spec.height as f64),
        None => (mon.x + spec.x as f64, mon.y + spec.y as f64),
    }
}

/// Monitor geometry is only reachable from an `ActiveEventLoop`, so the worker
/// cannot start until the first `resumed` callback.
struct PendingBoot {
    boot: MacBoot,
    command_tx: mpsc::Sender<SurfaceCommand>,
    event_rx: mpsc::Receiver<PresenterEvent>,
}

struct MacPresenter {
    panels: HashMap<String, MacPanel>,
    command_rx: mpsc::Receiver<SurfaceCommand>,
    event_tx: PresenterEvents,
    stop: Arc<AtomicBool>,
    monitor: MonitorRect,
    pending: Option<PendingBoot>,
    worker: Option<thread::JoinHandle<()>>,
}

impl MacPresenter {
    fn create(&mut self, elwt: &ActiveEventLoop, spec: &SurfaceSpec, frame: &SurfaceFrame) {
        let (x, y) = window_origin(spec, self.monitor);
        let attrs = Window::default_attributes()
            .with_title(format!("tauler:{}", spec.id))
            .with_decorations(false)
            .with_resizable(false)
            .with_transparent(true)
            .with_window_level(if spec.above {
                WindowLevel::AlwaysOnTop
            } else {
                WindowLevel::Normal
            })
            .with_inner_size(LogicalSize::new(spec.width as f64, spec.height as f64))
            .with_position(LogicalPosition::new(x, y));

        let window = match elwt.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                tracing::error!(panel = %spec.id, error = %e, "failed to create window");
                return;
            }
        };
        let context = match softbuffer::Context::new(window.clone()) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(panel = %spec.id, error = %e, "softbuffer context failed");
                return;
            }
        };
        let surface = match softbuffer::Surface::new(&context, window.clone()) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(panel = %spec.id, error = %e, "softbuffer surface failed");
                return;
            }
        };

        let size = window.inner_size();
        tracing::info!(
            panel = %spec.id,
            x, y,
            logical_w = spec.width,
            logical_h = spec.height,
            phys_w = size.width,
            phys_h = size.height,
            "created window"
        );

        let mut panel = MacPanel {
            window,
            _context: context,
            surface,
            last_frame: None,
            cursor: (0.0, 0.0),
            dpr: spec.dpr,
        };
        present(&mut panel, frame, &spec.id);
        self.panels.insert(spec.id.clone(), panel);
    }

    fn apply(&mut self, elwt: &ActiveEventLoop, cmd: SurfaceCommand) {
        match cmd {
            SurfaceCommand::Create { spec, frame } => self.create(elwt, &spec, &frame),
            SurfaceCommand::Move(spec) => {
                let origin = window_origin(&spec, self.monitor);
                if let Some(panel) = self.panels.get_mut(&spec.id) {
                    panel
                        .window
                        .set_outer_position(LogicalPosition::new(origin.0, origin.1));
                }
            }
            SurfaceCommand::Resize { spec, frame } => {
                if let Some(panel) = self.panels.get_mut(&spec.id) {
                    let _ = panel.window.request_inner_size(LogicalSize::new(
                        spec.width as f64,
                        spec.height as f64,
                    ));
                    panel.dpr = spec.dpr;
                    present(panel, &frame, &spec.id);
                }
            }
            SurfaceCommand::Delete { id } => {
                self.panels.remove(&id);
            }
            SurfaceCommand::UpdatePicture { id, frame } => {
                if let Some(panel) = self.panels.get_mut(&id) {
                    present(panel, &frame, &id);
                }
            }
            // macOS offers no equivalent of painting the desktop background.
            SurfaceCommand::PaintWallpaper { spec, .. } => {
                tracing::warn!(id = %spec.id, "<wallpaper> is not supported on macOS; ignoring");
            }
            SurfaceCommand::Shutdown => {
                unreachable!("Shutdown is intercepted by drain_commands before apply is called")
            }
        }
    }

    fn panel_id_for(&self, window_id: WindowId) -> Option<String> {
        self.panels
            .iter()
            .find(|(_, p)| p.window.id() == window_id)
            .map(|(id, _)| id.clone())
    }
}

fn present(panel: &mut MacPanel, frame: &SurfaceFrame, id: &str) {
    let size = panel.window.inner_size();
    let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
        return;
    };
    if let Err(e) = panel.surface.resize(w, h) {
        tracing::error!(panel = %id, error = %e, "surface resize failed");
        return;
    }
    match panel.surface.buffer_mut() {
        Ok(mut buffer) => {
            write_0rgb(&mut buffer, &frame.pixels[..]);
            let sample = buffer.first().copied().unwrap_or(0);
            if let Err(e) = buffer.present() {
                tracing::error!(panel = %id, error = %e, "buffer present failed");
            } else {
                tracing::debug!(
                    panel = %id,
                    surface = format!("{}x{}", w, h),
                    frame = format!("{}x{}", frame.width, frame.height),
                    top_left = format!("#{sample:06X}"),
                    "presented"
                );
            }
        }
        Err(e) => tracing::error!(panel = %id, error = %e, "buffer_mut failed"),
    }
    panel.last_frame = Some(frame.clone());
}

impl ApplicationHandler for MacPresenter {
    fn resumed(&mut self, elwt: &ActiveEventLoop) {
        let Some(PendingBoot {
            boot,
            command_tx,
            event_rx,
        }) = self.pending.take()
        else {
            return;
        };

        let monitor = elwt
            .primary_monitor()
            .or_else(|| elwt.available_monitors().next());
        let dpr = monitor.as_ref().map(|m| m.scale_factor()).unwrap_or(1.0);
        let output_name = monitor
            .as_ref()
            .and_then(|m| m.name())
            .unwrap_or_else(|| "macos".to_string());
        self.monitor = monitor
            .as_ref()
            .map(|m| {
                let (pos, size) = (m.position(), m.size());
                MonitorRect {
                    x: pos.x as f64 / dpr,
                    y: pos.y as f64 / dpr,
                    width: size.width as f64 / dpr,
                    height: size.height as f64 / dpr,
                }
            })
            .unwrap_or_else(MonitorRect::fallback);

        tracing::info!(
            output = %output_name,
            dpr,
            width = self.monitor.width,
            height = self.monitor.height,
            "display backend: macOS"
        );

        let mac = MacInit {
            command_tx,
            event_rx,
            screen_width_logical: self.monitor.width.round() as u32,
            screen_height_logical: self.monitor.height.round() as u32,
            dpr: dpr as f32,
            output_name,
        };
        self.worker = Some(spawn_worker(boot, mac));
    }

    fn about_to_wait(&mut self, elwt: &ActiveEventLoop) {
        // `drain_commands` blocks briefly, which doubles as the idle wait.
        let mut pending = Vec::new();
        let shutdown = drain_commands(&self.command_rx, |cmd| pending.push(cmd));
        for cmd in pending {
            self.apply(elwt, cmd);
        }
        if shutdown || self.stop.load(Ordering::Relaxed) {
            elwt.exit();
            return;
        }
        elwt.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + COMMAND_POLL_INTERVAL,
        ));
    }

    fn window_event(&mut self, elwt: &ActiveEventLoop, window_id: WindowId, event: WindowEvent) {
        let Some(id) = self.panel_id_for(window_id) else {
            return;
        };
        match event {
            WindowEvent::RedrawRequested => {
                if let Some(panel) = self.panels.get_mut(&id) {
                    if let Some(frame) = panel.last_frame.clone() {
                        present(panel, &frame, &id);
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(panel) = self.panels.get_mut(&id) {
                    panel.cursor = (position.x, position.y);
                }
            }
            // No motion is reported while a button is held, so a control here is
            // click-to-set. The release still has to be sent: a press that started a
            // capture nothing ever ends would pin a stale handler (`docs/adr/0020`).
            WindowEvent::MouseInput { state, button, .. } => {
                if let Some(panel) = self.panels.get(&id) {
                    let size = panel.window.inner_size();
                    let pressed = state.is_pressed();
                    let _ = self.event_tx.send(PresenterEvent::Pointer(PointerEvent {
                        panel_id: id.clone(),
                        x: panel.cursor.0 as f32,
                        y: panel.cursor.1 as f32,
                        phys_width: size.width,
                        phys_height: size.height,
                        dpr: panel.dpr,
                        phase: if pressed {
                            PointerPhase::Press
                        } else {
                            PointerPhase::Release
                        },
                        buttons: if pressed { dom_button(button) } else { 0 },
                    }));
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                let _ = self.event_tx.send(PresenterEvent::NeedsRender);
            }
            WindowEvent::CloseRequested => elwt.exit(),
            _ => {}
        }
    }
}

/// Which button this is, as a DOM `buttons` bit: 1 primary, 2 secondary, 4 auxiliary.
///
/// The same numbering the X11 presenter produces, so a handler sees one vocabulary
/// whatever it is running on (`docs/adr/0020`).
fn dom_button(button: MouseButton) -> u16 {
    match button {
        MouseButton::Left => 1,
        MouseButton::Right => 2,
        MouseButton::Middle => 4,
        _ => 0,
    }
}

/// `App` is built inside the closure because its QuickJS runtime is not `Send`.
/// Only the channel ends cross the thread boundary.
fn spawn_worker(boot: MacBoot, mac: MacInit) -> thread::JoinHandle<()> {
    let MacBoot {
        mut data_loop,
        handle,
        rx,
        item_tx,
        layout_jsx_path,
        config_yaml_path,
        module_event_txs,
        stop,
        last_tick,
        watcher,
    } = boot;

    thread::spawn(move || {
        let mut app = App::new_macos(
            mac,
            handle,
            rx,
            layout_jsx_path,
            config_yaml_path,
            module_event_txs,
            Arc::clone(&stop),
            last_tick,
            watcher,
        );
        data_loop.run(
            stop,
            move |item: StreamItem| {
                let _ = item_tx.send((item.key, item.line));
            },
            move || app.tick(),
        );
    })
}

pub(crate) fn run(boot: MacBoot) -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);

    let (command_tx, command_rx) = mpsc::channel();
    let (event_tx, event_rx) = mpsc::channel();
    // AppKit owns this thread, so the loop is off on a worker — which makes the
    // ping the only thing that gets a pointer event looked at before the
    // supervision timer comes round.
    let event_tx = PresenterEvents::new(event_tx, boot.data_loop.notifier());
    let stop = Arc::clone(&boot.stop);

    let mut presenter = MacPresenter {
        panels: HashMap::new(),
        command_rx,
        event_tx,
        stop: Arc::clone(&stop),
        monitor: MonitorRect::fallback(),
        pending: Some(PendingBoot {
            boot,
            command_tx,
            event_rx,
        }),
        worker: None,
    };
    event_loop.run_app(&mut presenter)?;

    stop.store(true, Ordering::Relaxed);
    if let Some(worker) = presenter.worker.take() {
        let _ = worker.join();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{window_origin, write_0rgb, MonitorRect};
    use tauler::layout::{PanelAnchor, SurfaceSpec};

    #[test]
    fn packs_bgrx_into_0rgb_and_drops_the_pad_byte() {
        let mut dst = [0u32; 1];
        write_0rgb(&mut dst, &[0xAA, 0xBB, 0xCC, 0xDD]);
        assert_eq!(dst, [0x00CC_BBAA]);
    }

    #[test]
    fn writes_pixels_in_row_major_order() {
        let mut dst = [0u32; 2];
        write_0rgb(&mut dst, &[1, 2, 3, 255, 4, 5, 6, 255]);
        assert_eq!(dst, [0x0003_0201, 0x0006_0504]);
    }

    #[test]
    fn leaves_trailing_destination_pixels_untouched_when_the_frame_is_short() {
        let mut dst = [0xDEAD_BEEF; 3];
        write_0rgb(&mut dst, &[1, 2, 3, 255]);
        assert_eq!(dst, [0x0003_0201, 0xDEAD_BEEF, 0xDEAD_BEEF]);
    }

    #[test]
    fn clips_a_frame_that_is_larger_than_the_destination() {
        let mut dst = [0u32; 1];
        write_0rgb(&mut dst, &[1, 2, 3, 255, 4, 5, 6, 255]);
        assert_eq!(dst, [0x0003_0201]);
    }

    fn spec(anchor: Option<PanelAnchor>, x: i32, y: i32) -> SurfaceSpec {
        SurfaceSpec {
            kind: tauler::SurfaceKind::Panel,
            id: "p".into(),
            anchor,
            width: 200,
            height: 40,
            x,
            y,
            outer_gap: 0,
            output: None,
            above: false,
            content: serde_json::Value::Null,
            dpr: 2.0,
        }
    }

    const MON: MonitorRect = MonitorRect {
        x: 100.0,
        y: 50.0,
        width: 1000.0,
        height: 800.0,
    };

    #[test]
    fn anchors_top_and_left_panels_to_the_monitor_origin() {
        assert_eq!(
            window_origin(&spec(Some(PanelAnchor::Top), 7, 9), MON),
            (100.0, 50.0)
        );
        assert_eq!(
            window_origin(&spec(Some(PanelAnchor::Left), 7, 9), MON),
            (100.0, 50.0)
        );
    }

    #[test]
    fn anchors_right_and_bottom_panels_to_the_far_monitor_edge() {
        assert_eq!(
            window_origin(&spec(Some(PanelAnchor::Right), 0, 0), MON),
            (900.0, 50.0)
        );
        assert_eq!(
            window_origin(&spec(Some(PanelAnchor::Bottom), 0, 0), MON),
            (100.0, 810.0)
        );
    }

    #[test]
    fn offsets_unanchored_panels_from_the_monitor_origin() {
        assert_eq!(window_origin(&spec(None, 7, 9), MON), (107.0, 59.0));
    }
}
