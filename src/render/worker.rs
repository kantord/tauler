//! Rasterizing panel repaints off the tick thread.
//!
//! A repaint costs 40–90ms for a full-height panel, and the tick thread is the
//! only thing that reads stream values, reloads the layout file and routes
//! clicks. Doing that work inline means none of it happens while a panel draws.
//!
//! So repaints become requests. The worker drains the whole queue, keeps the
//! newest request per panel and renders those. That is the entirety of what
//! this module calls *superseding* a repaint: takumi's `render` is one opaque
//! call with no cancellation hook, so a render already under way cannot be
//! stopped — only a request that has not started yet can be replaced. Nothing
//! is ever abandoned mid-flight, which is what makes starvation impossible: an
//! endless stream of updates still renders one snapshot to completion, then the
//! next-newest, and so on.
//!
//! Only panel repaints come through here. Create, Resize and wallpaper paints
//! stay on the tick thread — Create needs pixels before the window exists,
//! Resize is what makes a stale-size frame droppable, and a wallpaper must be
//! painted before the panels that sample it are rendered against it.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::backdrop::Backdrop;

/// The shortest gap between two repaints of the same panel.
///
/// Above roughly 80 frames a second nothing reaches the eye, so a panel cheap
/// enough to draw faster than that is only burning CPU. Panels expensive enough
/// to matter (a full-height 4K bar draws in 40–90ms) never come near this floor
/// — the worker is its own rate limit there.
pub const MIN_REPAINT_INTERVAL: Duration = Duration::from_micros(12_500);

/// What the worker should do with a request it is holding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepaintDecision {
    /// Rasterize it now.
    RenderNow,
    /// Hold it until this instant. The request is kept, never dropped: this
    /// delays a repaint, it does not cancel one.
    WaitUntil(Instant),
}

/// Decide whether a panel may be repainted, given when it last was.
///
/// Pure and clock-free — the caller supplies `now`, as
/// `tauler-i3`'s scheduler does.
pub fn repaint_decision(now: Instant, last_render: Option<Instant>) -> RepaintDecision {
    match last_render {
        Some(last) if now < last + MIN_REPAINT_INTERVAL => {
            RepaintDecision::WaitUntil(last + MIN_REPAINT_INTERVAL)
        }
        _ => RepaintDecision::RenderNow,
    }
}

/// One panel's repaint, with everything the rasterizer needs to do it.
///
/// Self-contained on purpose: the backdrop is cropped by the tick thread and
/// travels with the request, so the worker never consults the wallpaper
/// registry and cannot render against a wallpaper newer than the one the
/// pipeline recorded.
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

/// Reduce a drained batch to the newest request per panel.
///
/// Position is first-seen and the payload is last-seen: a panel updating faster
/// than the worker can draw keeps its place in the batch instead of being
/// pushed behind quieter panels every round.
pub fn collapse(requests: Vec<RenderRequest>) -> Vec<RenderRequest> {
    let mut order: Vec<String> = Vec::new();
    let mut newest: HashMap<String, RenderRequest> = HashMap::new();
    for request in requests {
        if !newest.contains_key(&request.id) {
            order.push(request.id.clone());
        }
        newest.insert(request.id.clone(), request);
    }
    order
        .into_iter()
        .filter_map(|id| newest.remove(&id))
        .collect()
}

/// Rasterize repaints until the pipeline goes away.
///
/// Blocks on `requests`, then drains everything else queued behind it before
/// picking what to draw — that drain is where supersession happens, and it is
/// the only place a repaint is ever discarded. A request the throttle is
/// holding stays in `pending` and is drawn when its panel becomes eligible, so
/// a burst that stops mid-interval still lands.
pub fn run(
    requests: std::sync::mpsc::Receiver<RenderRequest>,
    commands: std::sync::mpsc::Sender<crate::presentation::SurfaceCommand>,
) {
    use std::sync::mpsc::RecvTimeoutError;

    let mut pending: Vec<RenderRequest> = Vec::new();
    let mut last_render: HashMap<String, Instant> = HashMap::new();

    loop {
        if pending.is_empty() {
            match requests.recv() {
                Ok(request) => pending.push(request),
                Err(_) => return,
            }
        }
        while let Ok(request) = requests.try_recv() {
            pending.push(request);
        }
        pending = collapse(pending);

        let now = Instant::now();
        let decide =
            |request: &RenderRequest| repaint_decision(now, last_render.get(&request.id).copied());

        match pending
            .iter()
            .position(|r| decide(r) == RepaintDecision::RenderNow)
        {
            Some(index) => {
                let request = pending.remove(index);
                let frame = rasterize(&request);
                last_render.insert(request.id.clone(), Instant::now());
                if commands
                    .send(crate::presentation::SurfaceCommand::UpdatePicture {
                        id: request.id,
                        frame,
                    })
                    .is_err()
                {
                    return;
                }
            }
            // Everything held by the throttle: wait for the nearest deadline,
            // but on the channel rather than sleeping, so a newer request can
            // still supersede what is being held.
            None => {
                let deadline = pending
                    .iter()
                    .filter_map(|r| match decide(r) {
                        RepaintDecision::WaitUntil(t) => Some(t),
                        RepaintDecision::RenderNow => None,
                    })
                    .min()
                    .expect("nothing was eligible, so something has a deadline");
                match requests.recv_timeout(deadline.saturating_duration_since(now)) {
                    Ok(request) => pending.push(request),
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
        }
    }
}

fn rasterize(request: &RenderRequest) -> crate::presentation::SurfaceFrame {
    let pixels = super::render_frame_keyed(
        &request.content,
        request.width,
        request.height,
        request.dpr,
        request.backdrop.as_ref(),
    );
    crate::presentation::SurfaceFrame {
        pixels,
        width: request.width,
        height: request.height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(id: &str, content: &str) -> RenderRequest {
        RenderRequest {
            id: id.to_string(),
            content: serde_json::json!(content),
            width: 100,
            height: 30,
            dpr: 1.0,
            backdrop: None,
        }
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
        let last = base;
        let now = base + Duration::from_millis(5);
        assert_eq!(
            repaint_decision(now, Some(last)),
            RepaintDecision::WaitUntil(last + MIN_REPAINT_INTERVAL),
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
        assert_eq!(
            repaint_decision(base + Duration::from_millis(50), Some(base)),
            RepaintDecision::RenderNow
        );
    }

    #[test]
    fn collapse_keeps_only_the_newest_request_for_a_panel() {
        let out = collapse(vec![
            request("p1", "first"),
            request("p1", "second"),
            request("p1", "third"),
        ]);
        assert_eq!(
            out.len(),
            1,
            "three requests for one panel must collapse to one; got {:?}",
            out.iter().map(|r| &r.content).collect::<Vec<_>>()
        );
        assert_eq!(
            out[0].content,
            serde_json::json!("third"),
            "the surviving request must be the newest"
        );
    }

    #[test]
    fn collapse_keeps_every_panel() {
        let out = collapse(vec![
            request("p1", "a"),
            request("p2", "b"),
            request("p1", "c"),
        ]);
        let ids: Vec<&str> = out.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["p1", "p2"],
            "one entry per panel, in first-seen order"
        );
        assert_eq!(out[0].content, serde_json::json!("c"));
        assert_eq!(out[1].content, serde_json::json!("b"));
    }
}
