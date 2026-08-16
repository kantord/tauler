//! The one thread that rasterizes, and how it decides what to draw next.
//!
//! A repaint costs 40–90ms for a full-height panel, and the tick thread is the
//! only thing that reads stream values, reloads the layout file and routes
//! pointer events. Doing that work inline means none of it happens while a panel
//! draws — so repaints become jobs, and the thread that draws them is this one.
//!
//! The scheduler is a slot per render target holding the latest frame nobody has
//! painted yet. Sending a second [`RenderRequest`] for a target overwrites the
//! first, and that overwrite is the entirety of what *superseding* means here:
//! takumi's `render` is one opaque call with no cancellation hook, so a render
//! already under way cannot be stopped — only one that has not started can be
//! replaced. Nothing is ever abandoned mid-draw, which is what makes starvation
//! impossible: an endless stream of updates still draws one snapshot to
//! completion, then the next-newest, and so on (ADR 0023).
//!
//! Everything rasterizes here, including the renders whose caller cannot carry
//! on without the pixels — a window needs a picture before it can exist. Those
//! arrive as [`RenderJob::Now`] and are answered on a reply channel while the
//! tick thread waits, which it did anyway when it drew them itself. One
//! rasterizer means one [`FrameCache`], with no lock around it and nothing else
//! able to evict from it.
//!
//! ## Where the order will come from
//!
//! Targets are drawn in a deterministic but arbitrary order — by id. That holds
//! only while every target is independent. `<BufferBoundary>` (#395) makes a
//! target's pixels an input to its parent's, and then the order stops being
//! arbitrary: a boundary has to be drawn before whatever composites it, exactly
//! as a wallpaper is painted before the panels that sample it. The dependency
//! order belongs here, in the choice of which pending target to draw next, and
//! nowhere else.

use std::collections::{BTreeMap, HashMap};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use super::cache::FrameCache;
use crate::backdrop::Backdrop;
use crate::presentation::{SurfaceCommand, SurfaceFrame};

/// The shortest gap between two repaints of the same target.
///
/// Above roughly 80 frames a second nothing reaches the eye, so a target cheap
/// enough to draw faster than that is only burning CPU. Targets expensive enough
/// to matter (a full-height 4K bar draws in 40–90ms) never come near this floor
/// — drawing is its own rate limit there.
pub const MIN_REPAINT_INTERVAL: Duration = Duration::from_micros(12_500);

/// One target's picture, with everything needed to draw it.
///
/// Self-contained on purpose: the backdrop is cropped by the tick thread and
/// travels with the request, so the worker never consults the wallpaper registry
/// and cannot draw against a wallpaper newer than the one the pipeline recorded.
#[derive(Clone)]
pub struct RenderRequest {
    pub id: String,
    pub content: serde_json::Value,
    /// Physical pixels.
    pub width: u32,
    /// Physical pixels.
    pub height: u32,
    pub dpr: f32,
    pub backdrop: Option<Backdrop>,
}

/// What the pipeline asks the worker for.
pub enum RenderJob {
    /// Draw this when you get to it, and send the pixels straight to the
    /// presenter. Superseded by a newer request for the same target.
    Repaint(RenderRequest),
    /// Draw this before anything pending and hand the frame back: the caller is
    /// blocked on it. Never throttled and never superseded — the pixels are
    /// going somewhere the pipeline needs them synchronously.
    Now {
        request: RenderRequest,
        reply: Sender<SurfaceFrame>,
    },
    /// The fonts were reloaded, so every cached frame was drawn with the wrong
    /// ones.
    FontsChanged,
}

/// What the worker should do with a target it is holding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepaintDecision {
    /// Draw it now.
    RenderNow,
    /// Hold it until this instant. The request is kept, never dropped: this
    /// delays a repaint, it does not cancel one.
    WaitUntil(Instant),
}

/// Decide whether a target may be repainted, given when it last was.
///
/// Pure and clock-free — the caller supplies `now`, as `tauler-i3`'s scheduler
/// does.
pub fn repaint_decision(now: Instant, last_render: Option<Instant>) -> RepaintDecision {
    match last_render {
        Some(last) if now < last + MIN_REPAINT_INTERVAL => {
            RepaintDecision::WaitUntil(last + MIN_REPAINT_INTERVAL)
        }
        _ => RepaintDecision::RenderNow,
    }
}

/// What one pass over the pending targets did.
enum Drawn {
    /// A target was drawn, or there was nothing to draw.
    Done,
    /// Every pending target is inside its interval; the earliest is due then.
    NothingUntil(Instant),
    /// The presenter is gone.
    Disconnected,
}

struct Worker {
    /// The latest unpainted request per target. Inserting over an entry is how a
    /// repaint is superseded — there is no queue for a stale one to sit in.
    ///
    /// Ordered by id, which is arbitrary but stable; see the module docs for
    /// what replaces that.
    pending: BTreeMap<String, RenderRequest>,
    last_render: HashMap<String, Instant>,
    cache: FrameCache,
    commands: Sender<SurfaceCommand>,
}

impl Worker {
    fn new(commands: Sender<SurfaceCommand>) -> Self {
        Worker {
            pending: BTreeMap::new(),
            last_render: HashMap::new(),
            cache: FrameCache::new(),
            commands,
        }
    }

    fn draw(&mut self, request: &RenderRequest) -> SurfaceFrame {
        let pixels = self.cache.frame(request);
        self.last_render.insert(request.id.clone(), Instant::now());
        SurfaceFrame {
            pixels,
            width: request.width,
            height: request.height,
        }
    }

    /// Take one job. Returns false once there is no one left to draw for.
    fn accept(&mut self, job: RenderJob) -> bool {
        match job {
            RenderJob::Repaint(request) => {
                self.pending.insert(request.id.clone(), request);
            }
            RenderJob::Now { request, reply } => {
                // Whatever was pending for this target is older than what is
                // about to be drawn, and the caller is about to put these pixels
                // on screen itself. Dropping it is why a frame for a stale size
                // cannot reach the presenter.
                self.pending.remove(&request.id);
                let frame = self.draw(&request);
                if reply.send(frame).is_err() {
                    tracing::debug!(target = %request.id, "nobody waiting for a Now render");
                }
            }
            RenderJob::FontsChanged => self.cache.clear(),
        }
        true
    }

    /// Draw the first target that is due.
    fn draw_next(&mut self) -> Drawn {
        let now = Instant::now();
        let mut earliest: Option<Instant> = None;
        let mut due: Option<String> = None;
        for id in self.pending.keys() {
            match repaint_decision(now, self.last_render.get(id).copied()) {
                RepaintDecision::RenderNow => {
                    due = Some(id.clone());
                    break;
                }
                RepaintDecision::WaitUntil(t) => {
                    earliest = Some(earliest.map_or(t, |e: Instant| e.min(t)));
                }
            }
        }
        let Some(id) = due else {
            return match earliest {
                Some(t) => Drawn::NothingUntil(t),
                None => Drawn::Done,
            };
        };
        let request = self.pending.remove(&id).expect("id came from pending");
        let frame = self.draw(&request);
        match self
            .commands
            .send(SurfaceCommand::UpdatePicture { id, frame })
        {
            Ok(()) => Drawn::Done,
            Err(_) => Drawn::Disconnected,
        }
    }
}

/// Rasterize until the pipeline goes away.
pub fn run(jobs: Receiver<RenderJob>, commands: Sender<SurfaceCommand>) {
    let mut worker = Worker::new(commands);

    loop {
        // Nothing to draw: wait for something to do rather than spin.
        if worker.pending.is_empty() {
            match jobs.recv() {
                Ok(job) => {
                    worker.accept(job);
                }
                Err(_) => return,
            }
        }
        // Everything else already queued, before choosing — this drain is where
        // a superseded repaint stops existing.
        while let Ok(job) = jobs.try_recv() {
            worker.accept(job);
        }

        match worker.draw_next() {
            Drawn::Done => {}
            Drawn::Disconnected => return,
            // Held by the throttle: wait on the channel rather than sleeping, so
            // a newer request can still supersede what is being held, and a
            // `Now` still gets answered without waiting out the interval.
            Drawn::NothingUntil(deadline) => {
                match jobs.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                    Ok(job) => {
                        worker.accept(job);
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(id: &str, content: &str) -> RenderRequest {
        RenderRequest {
            id: id.to_string(),
            content: serde_json::json!(content),
            width: 4,
            height: 4,
            dpr: 1.0,
            backdrop: None,
        }
    }

    fn worker() -> Worker {
        crate::render::init_global_ctx(crate::config::FontConfig::default());
        let (commands, _rx) = std::sync::mpsc::channel();
        Worker::new(commands)
    }

    #[test]
    fn a_panel_that_has_never_rendered_repaints_immediately() {
        assert_eq!(
            repaint_decision(Instant::now(), None),
            RepaintDecision::RenderNow
        );
    }

    #[test]
    fn a_panel_that_just_rendered_waits_out_the_interval() {
        let base = Instant::now();
        assert_eq!(
            repaint_decision(base + Duration::from_millis(5), Some(base)),
            RepaintDecision::WaitUntil(base + MIN_REPAINT_INTERVAL),
            "a repaint 5ms after the last one must be held, not dropped"
        );
    }

    #[test]
    fn a_panel_repaints_once_the_interval_has_elapsed() {
        let base = Instant::now();
        assert_eq!(
            repaint_decision(base + MIN_REPAINT_INTERVAL, Some(base)),
            RepaintDecision::RenderNow,
            "the deadline itself is eligible"
        );
    }

    /// The slot is the whole scheduler: a target has one unpainted frame, the
    /// newest, and the ones it replaced never existed as far as drawing goes.
    #[test]
    fn a_second_repaint_for_a_target_replaces_the_first() {
        let mut w = worker();
        w.accept(RenderJob::Repaint(request("p1", "first")));
        w.accept(RenderJob::Repaint(request("p1", "second")));
        assert_eq!(w.pending.len(), 1, "one target, one unpainted frame");
        assert_eq!(
            w.pending["p1"].content,
            serde_json::json!("second"),
            "the surviving request must be the newest"
        );
    }

    #[test]
    fn repaints_for_different_targets_all_survive() {
        let mut w = worker();
        w.accept(RenderJob::Repaint(request("p1", "a")));
        w.accept(RenderJob::Repaint(request("p2", "b")));
        w.accept(RenderJob::Repaint(request("p1", "c")));
        assert_eq!(w.pending.len(), 2, "one entry per target");
        assert_eq!(w.pending["p1"].content, serde_json::json!("c"));
        assert_eq!(w.pending["p2"].content, serde_json::json!("b"));
    }

    /// A `Now` render is what a resize does, and it puts newer pixels on screen
    /// than whatever was pending. Keeping the pending one would repaint the
    /// panel with an older picture at a size it no longer has.
    #[test]
    fn a_now_render_drops_what_was_pending_for_that_target() {
        let mut w = worker();
        let (reply, frames) = std::sync::mpsc::channel();
        w.accept(RenderJob::Repaint(request("p1", "stale")));
        w.accept(RenderJob::Repaint(request("p2", "untouched")));
        w.accept(RenderJob::Now {
            request: request("p1", "fresh"),
            reply,
        });
        assert!(
            frames.try_recv().is_ok(),
            "a Now render must answer the caller waiting on it"
        );
        assert!(
            !w.pending.contains_key("p1"),
            "the pending repaint for p1 is older than what was just drawn"
        );
        assert!(
            w.pending.contains_key("p2"),
            "other targets must not be disturbed"
        );
    }

    /// Drawing anything starts that target's interval, so the repaint that
    /// follows a resize is throttled like any other.
    #[test]
    fn drawing_a_target_starts_its_interval() {
        let mut w = worker();
        let (reply, _frames) = std::sync::mpsc::channel();
        w.accept(RenderJob::Now {
            request: request("p1", "fresh"),
            reply,
        });
        assert_eq!(
            repaint_decision(Instant::now(), w.last_render.get("p1").copied()),
            RepaintDecision::WaitUntil(w.last_render["p1"] + MIN_REPAINT_INTERVAL)
        );
    }
}
