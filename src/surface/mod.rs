use std::sync::mpsc::Sender;

use crate::layout::{SurfaceKind, SurfaceSpec};
use crate::managed_set::Lifecycle;
use crate::presentation::{SurfaceCommand, SurfaceFrame};
use crate::render::render_frame;
pub use crate::x11::panel::X11PanelContext;

/// Rasterize a spec's subtree at its physical size.
fn render(spec: &SurfaceSpec) -> SurfaceFrame {
    let width = (spec.width as f32 * spec.dpr).round() as u32;
    let height = (spec.height as f32 * spec.dpr).round() as u32;
    SurfaceFrame {
        pixels: render_frame(&spec.content, width, height, spec.dpr),
        width,
        height,
    }
}

// ---------------------------------------------------------------------------
// Surface — pipeline-side tracker of the desired <panel> / <wallpaper> set.
// Emits typed SurfaceCommand messages on lifecycle transitions; does NOT call
// DisplayManager methods directly. The presenter (src/presentation) applies the
// commands to an actual backend.
// ---------------------------------------------------------------------------

pub struct Surface(pub SurfaceSpec);

impl std::fmt::Display for Surface {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.id)
    }
}

impl Lifecycle for Surface {
    type Key = String;
    /// The pipeline tracks the last-reconciled spec so reconcile_self can diff
    /// and emit Move/Resize commands only when something actually changed.
    type State = SurfaceSpec;
    type Context = ();
    type Output = Sender<SurfaceCommand>;
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
        output: &mut Sender<SurfaceCommand>,
    ) -> Result<SurfaceSpec, anyhow::Error> {
        output.send(match self.0.kind {
            SurfaceKind::Panel => SurfaceCommand::Create {
                spec: self.0.clone(),
                frame: render(&self.0),
            },
            SurfaceKind::Wallpaper => SurfaceCommand::PaintWallpaper {
                spec: self.0.clone(),
                frame: render(&self.0),
            },
        })?;
        Ok(self.0)
    }

    fn reconcile_self(
        self,
        state: &mut SurfaceSpec,
        _ctx: &mut (),
        output: &mut Sender<SurfaceCommand>,
    ) -> Result<(), anyhow::Error> {
        let new = self.0;
        // A wallpaper has no window to move or resize, and re-painting is the
        // same operation as first painting — so any change at all is one command.
        if new.kind == SurfaceKind::Wallpaper {
            if new != *state {
                output.send(SurfaceCommand::PaintWallpaper {
                    spec: new.clone(),
                    frame: render(&new),
                })?;
            }
            *state = new;
            return Ok(());
        }
        let phys_w = (new.width as f32 * new.dpr).round() as u32;
        let phys_h = (new.height as f32 * new.dpr).round() as u32;
        let state_phys_w = (state.width as f32 * state.dpr).round() as u32;
        let state_phys_h = (state.height as f32 * state.dpr).round() as u32;
        let phys_dims_changed = phys_w != state_phys_w || phys_h != state_phys_h;
        let pos_changed = new.x != state.x
            || new.y != state.y
            || new.anchor != state.anchor
            || new.output != state.output
            || new.outer_gap != state.outer_gap;
        let render_changed = new.content != state.content || new.dpr != state.dpr;

        if phys_dims_changed {
            output.send(SurfaceCommand::Resize {
                spec: new.clone(),
                frame: render(&new),
            })?;
            output.send(SurfaceCommand::Move(new.clone()))?;
        } else {
            if pos_changed {
                output.send(SurfaceCommand::Move(new.clone()))?;
            }
            if render_changed {
                output.send(SurfaceCommand::UpdatePicture {
                    id: new.id.clone(),
                    frame: render(&new),
                })?;
            }
        }
        *state = new;
        Ok(())
    }

    fn exit(
        state: SurfaceSpec,
        _ctx: &mut (),
        output: &mut Sender<SurfaceCommand>,
    ) -> Result<(), anyhow::Error> {
        let _ = output.send(SurfaceCommand::Delete { id: state.id });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Surface;
    use crate::config::FontConfig;
    use crate::layout::SurfaceSpec;
    use crate::managed_set::Lifecycle;
    use crate::presentation::SurfaceCommand;

    fn init_ctx() {
        crate::render::init_global_ctx(FontConfig::default());
    }

    fn make_spec_data(id: &str) -> SurfaceSpec {
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

    #[test]
    fn panel_spec_enter_emits_create_command_and_returns_state() {
        init_ctx();
        let (mut tx, rx) = std::sync::mpsc::channel::<SurfaceCommand>();
        let spec = Surface(make_spec_data("p1"));
        let state =
            <Surface as Lifecycle>::enter(spec, &mut (), &mut tx).expect("enter should succeed");
        assert_eq!(state.id, "p1", "enter returns the spec data as state");
        let cmds: Vec<SurfaceCommand> = rx.try_iter().collect();
        assert!(
            matches!(cmds.as_slice(), [SurfaceCommand::Create { spec: s, .. }] if s.id == "p1"),
            "enter must emit exactly one Create command; got {} commands",
            cmds.len()
        );
    }

    #[test]
    fn panel_spec_reconcile_self_emits_nothing_when_unchanged() {
        let (mut tx, rx) = std::sync::mpsc::channel::<SurfaceCommand>();
        let mut state = make_spec_data("p1");
        let spec = Surface(make_spec_data("p1"));
        <Surface as Lifecycle>::reconcile_self(spec, &mut state, &mut (), &mut tx).unwrap();
        let cmds: Vec<SurfaceCommand> = rx.try_iter().collect();
        assert!(
            cmds.is_empty(),
            "reconcile_self must emit no commands when nothing changed; got {}",
            cmds.len()
        );
    }

    #[test]
    fn panel_spec_reconcile_self_emits_resize_when_dimensions_change() {
        init_ctx();
        let (mut tx, rx) = std::sync::mpsc::channel::<SurfaceCommand>();
        let mut state = make_spec_data("p1");
        let mut next = make_spec_data("p1");
        next.width = 200;
        let spec = Surface(next);
        <Surface as Lifecycle>::reconcile_self(spec, &mut state, &mut (), &mut tx).unwrap();
        let cmds: Vec<SurfaceCommand> = rx.try_iter().collect();
        assert!(
            cmds.iter()
                .any(|c| matches!(c, SurfaceCommand::Resize { spec: s, .. } if s.id == "p1")),
            "reconcile_self must emit Resize when dimensions change"
        );
        assert!(
            !cmds
                .iter()
                .any(|c| matches!(c, SurfaceCommand::UpdatePicture { .. })),
            "reconcile_self must NOT emit UpdatePicture when dimensions change"
        );
    }

    #[test]
    fn panel_spec_reconcile_self_emits_move_when_position_changes() {
        let (mut tx, rx) = std::sync::mpsc::channel::<SurfaceCommand>();
        let mut state = make_spec_data("p1");
        let mut next = make_spec_data("p1");
        next.x = 50;
        let spec = Surface(next);
        <Surface as Lifecycle>::reconcile_self(spec, &mut state, &mut (), &mut tx).unwrap();
        let cmds: Vec<SurfaceCommand> = rx.try_iter().collect();
        assert!(
            cmds.iter()
                .any(|c| matches!(c, SurfaceCommand::Move(s) if s.id == "p1")),
            "reconcile_self must emit Move when position changes"
        );
        assert!(
            !cmds
                .iter()
                .any(|c| matches!(c, SurfaceCommand::Resize { .. })),
            "reconcile_self must NOT emit Resize when only position changes"
        );
    }

    #[test]
    fn panel_spec_reconcile_self_emits_update_picture_when_only_content_changes() {
        init_ctx();
        let (mut tx, rx) = std::sync::mpsc::channel::<SurfaceCommand>();
        let mut state = make_spec_data("p1");
        let mut next = make_spec_data("p1");
        next.content = serde_json::json!({"type": "text", "text": "hello"});
        let spec = Surface(next);
        <Surface as Lifecycle>::reconcile_self(spec, &mut state, &mut (), &mut tx).unwrap();
        let cmds: Vec<SurfaceCommand> = rx.try_iter().collect();
        assert!(
            cmds.iter()
                .any(|c| matches!(c, SurfaceCommand::UpdatePicture { id, .. } if id == "p1")),
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
        let (mut tx, rx) = std::sync::mpsc::channel::<SurfaceCommand>();
        let mut state = make_spec_data("p1");
        // state starts with dpr=1.0 (default from make_spec_data)
        assert_eq!(state.dpr, 1.0);
        let mut next = make_spec_data("p1");
        next.dpr = 2.0; // logical dims unchanged, but physical dims double
        let spec = Surface(next);
        <Surface as Lifecycle>::reconcile_self(spec, &mut state, &mut (), &mut tx).unwrap();
        let cmds: Vec<SurfaceCommand> = rx.try_iter().collect();
        assert!(
            cmds.iter()
                .any(|c| matches!(c, SurfaceCommand::Resize { spec: s, .. } if s.id == "p1")),
            "reconcile_self must emit Resize when DPR change causes physical dims to change; got {:?} command variants",
            cmds.iter()
                .map(|c| match c {
                    SurfaceCommand::Create { .. } => "Create",
                    SurfaceCommand::Move(_) => "Move",
                    SurfaceCommand::Resize { .. } => "Resize",
                    SurfaceCommand::Delete { .. } => "Delete",
                    SurfaceCommand::UpdatePicture { .. } => "UpdatePicture",
                    SurfaceCommand::PaintWallpaper { .. } => "PaintWallpaper",
                    SurfaceCommand::Shutdown => "Shutdown",
                })
                .collect::<Vec<_>>()
        );
        assert!(
            !cmds
                .iter()
                .any(|c| matches!(c, SurfaceCommand::UpdatePicture { .. })),
            "reconcile_self must NOT emit UpdatePicture when physical dims change due to DPR; got {:?} command variants",
            cmds.iter()
                .map(|c| match c {
                    SurfaceCommand::Create { .. } => "Create",
                    SurfaceCommand::Move(_) => "Move",
                    SurfaceCommand::Resize { .. } => "Resize",
                    SurfaceCommand::Delete { .. } => "Delete",
                    SurfaceCommand::UpdatePicture { .. } => "UpdatePicture",
                    SurfaceCommand::PaintWallpaper { .. } => "PaintWallpaper",
                    SurfaceCommand::Shutdown => "Shutdown",
                })
                .collect::<Vec<_>>()
        );
        assert!(
            cmds.iter()
                .any(|c| matches!(c, SurfaceCommand::Move(s) if s.id == "p1")),
            "reconcile_self must emit Move after Resize so the presenter can reposition anchored panels; got {:?} command variants",
            cmds.iter()
                .map(|c| match c {
                    SurfaceCommand::Create { .. } => "Create",
                    SurfaceCommand::Move(_) => "Move",
                    SurfaceCommand::Resize { .. } => "Resize",
                    SurfaceCommand::Delete { .. } => "Delete",
                    SurfaceCommand::UpdatePicture { .. } => "UpdatePicture",
                    SurfaceCommand::PaintWallpaper { .. } => "PaintWallpaper",
                    SurfaceCommand::Shutdown => "Shutdown",
                })
                .collect::<Vec<_>>()
        );
    }

    fn make_wallpaper_data(id: &str) -> SurfaceSpec {
        SurfaceSpec {
            kind: crate::layout::SurfaceKind::Wallpaper,
            ..make_spec_data(id)
        }
    }

    /// A wallpaper has no window to move or resize — the backend paints it into
    /// the output's slice of the desktop background. So *any* change (geometry
    /// included) is just a repaint.
    #[test]
    fn wallpaper_spec_reconcile_self_emits_update_picture_when_geometry_changes() {
        init_ctx();
        let (mut tx, rx) = std::sync::mpsc::channel::<SurfaceCommand>();
        let mut state = make_wallpaper_data("bg");
        let mut next = make_wallpaper_data("bg");
        next.width = 200;
        <Surface as Lifecycle>::reconcile_self(Surface(next), &mut state, &mut (), &mut tx)
            .unwrap();
        let cmds: Vec<SurfaceCommand> = rx.try_iter().collect();
        assert!(
            cmds.iter().any(
                |c| matches!(c, SurfaceCommand::PaintWallpaper { spec, .. } if spec.id == "bg")
            ),
            "wallpaper geometry change must emit PaintWallpaper; got {} commands",
            cmds.len()
        );
        assert!(
            !cmds
                .iter()
                .any(|c| matches!(c, SurfaceCommand::Resize { .. } | SurfaceCommand::Move(_))),
            "wallpaper must never emit Resize or Move — it owns no window"
        );
    }

    #[test]
    fn wallpaper_spec_reconcile_self_emits_nothing_when_unchanged() {
        let (mut tx, rx) = std::sync::mpsc::channel::<SurfaceCommand>();
        let mut state = make_wallpaper_data("bg");
        <Surface as Lifecycle>::reconcile_self(
            Surface(make_wallpaper_data("bg")),
            &mut state,
            &mut (),
            &mut tx,
        )
        .unwrap();
        assert!(
            rx.try_iter().next().is_none(),
            "unchanged wallpaper must emit no commands"
        );
    }

    #[test]
    fn panel_spec_exit_emits_delete_with_id() {
        let (mut tx, rx) = std::sync::mpsc::channel::<SurfaceCommand>();
        let state = make_spec_data("p1");
        <Surface as Lifecycle>::exit(state, &mut (), &mut tx).unwrap();
        let cmds: Vec<SurfaceCommand> = rx.try_iter().collect();
        assert!(
            matches!(cmds.as_slice(), [SurfaceCommand::Delete { id }] if id == "p1"),
            "exit must emit exactly one Delete command carrying the id"
        );
    }
}
