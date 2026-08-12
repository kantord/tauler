use std::sync::mpsc::Sender;
use std::sync::Arc;

use crate::layout::{SurfaceKind, SurfaceSpec};
use crate::managed_set::{Lifecycle, OptativeSet, Reconcile, ReconcileErrors};
use crate::presentation::{SurfaceCommand, SurfaceFrame};
use crate::render::render_frame_keyed;
pub use crate::x11::panel::X11PanelContext;

/// Rasterize a spec's subtree at its physical size.
///
/// Panels sample the wallpaper they cover first, so `backdrop-filter` has real
/// pixels to work on; wallpapers publish theirs afterwards, for the panels above
/// them. See [`crate::backdrop`].
///
/// Returns the backdrop generation the frame was rendered against so the caller
/// can store it: that is the only record of *which* wallpaper these pixels show.
fn render(spec: &SurfaceSpec) -> (SurfaceFrame, u64) {
    let width = (spec.width as f32 * spec.dpr).round() as u32;
    let height = (spec.height as f32 * spec.dpr).round() as u32;
    let backdrop = crate::backdrop::crop_for(spec, (width, height));
    let generation = backdrop.as_ref().map(|b| b.generation).unwrap_or(0);
    let pixels = render_frame_keyed(&spec.content, width, height, spec.dpr, backdrop.as_ref());
    if spec.kind == SurfaceKind::Wallpaper {
        crate::backdrop::publish_wallpaper(spec, Arc::clone(&pixels), width, height);
    }
    (
        SurfaceFrame {
            pixels,
            width,
            height,
        },
        generation,
    )
}

/// What the pipeline remembers about a surface between ticks.
///
/// The spec alone is not enough: a wallpaper moving under an otherwise-unchanged
/// panel changes nothing in that panel's spec, so `backdrop` records which
/// wallpaper frame the last emitted picture actually shows.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceState {
    pub spec: SurfaceSpec,
    backdrop: u64,
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
    type State = SurfaceState;
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
    ) -> Result<SurfaceState, anyhow::Error> {
        let (frame, backdrop) = render(&self.0);
        output.send(match self.0.kind {
            SurfaceKind::Panel => SurfaceCommand::Create {
                spec: self.0.clone(),
                frame,
            },
            SurfaceKind::Wallpaper => SurfaceCommand::PaintWallpaper {
                spec: self.0.clone(),
                frame,
            },
        })?;
        Ok(SurfaceState {
            spec: self.0,
            backdrop,
        })
    }

    fn reconcile_self(
        self,
        state: &mut SurfaceState,
        _ctx: &mut (),
        output: &mut Sender<SurfaceCommand>,
    ) -> Result<(), anyhow::Error> {
        let new = self.0;
        // A wallpaper has no window to move or resize, and re-painting is the
        // same operation as first painting — so any change at all is one command.
        if new.kind == SurfaceKind::Wallpaper {
            if new != state.spec {
                let (frame, backdrop) = render(&new);
                output.send(SurfaceCommand::PaintWallpaper {
                    spec: new.clone(),
                    frame,
                })?;
                state.backdrop = backdrop;
            }
            state.spec = new;
            return Ok(());
        }
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
        // The wallpaper behind this panel is invisible to a spec diff, so ask the
        // registry directly. Wallpapers reconcile before panels (see
        // [`SurfaceSets`]), so this already reflects the current tick.
        let backdrop_changed = crate::backdrop::generation_for(&new) != state.backdrop;
        let render_changed =
            new.content != state.spec.content || new.dpr != state.spec.dpr || backdrop_changed;

        if phys_dims_changed {
            let (frame, backdrop) = render(&new);
            output.send(SurfaceCommand::Resize {
                spec: new.clone(),
                frame,
            })?;
            output.send(SurfaceCommand::Move(new.clone()))?;
            state.backdrop = backdrop;
        } else {
            if pos_changed {
                output.send(SurfaceCommand::Move(new.clone()))?;
            }
            if render_changed {
                let (frame, backdrop) = render(&new);
                output.send(SurfaceCommand::UpdatePicture {
                    id: new.id.clone(),
                    frame,
                })?;
                state.backdrop = backdrop;
            }
        }
        state.spec = new;
        Ok(())
    }

    fn exit(
        state: SurfaceState,
        _ctx: &mut (),
        output: &mut Sender<SurfaceCommand>,
    ) -> Result<(), anyhow::Error> {
        crate::backdrop::forget(&state.spec);
        let _ = output.send(SurfaceCommand::Delete { id: state.spec.id });
        Ok(())
    }
}

/// The surface pipeline's reconciled sets, kept apart so wallpapers always
/// reconcile before the panels that sample them.
///
/// One set cannot express this. `OptativeSet::reconcile` dedups the desired
/// items into a `HashMap` and iterates its keys, so the order specs are handed
/// in is discarded outright; and it runs `update_existing` fully before
/// `enter_new`, so a newly added wallpaper would render after every
/// pre-existing panel however the input was sorted. Two sets make the ordering
/// structural instead of incidental.
#[derive(Default)]
pub struct SurfaceSets {
    wallpapers: OptativeSet<Surface>,
    panels: OptativeSet<Surface>,
}

impl SurfaceSets {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reconcile the whole desired surface set: wallpapers, then panels.
    pub fn reconcile_all(
        &mut self,
        specs: Vec<SurfaceSpec>,
        output: &mut Sender<SurfaceCommand>,
    ) -> ReconcileErrors<String, anyhow::Error> {
        let (wallpapers, panels): (Vec<_>, Vec<_>) = specs
            .into_iter()
            .partition(|s| s.kind == SurfaceKind::Wallpaper);
        let mut errors =
            self.wallpapers
                .reconcile(wallpapers.into_iter().map(Surface), &mut (), output);
        errors.extend(
            self.panels
                .reconcile(panels.into_iter().map(Surface), &mut (), output),
        );
        errors
    }

    /// Tear every surface down. Panels first: they are what the user sees, and a
    /// wallpaper outliving them by a moment is less jarring than the reverse.
    pub fn clear(
        &mut self,
        output: &mut Sender<SurfaceCommand>,
    ) -> ReconcileErrors<String, anyhow::Error> {
        let mut errors = self.panels.reconcile(vec![], &mut (), output);
        errors.extend(self.wallpapers.reconcile(vec![], &mut (), output));
        errors
    }

    /// The last-reconciled spec for `id`, whichever set holds it.
    pub fn spec(&self, id: &str) -> Option<&SurfaceSpec> {
        let key = id.to_string();
        self.panels
            .get(&key)
            .or_else(|| self.wallpapers.get(&key))
            .map(|s| &s.spec)
    }
}

#[cfg(test)]
mod tests {
    use super::{Surface, SurfaceSets, SurfaceState};
    use crate::config::FontConfig;
    use crate::layout::SurfaceSpec;
    use crate::managed_set::Lifecycle;
    use crate::presentation::SurfaceCommand;

    fn init_ctx() {
        crate::render::init_global_ctx(FontConfig::default());
    }

    /// A tracked surface as it looks before any wallpaper exists behind it.
    fn make_state(spec: SurfaceSpec) -> SurfaceState {
        SurfaceState { spec, backdrop: 0 }
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
        assert_eq!(state.spec.id, "p1", "enter returns the spec data as state");
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
        let mut state = make_state(make_spec_data("p1"));
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
        let mut state = make_state(make_spec_data("p1"));
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
        let mut state = make_state(make_spec_data("p1"));
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
        let mut state = make_state(make_spec_data("p1"));
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
        let mut state = make_state(make_spec_data("p1"));
        // state starts with dpr=1.0 (default from make_spec_data)
        assert_eq!(state.spec.dpr, 1.0);
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
        let mut state = make_state(make_wallpaper_data("bg"));
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
        let mut state = make_state(make_wallpaper_data("bg"));
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

    /// A wallpaper appearing under a panel that did not otherwise change must
    /// repaint that panel — and must paint itself *first*, so the panel samples
    /// this tick's wallpaper rather than the previous one or nothing at all.
    ///
    /// Both halves used to be broken: the panel's spec is byte-identical across
    /// the two ticks, so the diff saw no reason to re-render it; and ordering
    /// leant on a `sort_by_key` that `OptativeSet::reconcile` discards.
    #[test]
    fn a_new_wallpaper_paints_itself_then_repaints_an_unchanged_panel() {
        init_ctx();
        let (mut tx, rx) = std::sync::mpsc::channel::<SurfaceCommand>();
        let mut sets = SurfaceSets::new();

        let mut panel = make_spec_data("p1");
        panel.output = Some("ORDER".into());

        // Tick 1: the panel alone, with nothing behind it.
        sets.reconcile_all(vec![panel.clone()], &mut tx);
        let _ = rx.try_iter().count();

        // Tick 2: the same panel, plus a wallpaper underneath it. The panel is
        // listed first on purpose — ordering must not depend on the input order.
        let mut wallpaper = make_wallpaper_data("bg");
        wallpaper.output = Some("ORDER".into());
        wallpaper.width = 100;
        wallpaper.height = 100;
        sets.reconcile_all(vec![panel, wallpaper], &mut tx);

        let cmds: Vec<SurfaceCommand> = rx.try_iter().collect();
        let painted = cmds
            .iter()
            .position(|c| matches!(c, SurfaceCommand::PaintWallpaper { .. }));
        let repainted = cmds
            .iter()
            .position(|c| matches!(c, SurfaceCommand::UpdatePicture { id, .. } if id == "p1"));
        assert!(painted.is_some(), "the new wallpaper must be painted");
        assert!(
            repainted.is_some(),
            "the panel must repaint even though its own spec is unchanged; got {} commands",
            cmds.len()
        );
        assert!(
            painted < repainted,
            "the wallpaper must be painted before the panel that samples it"
        );
    }

    #[test]
    fn panel_spec_exit_emits_delete_with_id() {
        let (mut tx, rx) = std::sync::mpsc::channel::<SurfaceCommand>();
        let state = make_state(make_spec_data("p1"));
        <Surface as Lifecycle>::exit(state, &mut (), &mut tx).unwrap();
        let cmds: Vec<SurfaceCommand> = rx.try_iter().collect();
        assert!(
            matches!(cmds.as_slice(), [SurfaceCommand::Delete { id }] if id == "p1"),
            "exit must emit exactly one Delete command carrying the id"
        );
    }
}
