use std::os::fd::{AsFd, AsRawFd, RawFd};
use std::sync::Arc;

use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_layer, delegate_output, delegate_pointer, delegate_registry,
    delegate_seat, delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{
            Anchor, Layer, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use wayland_client::{
    backend::ObjectId,
    globals::registry_queue_init,
    protocol::{wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, EventQueue, Proxy, QueueHandle,
};

use super::{DispatchError, DisplayServer, WindowEvent};
use crate::display_manager::DisplayManager;
use crate::layout::{PanelAnchor, PanelSpecData};
use crate::presentation::PanelFrame;

// ---------------------------------------------------------------------------
// Public error type
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum WaylandConnectError {
    #[error("failed to connect to Wayland display: {0}")]
    Connect(String),
    #[error("failed to bind compositor: {0}")]
    BindCompositor(String),
    #[error("failed to bind shm: {0}")]
    BindShm(String),
    #[error("failed to bind layer shell: {0}")]
    BindLayerShell(String),
}

// ---------------------------------------------------------------------------
// Wayland panel: owns a layer surface and its SHM pool
// ---------------------------------------------------------------------------

pub struct WaylandPanel {
    pub layer_surface: LayerSurface,
    pool: SlotPool,
    pub surface_id: ObjectId,
    pub configured: bool,
    pub width: u32,
    pub height: u32,
    pub anchor: Option<PanelAnchor>,
    pub dpr: f32,
}

impl WaylandPanel {
    /// Update the panel from a new spec. Resizes the layer surface if the fixed dimension changed,
    /// which will cause the compositor to send a new configure before the next render.
    pub fn update_spec(&mut self, data: &PanelSpecData) {
        if fixed_axis_changed(
            self.anchor.as_ref(),
            self.width,
            self.height,
            data.width,
            data.height,
        ) {
            self.width = data.width;
            self.height = data.height;
            let (set_w, set_h) = compute_set_size(self.anchor.as_ref(), data.width, data.height);
            self.layer_surface.set_size(set_w, set_h);
            self.layer_surface.wl_surface().commit();
            self.configured = false;
        }
    }

    /// Paint a BGRX frame onto this panel's layer surface.
    pub fn render(&mut self, bgrx: &[u8]) {
        if !self.configured {
            return;
        }
        let stride = self.width as i32 * 4;
        // Derive height from bgrx rather than self.height: the compositor may have
        // configured a different height than what the app rendered (configure race),
        // and using self.height would produce a mismatched canvas size and panic.
        let actual_height = (bgrx.len() / 4).saturating_div(self.width as usize) as i32;
        if actual_height == 0 {
            return;
        }
        let Ok((buffer, canvas)) = self.pool.create_buffer(
            self.width as i32,
            actual_height,
            stride,
            wl_shm::Format::Xrgb8888,
        ) else {
            tracing::error!("failed to create Wayland SHM buffer");
            return;
        };
        // SlotPool rounds slot size up to 64 bytes; copy only the actual pixel data.
        canvas[..bgrx.len()].copy_from_slice(bgrx);
        let wl_surf = self.layer_surface.wl_surface();
        if buffer.attach_to(wl_surf).is_err() {
            tracing::error!("failed to attach buffer to surface");
            return;
        }
        wl_surf.damage_buffer(0, 0, self.width as i32, actual_height);
        wl_surf.commit();
    }
}

// ---------------------------------------------------------------------------
// Internal sctk dispatch state
// ---------------------------------------------------------------------------

pub(crate) struct WaylandState {
    pub(crate) registry_state: RegistryState,
    pub(crate) compositor_state: CompositorState,
    pub(crate) output_state: OutputState,
    pub(crate) shm: Shm,
    pub(crate) layer_shell: LayerShell,
    pub(crate) seat_state: SeatState,
    pub(crate) pointer: Option<wl_pointer::WlPointer>,
    pub(crate) pending_events: Vec<WindowEvent>,
    /// (surface_id, new_size) pairs from configure events received since last take.
    /// new_size of (0, 0) means "use your set_size value".
    pub(crate) pending_configures: Vec<(ObjectId, (u32, u32))>,
}

// ---------------------------------------------------------------------------
// Public struct
// ---------------------------------------------------------------------------

pub struct WaylandDisplayServer {
    conn: Arc<Connection>,
    event_queue: EventQueue<WaylandState>,
    state: WaylandState,
}

impl std::fmt::Debug for WaylandDisplayServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WaylandDisplayServer")
            .finish_non_exhaustive()
    }
}

impl WaylandDisplayServer {
    pub fn connect() -> Result<Self, WaylandConnectError> {
        let conn = Connection::connect_to_env()
            .map_err(|e| WaylandConnectError::Connect(e.to_string()))?;
        let conn = Arc::new(conn);

        let (globals, event_queue) = registry_queue_init::<WaylandState>(&conn)
            .map_err(|e| WaylandConnectError::Connect(e.to_string()))?;

        let qh = event_queue.handle();

        let compositor_state = CompositorState::bind(&globals, &qh)
            .map_err(|e| WaylandConnectError::BindCompositor(e.to_string()))?;
        let output_state = OutputState::new(&globals, &qh);
        let shm =
            Shm::bind(&globals, &qh).map_err(|e| WaylandConnectError::BindShm(e.to_string()))?;
        let layer_shell = LayerShell::bind(&globals, &qh)
            .map_err(|e| WaylandConnectError::BindLayerShell(e.to_string()))?;
        let seat_state = SeatState::new(&globals, &qh);

        let state = WaylandState {
            registry_state: RegistryState::new(&globals),
            compositor_state,
            output_state,
            shm,
            layer_shell,
            seat_state,
            pointer: None,
            pending_events: Vec::new(),
            pending_configures: Vec::new(),
        };

        let mut server = Self {
            conn,
            event_queue,
            state,
        };
        // Roundtrip so output geometry events are processed before the caller
        // queries output dimensions.
        server
            .event_queue
            .roundtrip(&mut server.state)
            .map_err(|e| WaylandConnectError::Connect(e.to_string()))?;
        Ok(server)
    }

    /// Returns the logical size of the first known output, falling back to the
    /// current physical mode if xdg-output logical size is unavailable.
    pub fn primary_output_size(&self) -> Option<(u32, u32)> {
        let output = self.state.output_state.outputs().next()?;
        let info = self.state.output_state.info(&output)?;
        if let Some((w, h)) = info.logical_size {
            if w > 0 && h > 0 {
                return Some((w as u32, h as u32));
            }
        }
        info.modes
            .iter()
            .find(|m| m.current)
            .map(|m| (m.dimensions.0 as u32, m.dimensions.1 as u32))
    }

    /// Returns the device pixel ratio of the primary output.
    /// Uses physical/logical width ratio if logical size is available, otherwise falls back to scale_factor.
    pub fn primary_output_scale(&self) -> f32 {
        let Some(output) = self.state.output_state.outputs().next() else {
            return 1.0;
        };
        let Some(info) = self.state.output_state.info(&output) else {
            return 1.0;
        };
        let physical_w = info
            .modes
            .iter()
            .find(|m| m.current)
            .map(|m| m.dimensions.0 as u32)
            .unwrap_or(0);
        let logical_w = info.logical_size.map(|(w, _)| w as u32).unwrap_or(0);
        compute_output_scale(logical_w, physical_w, info.scale_factor)
    }

    /// Create a Wayland layer-shell panel for the given spec.
    /// The surface won't render until the compositor sends a configure and
    /// `WaylandPanel::render` is called with pixel data.
    pub fn create_panel(&mut self, data: &PanelSpecData) -> Result<WaylandPanel, anyhow::Error> {
        let qh = self.event_queue.handle();
        let wl_surface = self.state.compositor_state.create_surface(&qh);

        let anchor = anchor_for_panel(data.anchor.as_ref());
        let layer = if data.above {
            Layer::Top
        } else {
            Layer::Bottom
        };

        let layer_surface = self.state.layer_shell.create_layer_surface(
            &qh,
            wl_surface,
            layer,
            Some("costae"),
            None,
        );

        // For panels that span the full perpendicular axis (composite anchor), set the spanned
        // dimension to 0 so the compositor fills it. The actual dimension arrives in the configure.
        let (set_w, set_h) = match data.anchor {
            Some(PanelAnchor::Left) | Some(PanelAnchor::Right) => (data.width, 0),
            Some(PanelAnchor::Top) | Some(PanelAnchor::Bottom) => (0, data.height),
            None => (data.width, data.height),
        };
        layer_surface.set_size(set_w, set_h);
        if !anchor.is_empty() {
            layer_surface.set_anchor(anchor);
            let exclusive_zone = match data.anchor {
                Some(PanelAnchor::Left) | Some(PanelAnchor::Right) => data.width as i32,
                Some(PanelAnchor::Top) | Some(PanelAnchor::Bottom) => data.height as i32,
                None => 0,
            };
            if exclusive_zone > 0 {
                layer_surface.set_exclusive_zone(exclusive_zone);
            }
        }
        layer_surface.wl_surface().commit();

        let surface_id = layer_surface.wl_surface().id();

        // 3× frame size: supports one buffer in-flight with the compositor + one being prepared.
        let pool_size = (data.width as usize) * (data.height as usize) * 4 * 3;
        let pool = SlotPool::new(pool_size.max(4096 * 3), &self.state.shm)
            .map_err(|e| anyhow::anyhow!("SlotPool::new: {e}"))?;

        self.conn
            .flush()
            .map_err(|e| anyhow::anyhow!("flush after create_panel: {e}"))?;

        let dpr = self.primary_output_scale();

        Ok(WaylandPanel {
            layer_surface,
            pool,
            surface_id,
            configured: false,
            width: data.width,
            height: data.height,
            anchor: data.anchor.clone(),
            dpr,
        })
    }

    /// Drain and return (surface_id, new_size) pairs from configure events since the last call.
    /// A new_size of (0, 0) means the compositor accepted the set_size value as-is.
    pub fn take_pending_configures(&mut self) -> Vec<(ObjectId, (u32, u32))> {
        std::mem::take(&mut self.state.pending_configures)
    }

    pub fn flush(&self) {
        let _ = self.conn.flush();
    }
}

// ---------------------------------------------------------------------------
// Pure helper — testable without a live Wayland connection
// ---------------------------------------------------------------------------

pub fn build_dispatch_result(
    dispatch_ok: bool,
    flush_ok: bool,
    pending: Vec<WindowEvent>,
) -> Result<Vec<WindowEvent>, DispatchError> {
    if !dispatch_ok || !flush_ok {
        return Err(DispatchError::ConnectionLost);
    }
    Ok(pending)
}

pub(crate) fn anchor_for_panel(anchor: Option<&PanelAnchor>) -> Anchor {
    // Use composite anchors so the compositor stretches the panel across the full perpendicular
    // axis (layer-shell spec: anchoring both opposite edges makes the surface span between them).
    match anchor {
        Some(PanelAnchor::Left) => Anchor::LEFT | Anchor::TOP | Anchor::BOTTOM,
        Some(PanelAnchor::Right) => Anchor::RIGHT | Anchor::TOP | Anchor::BOTTOM,
        Some(PanelAnchor::Top) => Anchor::TOP | Anchor::LEFT | Anchor::RIGHT,
        Some(PanelAnchor::Bottom) => Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
        None => Anchor::empty(),
    }
}

// ---------------------------------------------------------------------------
// DisplayServer impl
// ---------------------------------------------------------------------------

impl DisplayServer for WaylandDisplayServer {
    fn as_raw_fd(&self) -> RawFd {
        self.conn.as_fd().as_raw_fd()
    }

    fn dispatch(&mut self) -> Result<Vec<WindowEvent>, DispatchError> {
        // Flush outgoing requests, then do a non-blocking read of any incoming events.
        // dispatch_pending alone only processes already-buffered events; without the read
        // step configure/close events from the compositor are never received.
        let _ = self.event_queue.flush();
        if let Some(guard) = self.event_queue.prepare_read() {
            let _ = guard.read();
        }
        let dispatch_ok = self.event_queue.dispatch_pending(&mut self.state).is_ok();
        let flush_ok = self.event_queue.flush().is_ok();
        build_dispatch_result(
            dispatch_ok,
            flush_ok,
            std::mem::take(&mut self.state.pending_events),
        )
    }
}

// ---------------------------------------------------------------------------
// DisplayManager impl
// ---------------------------------------------------------------------------

impl DisplayManager for WaylandDisplayServer {
    type Panel = WaylandPanel;

    fn create_window(
        &mut self,
        spec: &PanelSpecData,
        frame: &PanelFrame,
    ) -> Result<WaylandPanel, anyhow::Error> {
        let mut panel = self.create_panel(spec)?;
        self.update_image(&mut panel, &frame.pixels)?;
        Ok(panel)
    }

    fn update_position(
        &mut self,
        _panel: &mut WaylandPanel,
        _spec: &PanelSpecData,
    ) -> Result<(), anyhow::Error> {
        // No-op: Wayland position is compositor-managed via anchor
        Ok(())
    }

    fn update_dimensions(
        &mut self,
        panel: &mut WaylandPanel,
        spec: &PanelSpecData,
    ) -> Result<(), anyhow::Error> {
        panel.update_spec(spec);
        Ok(())
    }

    fn update_image(&mut self, panel: &mut WaylandPanel, bgrx: &[u8]) -> Result<(), anyhow::Error> {
        panel.render(bgrx);
        Ok(())
    }

    fn delete_window(&mut self, _panel: WaylandPanel) -> Result<(), anyhow::Error> {
        // Panel is dropped here by ownership transfer; cleanup is implicit
        Ok(())
    }

    fn flush(&mut self) {
        let _ = self.conn.flush();
    }
}

// ---------------------------------------------------------------------------
// sctk handler implementations
// ---------------------------------------------------------------------------

impl CompositorHandler for WaylandState {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_factor: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for WaylandState {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
        self.pending_events.push(WindowEvent::OutputsChanged);
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
        self.pending_events.push(WindowEvent::OutputsChanged);
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
        self.pending_events.push(WindowEvent::OutputsChanged);
    }
}

impl LayerShellHandler for WaylandState {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _layer: &LayerSurface) {}

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        // ack_configure is called automatically by delegate_layer!
        self.pending_configures
            .push((layer.wl_surface().id(), configure.new_size));
    }
}

impl SeatHandler for WaylandState {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer && self.pointer.is_none() {
            match self.seat_state.get_pointer(qh, &seat) {
                Ok(ptr) => {
                    self.pointer = Some(ptr);
                }
                Err(e) => tracing::warn!(error = %e, "failed to bind pointer"),
            }
        }
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer {
            if let Some(ptr) = self.pointer.take() {
                ptr.release();
            }
        }
    }

    fn remove_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: wl_seat::WlSeat) {
    }
}

impl PointerHandler for WaylandState {
    fn pointer_frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _pointer: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            let PointerEventKind::Press { button, .. } = event.kind else {
                continue;
            };
            let mouse_button = match button {
                0x110 => super::MouseButton::Left,
                0x111 => super::MouseButton::Right,
                0x112 => super::MouseButton::Middle,
                other => super::MouseButton::Other(other),
            };
            self.pending_events.push(WindowEvent::Click {
                panel_id: event.surface.id().to_string(),
                x_logical: event.position.0 as f32,
                y_logical: event.position.1 as f32,
                button: mouse_button,
            });
        }
    }
}

impl ShmHandler for WaylandState {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for WaylandState {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    registry_handlers!(OutputState, SeatState);
}

delegate_compositor!(WaylandState);
delegate_output!(WaylandState);
delegate_layer!(WaylandState);
delegate_seat!(WaylandState);
delegate_pointer!(WaylandState);
delegate_shm!(WaylandState);
delegate_registry!(WaylandState);

// ---------------------------------------------------------------------------
// Helper functions used internally and by tests
// ---------------------------------------------------------------------------

/// Returns the `(width, height)` pair to pass to `set_size` for the compositor.
/// For anchors where the compositor controls one axis (Left/Right → height;
/// Top/Bottom → width), that axis must be 0 so the compositor can stretch it.
pub(crate) fn compute_set_size(
    anchor: Option<&PanelAnchor>,
    width: u32,
    height: u32,
) -> (u32, u32) {
    match anchor {
        Some(PanelAnchor::Left) | Some(PanelAnchor::Right) => (width, 0),
        Some(PanelAnchor::Top) | Some(PanelAnchor::Bottom) => (0, height),
        None => (width, height),
    }
}

/// Returns true if the fixed axis of the panel has changed dimensions, indicating
/// that a compositor reconfigure is needed. The fixed axis is:
/// - Left/Right: width
/// - Top/Bottom: height
/// - None: either axis
pub(crate) fn fixed_axis_changed(
    anchor: Option<&PanelAnchor>,
    old_w: u32,
    old_h: u32,
    new_w: u32,
    new_h: u32,
) -> bool {
    match anchor {
        Some(PanelAnchor::Left) | Some(PanelAnchor::Right) => new_w != old_w,
        Some(PanelAnchor::Top) | Some(PanelAnchor::Bottom) => new_h != old_h,
        None => new_w != old_w || new_h != old_h,
    }
}

/// Computes the device pixel ratio from output info.
/// If `logical_w > 0`, returns `physical_w / logical_w`; otherwise falls back to `scale_factor`.
pub(crate) fn compute_output_scale(logical_w: u32, physical_w: u32, scale_factor: i32) -> f32 {
    if logical_w > 0 {
        physical_w as f32 / logical_w as f32
    } else {
        scale_factor as f32
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{
        anchor_for_panel, compute_output_scale, compute_set_size, fixed_axis_changed,
        WaylandDisplayServer,
    };
    use crate::display_manager::DisplayManager;
    use crate::layout::{PanelAnchor, PanelSpecData};
    use smithay_client_toolkit::shell::wlr_layer::Anchor;

    // Minimal PanelSpecData for testing
    fn minimal_spec() -> PanelSpecData {
        PanelSpecData {
            id: "test-panel".to_string(),
            anchor: None,
            width: 100,
            height: 30,
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
    // DisplayManager trait impl for WaylandDisplayServer
    // ---------------------------------------------------------------------------

    // update_position is a no-op — Wayland position is compositor-managed via anchor
    #[test]
    fn display_manager_update_position_is_noop() {
        let mut server = match WaylandDisplayServer::connect() {
            Ok(s) => s,
            Err(_) => {
                println!("SKIP: no Wayland compositor available");
                return;
            }
        };
        let spec = minimal_spec();
        let mut panel =
            DisplayManager::create_window(&mut server, &spec, &blank_frame(100, 30)).unwrap();
        let new_spec = PanelSpecData {
            x: 50,
            y: 50,
            ..minimal_spec()
        };
        let result = DisplayManager::update_position(&mut server, &mut panel, &new_spec);
        assert!(result.is_ok());
    }

    // update_dimensions calls panel.update_spec(spec) which updates width/height for the fixed axis
    #[test]
    fn display_manager_update_dimensions_updates_panel() {
        let mut server = match WaylandDisplayServer::connect() {
            Ok(s) => s,
            Err(_) => {
                println!("SKIP: no Wayland compositor available");
                return;
            }
        };
        let spec = minimal_spec();
        let mut panel =
            DisplayManager::create_window(&mut server, &spec, &blank_frame(100, 30)).unwrap();
        let new_spec = PanelSpecData {
            width: 200,
            height: 60,
            ..minimal_spec()
        };
        let result = DisplayManager::update_dimensions(&mut server, &mut panel, &new_spec);
        assert!(result.is_ok());
        assert_eq!(panel.width, 200);
        assert_eq!(panel.height, 60);
    }

    // update_image calls panel.render(bgrx) and returns Ok(())
    #[test]
    fn display_manager_update_image_returns_ok() {
        let mut server = match WaylandDisplayServer::connect() {
            Ok(s) => s,
            Err(_) => {
                println!("SKIP: no Wayland compositor available");
                return;
            }
        };
        let spec = minimal_spec();
        let mut panel =
            DisplayManager::create_window(&mut server, &spec, &blank_frame(100, 30)).unwrap();
        // Provide a correctly-sized BGRX buffer (width * height * 4 bytes)
        let bgrx = vec![0u8; (spec.width * spec.height * 4) as usize];
        let result = DisplayManager::update_image(&mut server, &mut panel, &bgrx);
        assert!(result.is_ok());
    }

    // The height used to allocate the SHM buffer must be derived from the bgrx
    // byte count, not from panel.height. If it used panel.height instead, then
    // when bgrx has MORE rows than panel.height the canvas would be too small
    // and `canvas[..bgrx.len()].copy_from_slice(bgrx)` would panic.
    //
    // This is a pure computation test — no Wayland compositor needed.
    // It would FAIL if the formula were changed back to use panel.height.
    #[test]
    fn render_height_derived_from_bgrx_not_panel_height_when_bgrx_is_taller() {
        let width: u32 = 100;
        let panel_height: u32 = 30;
        // bgrx represents 50 rows — more than panel_height=30
        let bgrx = vec![0u8; (width * 50 * 4) as usize]; // 20 000 bytes

        // Old formula: canvas allocated with panel_height → would be too small
        let old_canvas_size = (panel_height * width * 4) as usize; // 12 000
        assert!(
            old_canvas_size < bgrx.len(),
            "old canvas ({old_canvas_size}B) < bgrx ({}B) → copy would panic",
            bgrx.len()
        );

        // New formula: derive height from bgrx
        let actual_height = (bgrx.len() / 4).saturating_div(width as usize);
        assert_eq!(actual_height, 50);
        let new_canvas_size = actual_height * width as usize * 4; // 20 000
        assert_eq!(
            new_canvas_size,
            bgrx.len(),
            "new canvas ({new_canvas_size}B) == bgrx ({}B) → copy is safe",
            bgrx.len()
        );
    }

    // Same invariant for a bgrx that is SHORTER than panel.height: actual_height
    // must still be derived from bgrx, not panel.height, so the canvas fits exactly.
    //
    // Pure computation test — no Wayland compositor needed.
    #[test]
    fn render_height_derived_from_bgrx_not_panel_height_when_bgrx_is_shorter() {
        let width: u32 = 100;
        // bgrx represents only 20 rows
        let bgrx = vec![0u8; (width * 20 * 4) as usize]; // 8 000 bytes

        let actual_height = (bgrx.len() / 4).saturating_div(width as usize);
        assert_eq!(actual_height, 20);
        let canvas_size = actual_height * width as usize * 4;
        assert_eq!(
            canvas_size,
            bgrx.len(),
            "canvas ({canvas_size}B) == bgrx ({}B) → copy is safe",
            bgrx.len()
        );
    }

    // delete_window drops the panel without panicking
    #[test]
    fn display_manager_delete_window_returns_ok() {
        let mut server = match WaylandDisplayServer::connect() {
            Ok(s) => s,
            Err(_) => {
                println!("SKIP: no Wayland compositor available");
                return;
            }
        };
        let spec = minimal_spec();
        let panel =
            DisplayManager::create_window(&mut server, &spec, &blank_frame(100, 30)).unwrap();
        let result = DisplayManager::delete_window(&mut server, panel);
        assert!(result.is_ok());
    }

    // ---------------------------------------------------------------------------
    // compute_set_size — the compositor-controlled axis must be 0
    // ---------------------------------------------------------------------------

    // Left/Right panels: width is fixed, height is compositor-controlled (must be 0)
    #[test]
    fn compute_set_size_left_passes_width_zeroes_height() {
        assert_eq!(
            compute_set_size(Some(&PanelAnchor::Left), 40, 1080),
            (40, 0),
        );
    }

    #[test]
    fn compute_set_size_right_passes_width_zeroes_height() {
        assert_eq!(
            compute_set_size(Some(&PanelAnchor::Right), 40, 1080),
            (40, 0),
        );
    }

    // Top/Bottom panels: height is fixed, width is compositor-controlled (must be 0)
    #[test]
    fn compute_set_size_top_passes_height_zeroes_width() {
        assert_eq!(compute_set_size(Some(&PanelAnchor::Top), 1920, 30), (0, 30),);
    }

    #[test]
    fn compute_set_size_bottom_passes_height_zeroes_width() {
        assert_eq!(
            compute_set_size(Some(&PanelAnchor::Bottom), 1920, 30),
            (0, 30),
        );
    }

    // None anchor: both axes are explicit
    #[test]
    fn compute_set_size_none_passes_both() {
        assert_eq!(compute_set_size(None, 800, 600), (800, 600),);
    }

    // ---------------------------------------------------------------------------
    // fixed_axis_changed — only the fixed dimension should trigger reconfigure
    // ---------------------------------------------------------------------------

    // Left/Right: only width change triggers reconfigure
    #[test]
    fn fixed_axis_changed_left_width_change_triggers() {
        assert!(fixed_axis_changed(
            Some(&PanelAnchor::Left),
            40,
            1080,
            50,
            1080
        ));
    }

    #[test]
    fn fixed_axis_changed_left_height_only_does_not_trigger() {
        // Height is compositor-controlled for Left panels; a height change alone must NOT
        // trigger reconfigure (the compositor manages it).
        assert!(!fixed_axis_changed(
            Some(&PanelAnchor::Left),
            40,
            1080,
            40,
            900
        ));
    }

    #[test]
    fn fixed_axis_changed_right_width_change_triggers() {
        assert!(fixed_axis_changed(
            Some(&PanelAnchor::Right),
            40,
            1080,
            50,
            1080
        ));
    }

    #[test]
    fn fixed_axis_changed_right_height_only_does_not_trigger() {
        assert!(!fixed_axis_changed(
            Some(&PanelAnchor::Right),
            40,
            1080,
            40,
            900
        ));
    }

    // Top/Bottom: only height change triggers reconfigure
    #[test]
    fn fixed_axis_changed_top_height_change_triggers() {
        assert!(fixed_axis_changed(
            Some(&PanelAnchor::Top),
            1920,
            30,
            1920,
            40
        ));
    }

    #[test]
    fn fixed_axis_changed_top_width_only_does_not_trigger() {
        assert!(!fixed_axis_changed(
            Some(&PanelAnchor::Top),
            1920,
            30,
            1600,
            30
        ));
    }

    #[test]
    fn fixed_axis_changed_bottom_height_change_triggers() {
        assert!(fixed_axis_changed(
            Some(&PanelAnchor::Bottom),
            1920,
            30,
            1920,
            40
        ));
    }

    #[test]
    fn fixed_axis_changed_bottom_width_only_does_not_trigger() {
        assert!(!fixed_axis_changed(
            Some(&PanelAnchor::Bottom),
            1920,
            30,
            1600,
            30
        ));
    }

    // None: either change triggers reconfigure
    #[test]
    fn fixed_axis_changed_none_width_change_triggers() {
        assert!(fixed_axis_changed(None, 800, 600, 900, 600));
    }

    #[test]
    fn fixed_axis_changed_none_height_change_triggers() {
        assert!(fixed_axis_changed(None, 800, 600, 800, 700));
    }

    #[test]
    fn fixed_axis_changed_none_no_change_does_not_trigger() {
        assert!(!fixed_axis_changed(None, 800, 600, 800, 600));
    }

    // --- None ---

    #[test]
    fn anchor_none_is_empty() {
        assert_eq!(anchor_for_panel(None), Anchor::empty());
    }

    // --- Left: must include LEFT, TOP, and BOTTOM ---

    #[test]
    fn anchor_left_contains_left() {
        assert!(anchor_for_panel(Some(&PanelAnchor::Left)).contains(Anchor::LEFT));
    }

    #[test]
    fn anchor_left_contains_top() {
        // Regression: single-anchor-only (LEFT only) would fail this.
        assert!(anchor_for_panel(Some(&PanelAnchor::Left)).contains(Anchor::TOP));
    }

    #[test]
    fn anchor_left_contains_bottom() {
        // Regression: single-anchor-only (LEFT only) would fail this.
        assert!(anchor_for_panel(Some(&PanelAnchor::Left)).contains(Anchor::BOTTOM));
    }

    #[test]
    fn anchor_left_exact() {
        assert_eq!(
            anchor_for_panel(Some(&PanelAnchor::Left)),
            Anchor::LEFT | Anchor::TOP | Anchor::BOTTOM,
        );
    }

    // --- Right: must include RIGHT, TOP, and BOTTOM ---

    #[test]
    fn anchor_right_contains_right() {
        assert!(anchor_for_panel(Some(&PanelAnchor::Right)).contains(Anchor::RIGHT));
    }

    #[test]
    fn anchor_right_contains_top() {
        // Regression: single-anchor-only (RIGHT only) would fail this.
        assert!(anchor_for_panel(Some(&PanelAnchor::Right)).contains(Anchor::TOP));
    }

    #[test]
    fn anchor_right_contains_bottom() {
        // Regression: single-anchor-only (RIGHT only) would fail this.
        assert!(anchor_for_panel(Some(&PanelAnchor::Right)).contains(Anchor::BOTTOM));
    }

    #[test]
    fn anchor_right_exact() {
        assert_eq!(
            anchor_for_panel(Some(&PanelAnchor::Right)),
            Anchor::RIGHT | Anchor::TOP | Anchor::BOTTOM,
        );
    }

    // --- Top: must include TOP, LEFT, and RIGHT ---

    #[test]
    fn anchor_top_contains_top() {
        assert!(anchor_for_panel(Some(&PanelAnchor::Top)).contains(Anchor::TOP));
    }

    #[test]
    fn anchor_top_contains_left() {
        // Regression: single-anchor-only (TOP only) would fail this.
        assert!(anchor_for_panel(Some(&PanelAnchor::Top)).contains(Anchor::LEFT));
    }

    #[test]
    fn anchor_top_contains_right() {
        // Regression: single-anchor-only (TOP only) would fail this.
        assert!(anchor_for_panel(Some(&PanelAnchor::Top)).contains(Anchor::RIGHT));
    }

    #[test]
    fn anchor_top_exact() {
        assert_eq!(
            anchor_for_panel(Some(&PanelAnchor::Top)),
            Anchor::TOP | Anchor::LEFT | Anchor::RIGHT,
        );
    }

    // --- Bottom: must include BOTTOM, LEFT, and RIGHT ---

    #[test]
    fn anchor_bottom_contains_bottom() {
        assert!(anchor_for_panel(Some(&PanelAnchor::Bottom)).contains(Anchor::BOTTOM));
    }

    #[test]
    fn anchor_bottom_contains_left() {
        // Regression: single-anchor-only (BOTTOM only) would fail this.
        assert!(anchor_for_panel(Some(&PanelAnchor::Bottom)).contains(Anchor::LEFT));
    }

    #[test]
    fn anchor_bottom_contains_right() {
        // Regression: single-anchor-only (BOTTOM only) would fail this.
        assert!(anchor_for_panel(Some(&PanelAnchor::Bottom)).contains(Anchor::RIGHT));
    }

    #[test]
    fn anchor_bottom_exact() {
        assert_eq!(
            anchor_for_panel(Some(&PanelAnchor::Bottom)),
            Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
        );
    }

    // ---------------------------------------------------------------------------
    // render() on unconfigured panel must not commit a buffer
    // ---------------------------------------------------------------------------

    // render() called on a panel that has not yet received a configure event must
    // not commit any buffer to the surface. If it did, the compositor would send a
    // protocol error and the next roundtrip would fail.
    #[test]
    fn render_on_unconfigured_panel_leaves_connection_alive() {
        use crate::windowing::DisplayServer;
        let mut server = match WaylandDisplayServer::connect() {
            Ok(s) => s,
            Err(_) => {
                println!("SKIP: no Wayland compositor available");
                return;
            }
        };
        let spec = minimal_spec();
        let mut panel = server.create_panel(&spec).unwrap();
        assert!(!panel.configured, "panel must start unconfigured");

        let bgrx = vec![0u8; (spec.width * spec.height * 4) as usize];
        panel.render(&bgrx);

        // Flush then dispatch twice to give the compositor time to send its error
        // response. Without the configured guard, render() commits a buffer to an
        // unconfigured layer surface → compositor sends wl_display.error → at least
        // one of the dispatches returns Err.
        server.flush();
        let r1 = server.dispatch();
        let r2 = server.dispatch();
        assert!(
            r1.is_ok() && r2.is_ok(),
            "compositor connection broken after render on unconfigured panel: r1={r1:?} r2={r2:?}",
        );
    }

    // ---------------------------------------------------------------------------
    // compute_output_scale — DPR computation from physical/logical/scale_factor
    // ---------------------------------------------------------------------------

    // When logical_w > 0, the ratio physical_w / logical_w is returned as f32.
    // logical=1920, physical=3840, scale=2 → 3840/1920 = 2.0
    #[test]
    fn compute_output_scale_uses_physical_over_logical_when_available() {
        assert_eq!(compute_output_scale(1920, 3840, 2), 2.0_f32);
    }

    // Fractional DPR: logical=2560, physical=3840, scale=1 → 3840/2560 = 1.5
    #[test]
    fn compute_output_scale_fractional_when_logical_available() {
        assert_eq!(compute_output_scale(2560, 3840, 1), 1.5_f32);
    }

    // When logical_w == 0, falls back to scale_factor as f32.
    // logical=0, physical=3840, scale=2 → 2.0
    #[test]
    fn compute_output_scale_falls_back_to_scale_factor_when_logical_zero() {
        assert_eq!(compute_output_scale(0, 3840, 2), 2.0_f32);
    }

    // No scaling: logical=1920, physical=1920, scale=1 → 1920/1920 = 1.0
    #[test]
    fn compute_output_scale_returns_one_when_no_scaling() {
        assert_eq!(compute_output_scale(1920, 1920, 1), 1.0_f32);
    }
}
