use std::collections::HashMap;
use std::sync::Arc;

use crate::display_manager::DisplayManager;
use crate::layout::{OutputInfo, SurfaceSpec};

/// A rasterized panel frame ready to be committed to a display.
///
/// Pixel data is `Arc<Vec<u8>>` so the pipeline, the command channel, and
/// the presenter's coalescing buffer share one allocation via ref-count.
/// X11's existing `Panel::bgrx` is already this type, so the pipeline can
/// clone the Arc directly with no byte copy.
#[derive(Clone, Debug)]
pub struct SurfaceFrame {
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
#[derive(Debug)]
pub enum SurfaceCommand {
    Create {
        spec: SurfaceSpec,
        frame: SurfaceFrame,
    },
    Move(SurfaceSpec),
    Resize {
        spec: SurfaceSpec,
        frame: SurfaceFrame,
    },
    Delete {
        id: String,
    },
    UpdatePicture {
        id: String,
        frame: SurfaceFrame,
    },
    /// Paint a `<wallpaper>` into its output's slice of the desktop background.
    ///
    /// Wallpapers get their own command because their whole lifecycle is one
    /// verb. They own no window, so there is nothing to move, resize or destroy,
    /// and re-painting is the same operation as first painting. Riding the panel
    /// variants would mean a wallpaper branch inside `Create`, `Delete` and
    /// `UpdatePicture` to say "not that kind" three times over.
    PaintWallpaper {
        spec: SurfaceSpec,
        frame: SurfaceFrame,
    },
    Shutdown,
}

/// Events the presenter thread sends back to the pipeline.
/// Where in a gesture a pointer event falls (`docs/adr/0020`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerPhase {
    /// A button went down. Fires `on_click` and `on_drag`, and starts a capture.
    Press,
    /// The pointer moved with a button held. Only a capture answers these.
    Move,
    /// The button came up. Ends the capture; dispatches nothing.
    Release,
}

/// A press, a motion or a release, with the surface it landed on.
///
/// One type rather than eight arguments: every stage from the presenter to the
/// capture state machine needs the same set, and they only ever travel together.
#[derive(Debug, Clone)]
pub struct PointerEvent {
    pub panel_id: String,
    /// Physical pixels, relative to the panel.
    pub x: f32,
    pub y: f32,
    pub phys_width: u32,
    pub phys_height: u32,
    pub dpr: f32,
    pub phase: PointerPhase,
    /// Which buttons are held, as a DOM `buttons` bitmask: 1 primary, 2 secondary,
    /// 4 auxiliary. Handed to handlers untouched.
    pub buttons: u16,
}

pub enum PresenterEvent {
    /// The pipeline should re-render all panels and flush.
    NeedsRender,
    /// The set of connected outputs (and their DPRs) has changed.
    OutputsChanged { outputs: Vec<OutputInfo> },
    /// A pointer event, routed back for hit-testing in the pipeline.
    Pointer(PointerEvent),
}

/// Owns the window state: one `DM::Panel` per live panel id. Does NOT own the
/// `DisplayManager` — callers pass `&mut DM` into `apply`.
///
/// Wallpapers are deliberately absent. They own no window and keep no handle, so
/// there is nothing to track between paints — `PaintWallpaper` carries
/// everything the backend needs.
pub struct Presenter<DM: DisplayManager> {
    pub panels: HashMap<String, DM::Panel>,
}

/// Bundles `dm: DM` and `presenter: Presenter<DM>` so they travel together
/// as one owned unit. Lives on a dedicated thread; the main `App` interacts
/// with it only through `SurfaceCommand` / `PresenterEvent` mpsc channels.
pub struct PresentationThread<DM: DisplayManager> {
    pub dm: DM,
    pub presenter: Presenter<DM>,
}

impl<DM: DisplayManager> PresentationThread<DM> {
    pub fn new(dm: DM) -> Self {
        Self {
            dm,
            presenter: Presenter::new(),
        }
    }
}

impl<DM: DisplayManager> Default for Presenter<DM> {
    fn default() -> Self {
        Self {
            panels: HashMap::new(),
        }
    }
}

impl<DM: DisplayManager> Presenter<DM> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn apply(&mut self, cmd: SurfaceCommand, dm: &mut DM) -> anyhow::Result<()> {
        match cmd {
            SurfaceCommand::Create { spec, frame } => {
                let id = spec.id.clone();
                let panel = dm.create_window(&spec, &frame)?;
                self.panels.insert(id, panel);
            }
            SurfaceCommand::Move(spec) => {
                if let Some(panel) = self.panels.get_mut(&spec.id) {
                    dm.update_position(panel, &spec)?;
                }
            }
            SurfaceCommand::Resize { spec, frame } => {
                if let Some(panel) = self.panels.get_mut(&spec.id) {
                    dm.update_dimensions(panel, &spec)?;
                    if let Err(e) = dm.update_image(panel, &frame.pixels[..]) {
                        tracing::error!(panel = %spec.id, error = %e, "presenter resize update_image failed");
                    }
                }
            }
            SurfaceCommand::Delete { id } => {
                if let Some(panel) = self.panels.remove(&id) {
                    dm.delete_window(panel)?;
                }
            }
            SurfaceCommand::UpdatePicture { id, frame } => {
                if let Some(panel) = self.panels.get_mut(&id) {
                    if let Err(e) = dm.update_image(panel, &frame.pixels[..]) {
                        tracing::error!(panel = %id, error = %e, "presenter update_image failed");
                    }
                }
            }
            SurfaceCommand::PaintWallpaper { spec, frame } => {
                dm.paint_wallpaper(&spec, &frame)?;
            }
            SurfaceCommand::Shutdown => {
                unreachable!("Shutdown is intercepted by drain_commands before apply is called")
            }
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
        wallpaper_supported: bool,
    }

    impl MockDM {
        fn new() -> Self {
            MockDM {
                calls: Vec::new(),
                next_id: 0,
                wallpaper_supported: true,
            }
        }
    }

    impl DisplayManager for MockDM {
        type Panel = u32;
        fn create_window(
            &mut self,
            spec: &SurfaceSpec,
            _frame: &SurfaceFrame,
        ) -> anyhow::Result<u32> {
            self.next_id += 1;
            self.calls
                .push(format!("create:{}:{}", spec.id, self.next_id));
            Ok(self.next_id)
        }
        fn update_position(&mut self, panel: &mut u32, spec: &SurfaceSpec) -> anyhow::Result<()> {
            self.calls.push(format!("move:{}:{}", spec.id, panel));
            Ok(())
        }
        fn update_dimensions(&mut self, panel: &mut u32, spec: &SurfaceSpec) -> anyhow::Result<()> {
            self.calls.push(format!("resize:{}:{}", spec.id, panel));
            Ok(())
        }
        fn update_image(&mut self, panel: &mut u32, _bgrx: &[u8]) -> anyhow::Result<()> {
            self.calls.push(format!("image:{}", panel));
            Ok(())
        }
        fn delete_window(&mut self, panel: u32) -> anyhow::Result<()> {
            self.calls.push(format!("delete:{}", panel));
            Ok(())
        }
        fn paint_wallpaper(
            &mut self,
            spec: &SurfaceSpec,
            _frame: &SurfaceFrame,
        ) -> anyhow::Result<()> {
            if !self.wallpaper_supported {
                anyhow::bail!("wallpaper unsupported");
            }
            self.calls.push(format!("paint_wallpaper:{}", spec.id));
            Ok(())
        }
    }

    fn spec(id: &str) -> SurfaceSpec {
        SurfaceSpec {
            kind: crate::layout::SurfaceKind::Panel,
            id: id.to_string(),
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

    fn blank_frame() -> SurfaceFrame {
        SurfaceFrame {
            pixels: Arc::new(vec![0u8; 4]),
            width: 1,
            height: 1,
        }
    }

    fn wallpaper_spec(id: &str) -> SurfaceSpec {
        SurfaceSpec {
            kind: crate::layout::SurfaceKind::Wallpaper,
            ..spec(id)
        }
    }

    #[test]
    fn presenter_paint_wallpaper_calls_the_backend_and_tracks_nothing() {
        let mut p: Presenter<MockDM> = Presenter::new();
        let mut dm = MockDM::new();
        p.apply(
            SurfaceCommand::PaintWallpaper {
                spec: wallpaper_spec("bg"),
                frame: blank_frame(),
            },
            &mut dm,
        )
        .unwrap();
        assert!(
            !p.panels.contains_key("bg"),
            "a wallpaper owns no window, so it must not be tracked as a panel"
        );
        assert!(
            dm.calls.iter().any(|c| c == "paint_wallpaper:bg"),
            "dm.calls: {:?}",
            dm.calls
        );
    }

    /// Repainting is the same operation as first painting — there is no
    /// create/update distinction to get wrong.
    #[test]
    fn presenter_repeated_paint_wallpaper_just_repaints() {
        let mut p: Presenter<MockDM> = Presenter::new();
        let mut dm = MockDM::new();
        for _ in 0..2 {
            p.apply(
                SurfaceCommand::PaintWallpaper {
                    spec: wallpaper_spec("bg"),
                    frame: blank_frame(),
                },
                &mut dm,
            )
            .unwrap();
        }
        assert_eq!(
            dm.calls
                .iter()
                .filter(|c| *c == "paint_wallpaper:bg")
                .count(),
            2,
            "each PaintWallpaper must reach the backend; got {:?}",
            dm.calls
        );
    }

    /// A backend with no wallpaper support (Wayland, macOS) surfaces the failure
    /// rather than silently dropping the node.
    #[test]
    fn presenter_surfaces_the_error_when_backend_rejects_a_wallpaper() {
        let mut p: Presenter<MockDM> = Presenter::new();
        let mut dm = MockDM::new();
        dm.wallpaper_supported = false;
        let result = p.apply(
            SurfaceCommand::PaintWallpaper {
                spec: wallpaper_spec("bg"),
                frame: blank_frame(),
            },
            &mut dm,
        );
        assert!(result.is_err(), "the backend's error must propagate");
    }

    #[test]
    fn presenter_create_calls_dm_create_window_and_tracks_panel() {
        let mut p: Presenter<MockDM> = Presenter::new();
        let mut dm = MockDM::new();
        p.apply(
            SurfaceCommand::Create {
                spec: spec("p1"),
                frame: blank_frame(),
            },
            &mut dm,
        )
        .unwrap();
        assert!(
            p.panels.contains_key("p1"),
            "panel id must be tracked after Create"
        );
        assert!(
            dm.calls.iter().any(|c| c.starts_with("create:p1")),
            "dm.calls: {:?}",
            dm.calls
        );
    }

    #[test]
    fn presenter_delete_removes_panel_and_calls_dm_delete_window() {
        let mut p: Presenter<MockDM> = Presenter::new();
        let mut dm = MockDM::new();
        p.apply(
            SurfaceCommand::Create {
                spec: spec("p1"),
                frame: blank_frame(),
            },
            &mut dm,
        )
        .unwrap();
        p.apply(
            SurfaceCommand::Delete {
                id: "p1".to_string(),
            },
            &mut dm,
        )
        .unwrap();
        assert!(
            !p.panels.contains_key("p1"),
            "panel id must be removed after Delete"
        );
        assert!(
            dm.calls.iter().any(|c| c.starts_with("delete:")),
            "dm.calls: {:?}",
            dm.calls
        );
    }

    #[test]
    fn presenter_update_picture_calls_update_image_immediately() {
        let mut p: Presenter<MockDM> = Presenter::new();
        let mut dm = MockDM::new();
        p.apply(
            SurfaceCommand::Create {
                spec: spec("p1"),
                frame: blank_frame(),
            },
            &mut dm,
        )
        .unwrap();
        let frame = SurfaceFrame {
            pixels: Arc::new(vec![42u8; 4]),
            width: 1,
            height: 1,
        };
        p.apply(
            SurfaceCommand::UpdatePicture {
                id: "p1".to_string(),
                frame,
            },
            &mut dm,
        )
        .unwrap();
        assert!(
            dm.calls.iter().any(|c| c.starts_with("image:")),
            "UpdatePicture must call dm.update_image immediately; got {:?}",
            dm.calls
        );
    }

    #[test]
    fn presenter_update_picture_for_unknown_panel_is_noop() {
        let mut p: Presenter<MockDM> = Presenter::new();
        let mut dm = MockDM::new();
        let frame = SurfaceFrame {
            pixels: Arc::new(vec![42u8; 4]),
            width: 1,
            height: 1,
        };
        p.apply(
            SurfaceCommand::UpdatePicture {
                id: "ghost".to_string(),
                frame,
            },
            &mut dm,
        )
        .unwrap();
        assert!(
            !dm.calls.iter().any(|c| c.starts_with("image:")),
            "UpdatePicture for unknown panel must not call update_image; got {:?}",
            dm.calls
        );
    }

    #[test]
    fn presenter_move_and_resize_only_affect_matching_id() {
        let mut p: Presenter<MockDM> = Presenter::new();
        let mut dm = MockDM::new();
        p.apply(
            SurfaceCommand::Create {
                spec: spec("p1"),
                frame: blank_frame(),
            },
            &mut dm,
        )
        .unwrap();
        p.apply(SurfaceCommand::Move(spec("p2")), &mut dm).unwrap(); // unknown id: no-op
        p.apply(
            SurfaceCommand::Resize {
                spec: spec("p1"),
                frame: blank_frame(),
            },
            &mut dm,
        )
        .unwrap();
        assert!(
            dm.calls.iter().any(|c| c.starts_with("resize:p1")),
            "Resize on known id must call dm"
        );
        assert!(
            !dm.calls.iter().any(|c| c.starts_with("move:")),
            "Move on unknown id must be a no-op"
        );
    }
}
