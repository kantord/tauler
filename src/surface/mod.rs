use std::sync::mpsc::Sender;
use std::sync::Arc;

use crate::layout::{SurfaceKind, SurfaceSpec};
use crate::managed_set::{Lifecycle, OptativeSet, Reconcile, ReconcileErrors};
use crate::presentation::{SurfaceCommand, SurfaceFrame};
use crate::render::worker::{RenderJob, RenderRequest};
pub use crate::x11::panel::X11PanelContext;

/// Where a reconciled surface sends its work.
///
/// Two channels, not one. Lifecycle commands must reach the presenter in the
/// order the pipeline decided them — a window has to exist before it moves —
/// while drawing is worth up to 90ms and belongs on the worker. Routing the
/// commands through the worker too would put a `Move` behind whatever it happens
/// to be drawing.
pub struct SurfaceOutputs {
    pub commands: Sender<SurfaceCommand>,
    pub jobs: Sender<RenderJob>,
}

impl SurfaceOutputs {
    /// Ask for a repaint and carry on. The pixels reach the presenter without
    /// coming back through here, and a newer request for the same surface
    /// replaces this one if it has not been drawn yet.
    fn repaint(&self, request: RenderRequest) -> Result<(), anyhow::Error> {
        self.jobs
            .send(RenderJob::Repaint(request))
            .map_err(|e| anyhow::anyhow!("render worker is gone: {e}"))
    }

    /// Draw now and wait for the pixels.
    ///
    /// For the frames the pipeline cannot proceed without: a window needs a
    /// picture before it can be created, a resize has to land with one, and a
    /// wallpaper has to be published before the panels that sample it are
    /// cropped against it. Blocking here is what the tick thread did anyway when
    /// it drew these itself — the difference is that now there is one rasterizer
    /// and therefore one cache.
    /// Tell the worker a target is gone, so it stops remembering one.
    fn forget(&self, id: &str) {
        let _ = self.jobs.send(RenderJob::Forget { id: id.to_string() });
    }

    fn render_now(&self, request: RenderRequest) -> Result<SurfaceFrame, anyhow::Error> {
        let (reply, frames) = std::sync::mpsc::channel();
        self.jobs
            .send(RenderJob::Now { request, reply })
            .map_err(|e| anyhow::anyhow!("render worker is gone: {e}"))?;
        frames
            .recv()
            .map_err(|e| anyhow::anyhow!("render worker dropped the job: {e}"))
    }
}

/// The physical size a spec rasterizes at.
fn phys_size(spec: &SurfaceSpec) -> (u32, u32) {
    (
        (spec.width as f32 * spec.dpr).round() as u32,
        (spec.height as f32 * spec.dpr).round() as u32,
    )
}

/// Everything the render worker needs to draw `spec`, and the backdrop
/// generation it will be drawn against.
///
/// The crop happens here, on the tick thread, rather than in the worker: it is
/// what `SurfaceState.backdrop` records, and a worker cropping for itself could
/// draw against a wallpaper newer than the one the pipeline believes it drew
/// against. Cropping costs ~0.5ms uncached and a refcount bump once cached.
fn request_for(spec: &SurfaceSpec) -> (RenderRequest, u64) {
    let (width, height) = phys_size(spec);
    let backdrop = crate::backdrop::crop_for(spec, (width, height));
    let generation = backdrop.as_ref().map(|b| b.generation).unwrap_or(0);
    (
        RenderRequest {
            id: spec.id.clone(),
            content: spec.content.clone(),
            width,
            height,
            dpr: spec.dpr,
            backdrop,
        },
        generation,
    )
}

/// Draw `spec` on the worker and wait for the pixels.
///
/// Panels sample the wallpaper they cover, so a wallpaper publishes its pixels
/// the moment it has them — which is only possible because the caller is
/// blocked here until it does. See [`crate::backdrop`].
///
/// Returns the backdrop generation the frame was drawn against so the caller can
/// store it: that is the only record of *which* wallpaper these pixels show.
fn render_now(
    spec: &SurfaceSpec,
    output: &SurfaceOutputs,
) -> Result<(SurfaceFrame, u64), anyhow::Error> {
    let (request, generation) = request_for(spec);
    let frame = output.render_now(request)?;
    if spec.kind == SurfaceKind::Wallpaper {
        crate::backdrop::publish_wallpaper(
            spec,
            Arc::clone(&frame.pixels),
            frame.width,
            frame.height,
        );
    }
    Ok((frame, generation))
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
    type Output = SurfaceOutputs;
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
        output: &mut SurfaceOutputs,
    ) -> Result<SurfaceState, anyhow::Error> {
        let (frame, backdrop) = render_now(&self.0, output)?;
        output.commands.send(match self.0.kind {
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
        output: &mut SurfaceOutputs,
    ) -> Result<(), anyhow::Error> {
        let new = self.0;
        // A wallpaper has no window to move or resize, and re-painting is the
        // same operation as first painting — so any change at all is one command.
        if new.kind == SurfaceKind::Wallpaper {
            if new != state.spec {
                let (frame, backdrop) = render_now(&new, output)?;
                output.commands.send(SurfaceCommand::PaintWallpaper {
                    spec: new.clone(),
                    frame,
                })?;
                state.backdrop = backdrop;
            }
            state.spec = new;
            return Ok(());
        }
        let (phys_w, phys_h) = phys_size(&new);
        let (state_phys_w, state_phys_h) = phys_size(&state.spec);
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
            let (frame, backdrop) = render_now(&new, output)?;
            output.commands.send(SurfaceCommand::Resize {
                spec: new.clone(),
                frame,
            })?;
            output.commands.send(SurfaceCommand::Move(new.clone()))?;
            state.backdrop = backdrop;
        } else {
            if pos_changed {
                output.commands.send(SurfaceCommand::Move(new.clone()))?;
            }
            if render_changed {
                // Off to the worker: the pixels arrive as an UpdatePicture the
                // worker sends itself. The request is guaranteed to be drawn or
                // replaced by a newer one for this panel, so recording the
                // generation now is not getting ahead of anything.
                let (request, backdrop) = request_for(&new);
                output.repaint(request)?;
                state.backdrop = backdrop;
            }
        }
        state.spec = new;
        Ok(())
    }

    fn exit(
        state: SurfaceState,
        _ctx: &mut (),
        output: &mut SurfaceOutputs,
    ) -> Result<(), anyhow::Error> {
        crate::backdrop::forget(&state.spec);
        output.forget(&state.spec.id);
        let _ = output
            .commands
            .send(SurfaceCommand::Delete { id: state.spec.id });
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
        output: &mut SurfaceOutputs,
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
    pub fn clear(&mut self, output: &mut SurfaceOutputs) -> ReconcileErrors<String, anyhow::Error> {
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

    /// Every panel's current spec, for consumers that iterate the whole set (a11y).
    pub fn panel_specs(&self) -> Vec<SurfaceSpec> {
        self.panels.iter().map(|(_, s)| s.spec.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{Surface, SurfaceOutputs, SurfaceSets, SurfaceState};
    use crate::layout::SurfaceSpec;
    use crate::managed_set::Lifecycle;
    use crate::presentation::{SurfaceCommand, SurfaceFrame};
    use crate::render::worker::{RenderJob, RenderRequest};
    use std::sync::Arc;

    /// Outputs wired to a stand-in for the worker, plus both receiving ends.
    ///
    /// The stand-in answers a `Now` job with a blank frame of the right size and
    /// hands every repaint request to the test instead of drawing it. What these
    /// tests are about is which surface asks for what, so real pixels would only
    /// make them slower and drag the font stack in.
    #[allow(clippy::type_complexity)]
    fn test_outputs() -> (
        SurfaceOutputs,
        std::sync::mpsc::Receiver<SurfaceCommand>,
        std::sync::mpsc::Receiver<RenderRequest>,
    ) {
        let (commands, command_rx) = std::sync::mpsc::channel::<SurfaceCommand>();
        let (jobs, job_rx) = std::sync::mpsc::channel::<RenderJob>();
        let (repaints, repaint_rx) = std::sync::mpsc::channel::<RenderRequest>();
        std::thread::spawn(move || {
            while let Ok(job) = job_rx.recv() {
                match job {
                    RenderJob::Now { request, reply } => {
                        let _ = reply.send(SurfaceFrame {
                            pixels: Arc::new(vec![
                                0u8;
                                (request.width * request.height * 4) as usize
                            ]),
                            width: request.width,
                            height: request.height,
                        });
                    }
                    RenderJob::Repaint(request) => {
                        let _ = repaints.send(request);
                    }
                    RenderJob::Forget { .. } | RenderJob::FontsChanged => {}
                }
            }
        });
        (SurfaceOutputs { commands, jobs }, command_rx, repaint_rx)
    }

    /// The repaints the stand-in has forwarded, waiting briefly for the first.
    ///
    /// Repaints cross a thread on their way to the worker, so `try_iter` alone
    /// races the hop.
    fn repaints_of(rx: &std::sync::mpsc::Receiver<RenderRequest>) -> Vec<RenderRequest> {
        let mut out = Vec::new();
        if let Ok(first) = rx.recv_timeout(std::time::Duration::from_secs(2)) {
            out.push(first);
            out.extend(rx.try_iter());
        }
        out
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
        let (mut tx, rx, _repaints) = test_outputs();
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
        let (mut tx, rx, _repaints) = test_outputs();
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
        let (mut tx, rx, _repaints) = test_outputs();
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
        let (mut tx, rx, _repaints) = test_outputs();
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

    /// A content change is a repaint, and a repaint is the worker's job — the
    /// tick thread must not rasterize it. So the change leaves as a request,
    /// and nothing at all goes to the presenter.
    #[test]
    fn panel_spec_reconcile_self_requests_a_repaint_when_only_content_changes() {
        let (mut tx, rx, repaints) = test_outputs();
        let mut state = make_state(make_spec_data("p1"));
        let mut next = make_spec_data("p1");
        next.content = serde_json::json!("hello");
        let spec = Surface(next);
        <Surface as Lifecycle>::reconcile_self(spec, &mut state, &mut (), &mut tx).unwrap();
        let requests = repaints_of(&repaints);
        assert!(
            matches!(requests.as_slice(), [r] if r.id == "p1" && r.content == serde_json::json!("hello")),
            "a content-only change must produce exactly one repaint request carrying the new content; got {}",
            requests.len()
        );
        let cmds: Vec<SurfaceCommand> = rx.try_iter().collect();
        assert!(
            cmds.is_empty(),
            "the pixels come back from the worker, so the reconciler must send the presenter nothing; got {} commands",
            cmds.len()
        );
    }

    #[test]
    fn panel_spec_reconcile_self_emits_resize_not_update_picture_when_dpr_changes_phys_dims() {
        // State has dpr=1.0, logical 100x30 → physical 100x30.
        // New spec has dpr=2.0, logical 100x30 → physical 200x60.
        // Physical dims changed, so reconcile_self must emit Resize (not UpdatePicture)
        // and a Move so the presenter can reposition anchored panels.
        let (mut tx, rx, _repaints) = test_outputs();
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
        let (mut tx, rx, _repaints) = test_outputs();
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
        let (mut tx, rx, _repaints) = test_outputs();
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
    ///
    /// "First" is no longer a position in one command stream — the repaint
    /// leaves on the worker's channel — so the ordering claim is made where it
    /// actually bites: the request carries the crop of the wallpaper painted
    /// this tick, not the nothing that was there before it.
    #[test]
    fn a_new_wallpaper_paints_itself_then_repaints_an_unchanged_panel() {
        let (mut tx, rx, repaints) = test_outputs();
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
        assert!(
            cmds.iter()
                .any(|c| matches!(c, SurfaceCommand::PaintWallpaper { .. })),
            "the new wallpaper must be painted"
        );
        let requests = repaints_of(&repaints);
        let repaint = requests.iter().find(|r| r.id == "p1");
        assert!(
            repaint.is_some(),
            "the panel must repaint even though its own spec is unchanged; got {} requests",
            requests.len()
        );
        assert!(
            repaint.unwrap().backdrop.is_some(),
            "the repaint must carry this tick's wallpaper crop — a request built \
             before the wallpaper was painted would carry nothing"
        );
    }

    #[test]
    fn panel_spec_exit_emits_delete_with_id() {
        let (mut tx, rx, _repaints) = test_outputs();
        let state = make_state(make_spec_data("p1"));
        <Surface as Lifecycle>::exit(state, &mut (), &mut tx).unwrap();
        let cmds: Vec<SurfaceCommand> = rx.try_iter().collect();
        assert!(
            matches!(cmds.as_slice(), [SurfaceCommand::Delete { id }] if id == "p1"),
            "exit must emit exactly one Delete command carrying the id"
        );
    }
}
