use std::collections::HashMap;
use std::sync::Arc;

use crate::display_manager::DisplayManager;
use crate::layout::{OutputInfo, PanelSpecData};

/// A rasterized panel frame ready to be committed to a display.
///
/// Pixel data is `Arc<Vec<u8>>` so the pipeline, the command channel, and
/// the presenter's coalescing buffer share one allocation via ref-count.
/// X11's existing `Panel::bgrx` is already this type, so the pipeline can
/// clone the Arc directly with no byte copy.
#[derive(Clone)]
pub struct PanelFrame {
    pub pixels: Arc<Vec<u8>>,
    pub width: u32,
    pub height: u32,
}

/// The typed vocabulary the pipeline speaks to the presenter.
///
/// Lifecycle variants (`Create`, `Move`, `Resize`, `Delete`) are applied
/// immediately by the presenter thread. `UpdatePicture` triggers a
/// `DM::update_image` call on the presenter thread as soon as it is drained
/// from the command channel. `Shutdown` is intercepted by `drain_commands`
/// before reaching `Presenter::apply` — it is never passed to `apply`.
pub enum PanelCommand {
    Create { spec: PanelSpecData, frame: PanelFrame },
    Move(PanelSpecData),
    Resize { spec: PanelSpecData, frame: PanelFrame },
    Delete { id: String },
    UpdatePicture { id: String, frame: PanelFrame },
    Shutdown,
}

/// Events the presenter thread sends back to the pipeline.
pub enum PresenterEvent {
    /// The pipeline should re-render all panels and flush.
    NeedsRender,
    /// The set of connected outputs (and their DPRs) has changed.
    OutputsChanged { outputs: Vec<OutputInfo> },
    /// A click event, routed back for hit-testing in the pipeline.
    Click { panel_id: String, x: f32, y: f32, phys_width: u32, phys_height: u32, dpr: f32 },
}

/// Owns the window state (one `DM::Panel` per live panel id). Does NOT own
/// the `DisplayManager` — callers pass `&mut DM` into `apply`.
pub struct Presenter<DM: DisplayManager> {
    pub panels: HashMap<String, DM::Panel>,
}

/// Bundles `dm: DM` and `presenter: Presenter<DM>` so they travel together
/// as one owned unit. Lives on a dedicated thread; the main `App` interacts
/// with it only through `PanelCommand` / `PresenterEvent` mpsc channels.
pub struct PresentationThread<DM: DisplayManager> {
    pub dm: DM,
    pub presenter: Presenter<DM>,
}

impl<DM: DisplayManager> PresentationThread<DM> {
    pub fn new(dm: DM) -> Self {
        Self { dm, presenter: Presenter::new() }
    }
}

impl<DM: DisplayManager> Default for Presenter<DM> {
    fn default() -> Self {
        Self { panels: HashMap::new() }
    }
}

impl<DM: DisplayManager> Presenter<DM> {
    pub fn new() -> Self { Self::default() }

    pub fn apply(&mut self, cmd: PanelCommand, dm: &mut DM) -> anyhow::Result<()> {
        match cmd {
            PanelCommand::Create { spec, frame } => {
                let id = spec.id.clone();
                let panel = dm.create_window(&spec, &frame)?;
                self.panels.insert(id, panel);
            }
            PanelCommand::Move(spec) => {
                if let Some(panel) = self.panels.get_mut(&spec.id) {
                    dm.update_position(panel, &spec)?;
                }
            }
            PanelCommand::Resize { spec, frame } => {
                if let Some(panel) = self.panels.get_mut(&spec.id) {
                    dm.update_dimensions(panel, &spec)?;
                    if let Err(e) = dm.update_image(panel, &frame.pixels[..]) {
                        tracing::error!(panel = %spec.id, error = %e, "presenter resize update_image failed");
                    }
                }
            }
            PanelCommand::Delete { id } => {
                if let Some(panel) = self.panels.remove(&id) {
                    dm.delete_window(panel)?;
                }
            }
            PanelCommand::UpdatePicture { id, frame } => {
                if let Some(panel) = self.panels.get_mut(&id) {
                    if let Err(e) = dm.update_image(panel, &frame.pixels[..]) {
                        tracing::error!(panel = %id, error = %e, "presenter update_image failed");
                    }
                }
            }
            PanelCommand::Shutdown => unreachable!("Shutdown is intercepted by drain_commands before apply is called"),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockDM {
        calls: Vec<String>,
        next_id: u32,
    }

    impl MockDM {
        fn new() -> Self { MockDM { calls: Vec::new(), next_id: 0 } }
    }

    impl DisplayManager for MockDM {
        type Panel = u32;
        fn create_window(&mut self, spec: &PanelSpecData, _frame: &PanelFrame) -> anyhow::Result<u32> {
            self.next_id += 1;
            self.calls.push(format!("create:{}:{}", spec.id, self.next_id));
            Ok(self.next_id)
        }
        fn update_position(&mut self, panel: &mut u32, spec: &PanelSpecData) -> anyhow::Result<()> {
            self.calls.push(format!("move:{}:{}", spec.id, panel)); Ok(())
        }
        fn update_dimensions(&mut self, panel: &mut u32, spec: &PanelSpecData) -> anyhow::Result<()> {
            self.calls.push(format!("resize:{}:{}", spec.id, panel)); Ok(())
        }
        fn update_image(&mut self, panel: &mut u32, _bgrx: &[u8]) -> anyhow::Result<()> {
            self.calls.push(format!("image:{}", panel)); Ok(())
        }
        fn delete_window(&mut self, panel: u32) -> anyhow::Result<()> {
            self.calls.push(format!("delete:{}", panel)); Ok(())
        }
    }

    fn spec(id: &str) -> PanelSpecData {
        PanelSpecData {
            id: id.to_string(),
            anchor: None,
            width: 100, height: 30,
            x: 0, y: 0,
            outer_gap: 0,
            output: None,
            above: false,
            content: serde_json::Value::Null,
            dpr: 1.0,
        }
    }

    fn blank_frame() -> PanelFrame {
        PanelFrame { pixels: Arc::new(vec![0u8; 4]), width: 1, height: 1 }
    }

    #[test]
    fn presenter_create_calls_dm_create_window_and_tracks_panel() {
        let mut p: Presenter<MockDM> = Presenter::new();
        let mut dm = MockDM::new();
        p.apply(PanelCommand::Create { spec: spec("p1"), frame: blank_frame() }, &mut dm).unwrap();
        assert!(p.panels.contains_key("p1"), "panel id must be tracked after Create");
        assert!(dm.calls.iter().any(|c| c.starts_with("create:p1")), "dm.calls: {:?}", dm.calls);
    }

    #[test]
    fn presenter_delete_removes_panel_and_calls_dm_delete_window() {
        let mut p: Presenter<MockDM> = Presenter::new();
        let mut dm = MockDM::new();
        p.apply(PanelCommand::Create { spec: spec("p1"), frame: blank_frame() }, &mut dm).unwrap();
        p.apply(PanelCommand::Delete { id: "p1".to_string() }, &mut dm).unwrap();
        assert!(!p.panels.contains_key("p1"), "panel id must be removed after Delete");
        assert!(dm.calls.iter().any(|c| c.starts_with("delete:")), "dm.calls: {:?}", dm.calls);
    }

    #[test]
    fn presenter_update_picture_calls_update_image_immediately() {
        let mut p: Presenter<MockDM> = Presenter::new();
        let mut dm = MockDM::new();
        p.apply(PanelCommand::Create { spec: spec("p1"), frame: blank_frame() }, &mut dm).unwrap();
        let frame = PanelFrame { pixels: Arc::new(vec![42u8; 4]), width: 1, height: 1 };
        p.apply(PanelCommand::UpdatePicture { id: "p1".to_string(), frame }, &mut dm).unwrap();
        assert!(dm.calls.iter().any(|c| c.starts_with("image:")),
            "UpdatePicture must call dm.update_image immediately; got {:?}", dm.calls);
    }

    #[test]
    fn presenter_update_picture_for_unknown_panel_is_noop() {
        let mut p: Presenter<MockDM> = Presenter::new();
        let mut dm = MockDM::new();
        let frame = PanelFrame { pixels: Arc::new(vec![42u8; 4]), width: 1, height: 1 };
        p.apply(PanelCommand::UpdatePicture { id: "ghost".to_string(), frame }, &mut dm).unwrap();
        assert!(!dm.calls.iter().any(|c| c.starts_with("image:")),
            "UpdatePicture for unknown panel must not call update_image; got {:?}", dm.calls);
    }

    #[test]
    fn presenter_move_and_resize_only_affect_matching_id() {
        let mut p: Presenter<MockDM> = Presenter::new();
        let mut dm = MockDM::new();
        p.apply(PanelCommand::Create { spec: spec("p1"), frame: blank_frame() }, &mut dm).unwrap();
        p.apply(PanelCommand::Move(spec("p2")), &mut dm).unwrap(); // unknown id: no-op
        p.apply(PanelCommand::Resize { spec: spec("p1"), frame: blank_frame() }, &mut dm).unwrap();
        assert!(dm.calls.iter().any(|c| c.starts_with("resize:p1")), "Resize on known id must call dm");
        assert!(!dm.calls.iter().any(|c| c.starts_with("move:")), "Move on unknown id must be a no-op");
    }
}
