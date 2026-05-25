use std::sync::mpsc::Sender;

use crate::layout::PanelSpecData;
use crate::managed_set::Lifecycle;
use crate::presentation::{PanelCommand, PanelFrame};
use crate::render::render_frame_partial;
pub use crate::x11::panel::X11PanelContext;
use takumi_incr::PartialRenderScene;

pub struct PanelState {
    pub spec: PanelSpecData,
    pub scene: PartialRenderScene,
}

// ---------------------------------------------------------------------------
// PanelSpec — pipeline-side tracker of desired panels. Emits typed
// PanelCommand messages on lifecycle transitions; does NOT call DisplayManager
// methods directly. The presenter (src/presentation) applies the commands to
// an actual backend.
// ---------------------------------------------------------------------------

pub struct PanelSpec(pub PanelSpecData);

impl std::fmt::Display for PanelSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.id)
    }
}

impl Lifecycle for PanelSpec {
    type Key = String;
    /// The pipeline tracks the last-reconciled spec so reconcile_self can diff
    /// and emit Move/Resize commands only when something actually changed.
    type State = PanelState;
    type Context = ();
    type Output = Sender<PanelCommand>;
    type Error = anyhow::Error;

    fn key(&self) -> String {
        self.0.id.clone()
    }

    fn display_name(&self) -> String {
        self.0.id.clone()
    }

    fn enter(
        self,
        _ctx: &mut (),
        output: &mut Sender<PanelCommand>,
    ) -> Result<PanelState, anyhow::Error> {
        let spec = self.0.clone();
        let phys_w = (spec.width as f32 * spec.dpr).round() as u32;
        let phys_h = (spec.height as f32 * spec.dpr).round() as u32;
        let mut scene = PartialRenderScene::new();
        let frame = PanelFrame {
            pixels: render_frame_partial(&mut scene, &spec.content, phys_w, phys_h, spec.dpr),
            width: phys_w,
            height: phys_h,
        };
        output.send(PanelCommand::Create {
            spec: self.0.clone(),
            frame,
        })?;
        Ok(PanelState {
            spec: self.0,
            scene,
        })
    }

    fn reconcile_self(
        self,
        state: &mut PanelState,
        _ctx: &mut (),
        output: &mut Sender<PanelCommand>,
    ) -> Result<(), anyhow::Error> {
        let new = self.0;
        let phys_w = (new.width as f32 * new.dpr).round() as u32;
        let phys_h = (new.height as f32 * new.dpr).round() as u32;
        let state_phys_w = (state.spec.width as f32 * state.spec.dpr).round() as u32;
        let state_phys_h = (state.spec.height as f32 * state.spec.dpr).round() as u32;
        let phys_dims_changed = phys_w != state_phys_w || phys_h != state_phys_h;
        let pos_changed = new.x != state.spec.x
            || new.y != state.spec.y
            || new.anchor != state.spec.anchor
            || new.output != state.spec.output
            || new.outer_gap != state.spec.outer_gap;
        let render_changed = new.content != state.spec.content || new.dpr != state.spec.dpr;

        if phys_dims_changed {
            let frame = PanelFrame {
                pixels: render_frame_partial(
                    &mut state.scene,
                    &new.content,
                    phys_w,
                    phys_h,
                    new.dpr,
                ),
                width: phys_w,
                height: phys_h,
            };
            output.send(PanelCommand::Resize {
                spec: new.clone(),
                frame,
            })?;
            output.send(PanelCommand::Move(new.clone()))?;
        } else {
            if pos_changed {
                output.send(PanelCommand::Move(new.clone()))?;
            }
            if render_changed {
                let frame = PanelFrame {
                    pixels: render_frame_partial(
                        &mut state.scene,
                        &new.content,
                        phys_w,
                        phys_h,
                        new.dpr,
                    ),
                    width: phys_w,
                    height: phys_h,
                };
                output.send(PanelCommand::UpdatePicture {
                    id: new.id.clone(),
                    frame,
                })?;
            }
        }
        state.spec = new;
        Ok(())
    }

    fn exit(
        state: PanelState,
        _ctx: &mut (),
        output: &mut Sender<PanelCommand>,
    ) -> Result<(), anyhow::Error> {
        let _ = output.send(PanelCommand::Delete { id: state.spec.id });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::PanelSpec;
    use crate::config::FontConfig;
    use crate::layout::PanelSpecData;
    use crate::managed_set::Lifecycle;
    use crate::presentation::PanelCommand;

    fn init_ctx() {
        crate::render::init_global_ctx(FontConfig::default());
    }

    fn make_spec_data(id: &str) -> PanelSpecData {
        PanelSpecData {
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

    #[test]
    fn panel_spec_enter_emits_create_command_and_returns_state() {
        init_ctx();
        let (mut tx, rx) = std::sync::mpsc::channel::<PanelCommand>();
        let spec = PanelSpec(make_spec_data("p1"));
        let state =
            <PanelSpec as Lifecycle>::enter(spec, &mut (), &mut tx).expect("enter should succeed");
        assert_eq!(state.spec.id, "p1", "enter returns the spec data as state");
        let cmds: Vec<PanelCommand> = rx.try_iter().collect();
        assert!(
            matches!(cmds.as_slice(), [PanelCommand::Create { spec: s, .. }] if s.id == "p1"),
            "enter must emit exactly one Create command; got {} commands",
            cmds.len()
        );
    }

    #[test]
    fn panel_spec_reconcile_self_emits_nothing_when_unchanged() {
        let (mut tx, rx) = std::sync::mpsc::channel::<PanelCommand>();
        let mut state = super::PanelState {
            spec: make_spec_data("p1"),
            scene: takumi_incr::PartialRenderScene::new(),
        };
        let spec = PanelSpec(make_spec_data("p1"));
        <PanelSpec as Lifecycle>::reconcile_self(spec, &mut state, &mut (), &mut tx).unwrap();
        let cmds: Vec<PanelCommand> = rx.try_iter().collect();
        assert!(
            cmds.is_empty(),
            "reconcile_self must emit no commands when nothing changed; got {}",
            cmds.len()
        );
    }

    #[test]
    fn panel_spec_reconcile_self_emits_resize_when_dimensions_change() {
        init_ctx();
        let (mut tx, rx) = std::sync::mpsc::channel::<PanelCommand>();
        let mut state = super::PanelState {
            spec: make_spec_data("p1"),
            scene: takumi_incr::PartialRenderScene::new(),
        };
        let mut next = make_spec_data("p1");
        next.width = 200;
        let spec = PanelSpec(next);
        <PanelSpec as Lifecycle>::reconcile_self(spec, &mut state, &mut (), &mut tx).unwrap();
        let cmds: Vec<PanelCommand> = rx.try_iter().collect();
        assert!(
            cmds.iter()
                .any(|c| matches!(c, PanelCommand::Resize { spec: s, .. } if s.id == "p1")),
            "reconcile_self must emit Resize when dimensions change"
        );
        assert!(
            !cmds
                .iter()
                .any(|c| matches!(c, PanelCommand::UpdatePicture { .. })),
            "reconcile_self must NOT emit UpdatePicture when dimensions change"
        );
    }

    #[test]
    fn panel_spec_reconcile_self_emits_move_when_position_changes() {
        let (mut tx, rx) = std::sync::mpsc::channel::<PanelCommand>();
        let mut state = super::PanelState {
            spec: make_spec_data("p1"),
            scene: takumi_incr::PartialRenderScene::new(),
        };
        let mut next = make_spec_data("p1");
        next.x = 50;
        let spec = PanelSpec(next);
        <PanelSpec as Lifecycle>::reconcile_self(spec, &mut state, &mut (), &mut tx).unwrap();
        let cmds: Vec<PanelCommand> = rx.try_iter().collect();
        assert!(
            cmds.iter()
                .any(|c| matches!(c, PanelCommand::Move(s) if s.id == "p1")),
            "reconcile_self must emit Move when position changes"
        );
        assert!(
            !cmds
                .iter()
                .any(|c| matches!(c, PanelCommand::Resize { .. })),
            "reconcile_self must NOT emit Resize when only position changes"
        );
    }

    #[test]
    fn panel_spec_reconcile_self_emits_update_picture_when_only_content_changes() {
        init_ctx();
        let (mut tx, rx) = std::sync::mpsc::channel::<PanelCommand>();
        let mut state = super::PanelState {
            spec: make_spec_data("p1"),
            scene: takumi_incr::PartialRenderScene::new(),
        };
        let mut next = make_spec_data("p1");
        next.content = serde_json::json!({"type": "text", "text": "hello"});
        let spec = PanelSpec(next);
        <PanelSpec as Lifecycle>::reconcile_self(spec, &mut state, &mut (), &mut tx).unwrap();
        let cmds: Vec<PanelCommand> = rx.try_iter().collect();
        assert!(
            cmds.iter()
                .any(|c| matches!(c, PanelCommand::UpdatePicture { id, .. } if id == "p1")),
            "reconcile_self must emit UpdatePicture on content-only change; got {} commands",
            cmds.len()
        );
    }

    #[test]
    fn panel_spec_reconcile_self_emits_resize_not_update_picture_when_dpr_changes_phys_dims() {
        init_ctx();
        // State has dpr=1.0, logical 100x30 → physical 100x30.
        // New spec has dpr=2.0, logical 100x30 → physical 200x60.
        // Physical dims changed, so reconcile_self must emit Resize (not UpdatePicture)
        // and a Move so the presenter can reposition anchored panels.
        let (mut tx, rx) = std::sync::mpsc::channel::<PanelCommand>();
        let mut state = super::PanelState {
            spec: make_spec_data("p1"),
            scene: takumi_incr::PartialRenderScene::new(),
        };
        // state starts with dpr=1.0 (default from make_spec_data)
        assert_eq!(state.spec.dpr, 1.0);
        let mut next = make_spec_data("p1");
        next.dpr = 2.0; // logical dims unchanged, but physical dims double
        let spec = PanelSpec(next);
        <PanelSpec as Lifecycle>::reconcile_self(spec, &mut state, &mut (), &mut tx).unwrap();
        let cmds: Vec<PanelCommand> = rx.try_iter().collect();
        assert!(
            cmds.iter()
                .any(|c| matches!(c, PanelCommand::Resize { spec: s, .. } if s.id == "p1")),
            "reconcile_self must emit Resize when DPR change causes physical dims to change; got {:?} command variants",
            cmds.iter()
                .map(|c| match c {
                    PanelCommand::Create { .. } => "Create",
                    PanelCommand::Move(_) => "Move",
                    PanelCommand::Resize { .. } => "Resize",
                    PanelCommand::Delete { .. } => "Delete",
                    PanelCommand::UpdatePicture { .. } => "UpdatePicture",
                    PanelCommand::Shutdown => "Shutdown",
                })
                .collect::<Vec<_>>()
        );
        assert!(
            !cmds
                .iter()
                .any(|c| matches!(c, PanelCommand::UpdatePicture { .. })),
            "reconcile_self must NOT emit UpdatePicture when physical dims change due to DPR; got {:?} command variants",
            cmds.iter()
                .map(|c| match c {
                    PanelCommand::Create { .. } => "Create",
                    PanelCommand::Move(_) => "Move",
                    PanelCommand::Resize { .. } => "Resize",
                    PanelCommand::Delete { .. } => "Delete",
                    PanelCommand::UpdatePicture { .. } => "UpdatePicture",
                    PanelCommand::Shutdown => "Shutdown",
                })
                .collect::<Vec<_>>()
        );
        assert!(
            cmds.iter()
                .any(|c| matches!(c, PanelCommand::Move(s) if s.id == "p1")),
            "reconcile_self must emit Move after Resize so the presenter can reposition anchored panels; got {:?} command variants",
            cmds.iter()
                .map(|c| match c {
                    PanelCommand::Create { .. } => "Create",
                    PanelCommand::Move(_) => "Move",
                    PanelCommand::Resize { .. } => "Resize",
                    PanelCommand::Delete { .. } => "Delete",
                    PanelCommand::UpdatePicture { .. } => "UpdatePicture",
                    PanelCommand::Shutdown => "Shutdown",
                })
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn panel_spec_exit_emits_delete_with_id() {
        let (mut tx, rx) = std::sync::mpsc::channel::<PanelCommand>();
        let state = super::PanelState {
            spec: make_spec_data("p1"),
            scene: takumi_incr::PartialRenderScene::new(),
        };
        <PanelSpec as Lifecycle>::exit(state, &mut (), &mut tx).unwrap();
        let cmds: Vec<PanelCommand> = rx.try_iter().collect();
        assert!(
            matches!(cmds.as_slice(), [PanelCommand::Delete { id }] if id == "p1"),
            "exit must emit exactly one Delete command carrying the id"
        );
    }
}
