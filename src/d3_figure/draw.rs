//! The Draw runtime: a thread that turns a [`DrawRequest`] into SVG markup by
//! running the matching `<D3Figure>`'s `draw(svg, d3, { name })` in a QuickJS
//! runtime of its own, and sends the result back as [`Drawn`] (ADR 0044).
//!
//! Built the way the reconciler thread is (`units::Reconciler`, ADR 0034): it
//! loads the same layout file and evaluates it with the inputs of the eval that
//! asked ([`crate::jsx::EvalInputs`]: the globals before that render and the
//! Stream values), not with the bar's current ones. Each round
//! evaluates once, with the newest batch's inputs: a pending key is always one
//! the newest batch names, so that eval names every figure still pending. The
//! loop spawns a new one on every layout load and drops the old one, which
//! never waits for it (see [`DrawRuntime`]'s `Drop`).
//!
//! The evaluator ([`crate::jsx::JsxEvaluator::new_draw`]) is built on the first
//! request, and d3 and linkedom are loaded on the first draw, so a layout with no
//! D3Figure never pays for either. With nothing pending the thread sleeps until
//! a request arrives. Each round collects the pending requests, checks once that
//! d3 and linkedom are loaded
//! ([`crate::jsx::JsxEvaluator::ensure_drawing_ready`]), evaluates the layout
//! once, then runs [`crate::jsx::JsxEvaluator::draw_figure`] for each with
//! [`TIME_LIMIT`], stopping early if the runtime was dropped. A drawing goes out
//! on `drawn`, then `notify` is pinged; any failure of one figure is logged and
//! sends nothing, so the figure keeps showing what it showed.
//!
//! A round that draws nothing — d3 and linkedom not downloaded yet
//! ([`DrawError::NotReady`]), or a layout that fails to build or evaluate —
//! keeps its requests, and the next round waits [`NOT_READY_POLL`] however many
//! requests arrive meanwhile: each attempt to load the libraries leaves a module
//! behind in QuickJS, so they are tried once per round, not once per request.
//! What is kept pending is bounded by what is on screen, not by a count: each
//! [`DrawRuntime::request`] carries the keys the loop's latest plan names, and a
//! pending request for any other key is dropped — a figure of live data asks for
//! a new key every Tick, and only its newest key is still on screen. Every
//! request still named is kept, however many figures there are.
//!
//! The thread draws every request it keeps and remembers no drawn keys:
//! [`crate::d3_figure::requests::Requests`] in the loop is the one authority on
//! what needs drawing. A memory here would have to be bounded (live data changes
//! its key every Tick), and a bounded one would skip a key that came back after
//! its Picture was dropped — theme dark → light → dark would leave the figure
//! showing light for good. Requests for the same key still pending are collapsed
//! into one drawing.
//!
//! Evaluated with the same inputs, a deterministic render names the same keys,
//! so a [`DrawError::UnknownKey`] means the render is not deterministic
//! (module-scope state, `Date.now`, `Math.random`) and the figure is never
//! drawn. It is logged at `warn` once per layout load, and at `debug` after
//! that: a key built from `Date.now` or a module-scope counter is new on every
//! eval, so a limit per key would warn on every draw round.

use crate::jsx::{EvalInputs, JsxEvaluator};
use crate::pkg::PackageContext;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

/// How long one `draw` may run before it is interrupted.
///
/// It exists to stop runaway code, not slow drawings — a `draw` runs off the
/// loop. A real scene (the Venice map in `examples/weather-map`) takes 1.73 s.
pub const TIME_LIMIT: Duration = Duration::from_secs(5);

/// How long a round that drew nothing (d3 not downloaded yet, a broken layout)
/// holds back the next one, however many requests arrive meanwhile.
pub const NOT_READY_POLL: Duration = Duration::from_millis(500);

/// Draw the D3Figure with this `key` at this size.
#[derive(Debug, Clone, PartialEq)]
pub struct DrawRequest {
    pub key: String,
    pub width: f64,
    pub height: f64,
}

/// A finished drawing: the `<svg>` markup `draw` produced for `key`.
#[derive(Debug, Clone, PartialEq)]
pub struct Drawn {
    /// The generation of the Draw runtime that drew it ([`DrawRuntime::spawn`]).
    pub generation: u64,
    pub key: String,
    pub width: f64,
    pub height: f64,
    pub svg: String,
}

/// Why [`crate::jsx::JsxEvaluator::draw_figure`] produced no markup.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DrawError {
    /// d3 or linkedom is not downloaded yet (the Fetcher is cold), or the
    /// runtime has no Fetcher at all.
    #[error("d3 and linkedom are not available yet")]
    NotReady,
    /// The latest eval rendered no D3Figure with this key.
    #[error("no <D3Figure> with key {0:?} in the latest eval")]
    UnknownKey(String),
    /// `draw` threw; the thrown message.
    #[error("draw threw: {0}")]
    Threw(String),
    /// `draw` ran past its time limit and was interrupted.
    #[error("draw ran past its time limit and was interrupted")]
    TimedOut,
}

/// One [`DrawRuntime::request`]: the drawings asked for, every key on screen
/// at the time, and the inputs of the eval that asked.
struct Batch {
    named: HashSet<String>,
    draw: Vec<DrawRequest>,
    inputs: Arc<EvalInputs>,
}

/// The draw runtime's thread. Dropping this stops the thread.
pub struct DrawRuntime {
    /// `None` only during `drop`: closing the channel is what wakes an idle
    /// thread at once.
    requests: Option<mpsc::Sender<Batch>>,
    stop: Arc<AtomicBool>,
}

impl DrawRuntime {
    /// Start the draw runtime's thread for the layout `source`.
    ///
    /// Each drawing is sent on `drawn`, tagged with `generation`, then
    /// `notify` is pinged (never blocking: a full notifier already means "look
    /// again"). The loop gives each runtime a new `generation` and keeps only
    /// the current one's drawings ([`current_only`]).
    #[allow(clippy::too_many_arguments)]
    pub fn spawn(
        source: String,
        ctx: serde_json::Value,
        layout_path: Option<PathBuf>,
        pkg_ctx: Option<PackageContext>,
        drawn: mpsc::Sender<Drawn>,
        notify: mpsc::SyncSender<()>,
        generation: u64,
    ) -> Self {
        let (requests, request_rx) = mpsc::channel::<Batch>();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = Arc::clone(&stop);
        std::thread::Builder::new()
            .name("tauler-draw".into())
            .spawn(move || {
                let build = || {
                    JsxEvaluator::new_draw(
                        &source,
                        ctx.clone(),
                        layout_path.as_deref(),
                        pkg_ctx.clone(),
                    )
                };
                run(
                    build,
                    &request_rx,
                    &stop_thread,
                    &drawn,
                    &notify,
                    generation,
                );
            })
            .expect("spawning the draw runtime thread");
        Self {
            requests: Some(requests),
            stop,
        }
    }

    /// Ask for the drawings in `draw`. `named` is every key on screen now (the
    /// loop's [`crate::d3_figure::requests::Requests::named`], which includes
    /// `draw`'s keys): a pending request for any other key is superseded and
    /// dropped. `inputs` are those of the eval that asked. Never blocks.
    pub fn request(&self, named: HashSet<String>, draw: Vec<DrawRequest>, inputs: Arc<EvalInputs>) {
        if let Some(requests) = &self.requests {
            let _ = requests.send(Batch {
                named,
                draw,
                inputs,
            });
        }
    }
}

/// What the thread waits for before its next round.
enum Wake {
    Request(Batch),
    RoundDue,
    Closed,
}

/// Blocks while nothing is pending, so an idle thread never wakes; while a
/// round is held back until `retry_at`, waits for a request or that moment.
fn wait(
    requests: &mpsc::Receiver<Batch>,
    pending: &[DrawRequest],
    retry_at: Option<Instant>,
) -> Wake {
    if pending.is_empty() {
        return requests.recv().map_or(Wake::Closed, Wake::Request);
    }
    let Some(at) = retry_at else {
        return Wake::RoundDue;
    };
    match requests.recv_timeout(at.saturating_duration_since(Instant::now())) {
        Ok(batch) => Wake::Request(batch),
        Err(mpsc::RecvTimeoutError::Timeout) => Wake::RoundDue,
        Err(mpsc::RecvTimeoutError::Disconnected) => Wake::Closed,
    }
}

/// What the thread has collected from the batches so far.
#[derive(Default)]
struct Queue {
    pending: Vec<DrawRequest>,
    /// The newest batch's inputs: its named keys include every pending one.
    latest: Option<Arc<EvalInputs>>,
}

impl Queue {
    /// Drops the pending requests that `batch` no longer names, then adds its requests as the newest, each replacing a pending
    /// one for the same key, and makes its inputs the `latest`.
    fn keep(&mut self, batch: Batch) {
        self.latest = Some(batch.inputs);
        self.pending.retain(|p| batch.named.contains(&p.key));
        for request in batch.draw {
            self.pending.retain(|p| p.key != request.key);
            self.pending.push(request);
        }
    }

    /// [`Self::keep`]s every batch already waiting, without blocking.
    fn keep_waiting(&mut self, requests: &mpsc::Receiver<Batch>) {
        while let Ok(batch) = requests.try_recv() {
            self.keep(batch);
        }
    }
}

/// The thread's loop: collect requests, and draw whatever is pending.
fn run(
    build: impl Fn() -> Result<JsxEvaluator, crate::jsx::JsError>,
    requests: &mpsc::Receiver<Batch>,
    stop: &AtomicBool,
    drawn: &mpsc::Sender<Drawn>,
    notify: &mpsc::SyncSender<()>,
    generation: u64,
) {
    let mut evaluator: Option<JsxEvaluator> = None;
    let mut queue = Queue::default();
    // A runtime lives for one layout load: an UnknownKey warns once per load.
    let mut warned_unknown_key = false;
    // Set when a round drew nothing: the next one waits for it.
    let mut retry_at: Option<Instant> = None;
    // So a broken layout is reported once, not on every poll.
    let mut reported_error: Option<String> = None;

    while !stop.load(Ordering::Relaxed) {
        match wait(requests, &queue.pending, retry_at) {
            Wake::Closed => return,
            Wake::Request(batch) => {
                queue.keep(batch);
                if retry_at.is_some_and(|at| Instant::now() < at) {
                    continue;
                }
            }
            Wake::RoundDue => {}
        }
        queue.keep_waiting(requests);
        if queue.pending.is_empty() {
            continue;
        }
        retry_at = Some(Instant::now() + NOT_READY_POLL);
        if evaluator.is_none() {
            match build() {
                Ok(built) => evaluator = Some(built),
                Err(e) => {
                    report_once(&mut reported_error, "the draw runtime failed to start", &e);
                    continue;
                }
            }
        }
        let Some(evaluator) = evaluator.as_ref() else {
            continue;
        };
        match evaluator.ensure_drawing_ready() {
            Ok(()) => {}
            Err(DrawError::NotReady) => continue,
            Err(e) => {
                tracing::warn!(error = %e, "d3 and linkedom failed to load; no D3Figure was drawn");
                queue.pending.clear();
                continue;
            }
        }
        // Building the evaluator and loading d3 can be slow: draw what is asked
        // for now, with the newest inputs, not what was asked for before.
        queue.keep_waiting(requests);
        let Some(inputs) = queue.latest.as_deref() else {
            continue;
        };
        if let Err(e) = evaluator.eval_inputs(inputs) {
            report_once(&mut reported_error, "the draw runtime's eval failed", &e);
            continue;
        }
        reported_error = None;
        retry_at = None;
        for request in std::mem::take(&mut queue.pending) {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            draw_one(
                evaluator,
                &request,
                &mut warned_unknown_key,
                drawn,
                notify,
                generation,
            );
        }
    }
}

/// Draws `request` and sends the result, or logs why there is none.
fn draw_one(
    evaluator: &JsxEvaluator,
    request: &DrawRequest,
    warned_unknown_key: &mut bool,
    drawn: &mpsc::Sender<Drawn>,
    notify: &mpsc::SyncSender<()>,
    generation: u64,
) {
    match evaluator.draw_figure(&request.key, request.width, request.height, TIME_LIMIT) {
        Ok(svg) => {
            let _ = drawn.send(Drawn {
                generation,
                key: request.key.clone(),
                width: request.width,
                height: request.height,
                svg,
            });
            let _ = notify.try_send(());
        }
        // The eval ran with the inputs of the eval that asked, so the same tree should name
        // the same key: a key it does not name means the render is nondeterministic
        // (module-scope state, Date.now, Math.random), and the figure will not be drawn.
        Err(e @ DrawError::UnknownKey(_)) => {
            if std::mem::replace(warned_unknown_key, true) {
                tracing::debug!(key = %request.key, error = %e, "the D3Figure was not drawn");
            } else {
                tracing::warn!(
                    key = %request.key,
                    error = %e,
                    "the D3Figure was not drawn: the layout rendered a different tree from the \
                     same inputs; is its render nondeterministic (module-scope state, Date.now, \
                     Math.random)?"
                );
            }
        }
        Err(e) => {
            tracing::warn!(key = %request.key, error = %e, "the D3Figure was not drawn");
        }
    }
}

/// The `drawings` drawn by the Draw runtime of `generation`: a dropped runtime
/// may still deliver its last drawing after a reload ([`DrawRuntime`]'s
/// `Drop`), and it must not overwrite the current runtime's drawing of the same
/// key, which may come from a changed `draw`.
pub fn current_only(generation: u64, drawings: impl IntoIterator<Item = Drawn>) -> Vec<Drawn> {
    drawings
        .into_iter()
        .filter(|d| d.generation == generation)
        .collect()
}

/// Logs `error` unless it is the one logged last.
fn report_once(last: &mut Option<String>, what: &str, error: &crate::jsx::JsError) {
    let message = error.to_string();
    if last.as_deref() != Some(message.as_str()) {
        tracing::warn!(error = %message, "{what}");
        *last = Some(message);
    }
}

impl Drop for DrawRuntime {
    /// Runs on the loop thread, on every layout reload, so it never waits: it
    /// raises the flag, closes the channel (which wakes a thread blocked in
    /// `recv`) and lets the thread go without joining it. A draw already
    /// running finishes on its own, within [`TIME_LIMIT`]; the thread then sees
    /// the flag and exits without starting another.
    ///
    /// Not joining is safe because the thread owns everything it touches (its
    /// evaluator, clones of the shared handles) and shares nothing that dies
    /// with this value. The one drawing it may still send lands on the loop's
    /// `drawn` channel after the reload, tagged with this runtime's generation,
    /// and the loop ignores it ([`current_only`]): its `draw` may be the one
    /// the reload replaced.
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.requests = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::d3_figure::test_support::{d3_only_pkg_ctx, fake_d3_fetcher, write_fake_files};
    use std::collections::HashMap;

    const DEADLINE: Duration = Duration::from_secs(5);

    /// The generation the tests' runtimes are spawned with.
    const GENERATION: u64 = 7;

    const CIRCLE_LAYOUT: &str = r#"export default function render() {
return <div><D3Figure key="k" width={10} height={20}
  draw={(svg) => svg.append("circle").attr("r", 3)} /></div>;
}"#;

    struct Running {
        runtime: DrawRuntime,
        drawn: mpsc::Receiver<Drawn>,
        notified: mpsc::Receiver<()>,
    }

    /// A draw runtime over [`CIRCLE_LAYOUT`] whose Fetcher reads `d3_dir`.
    fn spawn_over(dir: &std::path::Path, d3_dir: &std::path::Path) -> Running {
        spawn_layout(CIRCLE_LAYOUT, dir, d3_dir)
    }

    /// A draw runtime over `layout` whose Fetcher reads `d3_dir`.
    fn spawn_layout(layout: &str, dir: &std::path::Path, d3_dir: &std::path::Path) -> Running {
        let (drawn_tx, drawn) = mpsc::channel();
        let (notify_tx, notified) = mpsc::sync_channel(1);
        let runtime = DrawRuntime::spawn(
            layout.to_string(),
            serde_json::Value::Null,
            Some(dir.to_path_buf()),
            Some(d3_only_pkg_ctx(dir, fake_d3_fetcher(d3_dir))),
            drawn_tx,
            notify_tx,
            GENERATION,
        );
        Running {
            runtime,
            drawn,
            notified,
        }
    }

    fn circle_request() -> DrawRequest {
        DrawRequest {
            key: "k@10x20".into(),
            width: 10.0,
            height: 20.0,
        }
    }

    #[test]
    fn a_requested_figure_arrives_on_the_drawn_channel_and_pings_the_notifier() {
        let dir = tempfile::tempdir().expect("tempdir");
        let d3_dir = dir.path().join("d3");
        write_fake_files(&d3_dir);
        let running = spawn_over(dir.path(), &d3_dir);

        ask(&running.runtime, vec![circle_request()]);

        let drawn = running
            .drawn
            .recv_timeout(DEADLINE)
            .expect("the requested figure should be drawn");
        assert_eq!(drawn.key, "k@10x20");
        assert_eq!(
            drawn.generation, GENERATION,
            "a drawing carries the generation its runtime was spawned with"
        );
        assert_eq!(drawn.width, 10.0);
        assert_eq!(drawn.height, 20.0);
        assert!(drawn.svg.contains("<svg"), "got: {}", drawn.svg);
        assert!(drawn.svg.contains(r#"<circle r="3""#), "got: {}", drawn.svg);
        running
            .notified
            .recv_timeout(DEADLINE)
            .expect("the notifier should be pinged after a drawing is sent");
    }

    #[test]
    fn a_request_made_before_d3_is_downloaded_is_drawn_once_the_files_appear() {
        let dir = tempfile::tempdir().expect("tempdir");
        let d3_dir = dir.path().join("d3");
        std::fs::create_dir_all(&d3_dir).expect("create empty d3 dir");
        let running = spawn_over(dir.path(), &d3_dir);

        ask(&running.runtime, vec![circle_request()]);
        assert!(
            running
                .drawn
                .recv_timeout(Duration::from_millis(300))
                .is_err(),
            "nothing can be drawn before d3 and linkedom are here"
        );

        write_fake_files(&d3_dir);

        let drawn = running
            .drawn
            .recv_timeout(DEADLINE)
            .expect("the kept request should be drawn once the files appear");
        assert_eq!(drawn.key, "k@10x20");
        assert!(drawn.svg.contains(r#"<circle r="3""#), "got: {}", drawn.svg);
    }

    #[test]
    fn dropping_the_draw_runtime_stops_its_thread() {
        let dir = tempfile::tempdir().expect("tempdir");
        let d3_dir = dir.path().join("d3");
        std::fs::create_dir_all(&d3_dir).expect("create empty d3 dir");
        // Cold, so the thread is busy keeping and retrying the request.
        let running = spawn_over(dir.path(), &d3_dir);
        ask(&running.runtime, vec![circle_request()]);
        std::thread::sleep(Duration::from_millis(100));

        let (dropped_tx, dropped_rx) = mpsc::channel();
        let started = Instant::now();
        std::thread::spawn(move || {
            drop(running);
            let _ = dropped_tx.send(());
        });

        dropped_rx
            .recv_timeout(DEADLINE)
            .expect("dropping the draw runtime must stop and join its thread");
        // The loop thread drops this on every layout reload: an idle thread
        // must not make it wait out a poll interval.
        assert!(
            started.elapsed() < Duration::from_millis(100),
            "drop took {:?}",
            started.elapsed()
        );
    }

    /// `n` figures `k0`..`k{n-1}` whose `draw` never returns, so each runs
    /// until [`TIME_LIMIT`] interrupts it.
    fn looping_layout(n: usize) -> String {
        format!(
            r#"export default function render() {{
return <div>{{Array.from({{ length: {n} }}, (_, i) =>
  <D3Figure key={{"k" + i}} width={{10}} height={{20}} draw={{() => {{ for (;;) {{}} }}}} />)}}</div>;
}}"#
        )
    }

    /// `n` figures `k0`..`k{n-1}` that each draw a circle.
    fn circles_layout(n: usize) -> String {
        format!(
            r#"export default function render() {{
return <div>{{Array.from({{ length: {n} }}, (_, i) =>
  <D3Figure key={{"k" + i}} width={{10}} height={{20}}
    draw={{(svg) => svg.append("circle").attr("r", 3)}} />)}}</div>;
}}"#
        )
    }

    /// Asks for `draw`, naming exactly its keys as the ones on screen.
    fn ask(runtime: &DrawRuntime, draw: Vec<DrawRequest>) {
        let named = draw.iter().map(|r| r.key.clone()).collect();
        // These layouts read neither streams nor `globals`.
        runtime.request(named, draw, Default::default());
    }

    /// Asks for figures `k{i}` for every `i` in `range`, all of them on screen.
    fn ask_keyed(runtime: &DrawRuntime, range: std::ops::Range<usize>) {
        ask(runtime, range.map(keyed_request).collect());
    }

    fn keyed_request(i: usize) -> DrawRequest {
        DrawRequest {
            key: format!("k{i}@10x20"),
            width: 10.0,
            height: 20.0,
        }
    }

    const SLOW_FIGURES: usize = 5;

    /// A ready runtime over [`looping_layout`] that has been asked for every
    /// figure and is inside the first figure's `draw`.
    fn running_slow_figures(dir: &std::path::Path) -> Running {
        let d3_dir = dir.join("d3");
        write_fake_files(&d3_dir);
        let running = spawn_layout(&looping_layout(SLOW_FIGURES), dir, &d3_dir);
        ask_keyed(&running.runtime, 0..SLOW_FIGURES);
        std::thread::sleep(Duration::from_millis(300));
        running
    }

    #[test]
    fn dropping_the_draw_runtime_does_not_wait_for_a_running_draw() {
        let dir = tempfile::tempdir().expect("tempdir");
        let running = running_slow_figures(dir.path());

        let started = Instant::now();
        drop(running.runtime);

        // The loop thread drops this on every layout reload.
        assert!(
            started.elapsed() < Duration::from_millis(100),
            "drop took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn a_dropped_draw_runtime_abandons_the_rest_of_its_round() {
        let dir = tempfile::tempdir().expect("tempdir");
        let running = running_slow_figures(dir.path());

        let runtime = running.runtime;
        // On its own thread, so this measures the draw thread, not `drop`.
        std::thread::spawn(move || drop(runtime));

        // The thread holds the only `drawn` sender: the channel closes when the
        // thread exits, which must be after the running draw, not after all
        // five.
        let result = running
            .drawn
            .recv_timeout(TIME_LIMIT + Duration::from_secs(1));
        assert_eq!(result, Err(mpsc::RecvTimeoutError::Disconnected));
    }

    #[test]
    fn while_d3_is_not_ready_the_libraries_are_tried_once_per_poll_not_per_request() {
        let dir = tempfile::tempdir().expect("tempdir");
        let d3_dir = dir.path().join("d3");
        std::fs::create_dir_all(&d3_dir).expect("create empty d3 dir");
        let fetcher = Arc::new(fake_d3_fetcher(&d3_dir));
        let mut pkg_ctx = d3_only_pkg_ctx(dir.path(), fake_d3_fetcher(&d3_dir));
        pkg_ctx.d3 = Arc::clone(&fetcher);
        let (drawn_tx, _drawn) = mpsc::channel();
        let (notify_tx, _notified) = mpsc::sync_channel(1);
        let runtime = DrawRuntime::spawn(
            circles_layout(1),
            serde_json::Value::Null,
            Some(dir.path().to_path_buf()),
            Some(pkg_ctx),
            drawn_tx,
            notify_tx,
            GENERATION,
        );

        // 200 requests over ~1.2 s on Linux: about three polls' worth. Sleeps are
        // coarser elsewhere (macOS ran this loop for much longer), so the bound
        // below comes from the time it actually took.
        let requests = 200;
        let started = Instant::now();
        for i in 0..requests {
            ask(&runtime, vec![keyed_request(i)]);
            std::thread::sleep(Duration::from_millis(6));
        }

        // Each attempt to load d3 asks the Fetcher once: the bootstrap's first
        // import fails to resolve, which ends the attempt. Read before the clock,
        // so no attempt is counted that the elapsed time does not cover.
        let attempts = fetcher.sources_calls();
        let elapsed = started.elapsed();
        // One attempt right away, then at most one per poll; +1 of slack for a
        // poll that lands just as the clock is read.
        let polls = elapsed.as_millis().div_ceil(NOT_READY_POLL.as_millis()) as usize;
        assert!(
            attempts <= polls + 2,
            "{attempts} attempts to load d3 in {elapsed:?}: more than one per {NOT_READY_POLL:?} poll"
        );
        // What the bug looked like: one attempt per request.
        assert!(
            attempts * 10 < requests,
            "{attempts} attempts for {requests} requests"
        );
    }

    #[test]
    fn pending_requests_for_keys_no_longer_named_are_dropped() {
        let figures = 200;
        let dir = tempfile::tempdir().expect("tempdir");
        let d3_dir = dir.path().join("d3");
        std::fs::create_dir_all(&d3_dir).expect("create empty d3 dir");
        let running = spawn_layout(&circles_layout(figures), dir.path(), &d3_dir);
        // Every figure's key changed: the first hundred keys left the screen.
        ask_keyed(&running.runtime, 0..100);
        ask_keyed(&running.runtime, 100..figures);

        write_fake_files(&d3_dir);

        let still_named: Vec<String> = (100..figures).map(|i| format!("k{i}@10x20")).collect();
        assert_eq!(
            drain_keys(&running.drawn),
            still_named,
            "superseded keys are dropped; every key still named is drawn, however many"
        );
    }

    /// Every drawing that arrives until the channel stays quiet for a second.
    fn drain_keys(drawn: &mpsc::Receiver<Drawn>) -> Vec<String> {
        let mut keys = vec![
            drawn
                .recv_timeout(DEADLINE)
                .expect("the kept requests should be drawn once the files appear")
                .key,
        ];
        while let Ok(d) = drawn.recv_timeout(Duration::from_secs(1)) {
            keys.push(d.key);
        }
        keys
    }

    #[test]
    fn a_hundred_figures_named_at_once_are_all_drawn_once_the_files_appear() {
        let figures = 100;
        let dir = tempfile::tempdir().expect("tempdir");
        let d3_dir = dir.path().join("d3");
        std::fs::create_dir_all(&d3_dir).expect("create empty d3 dir");
        let running = spawn_layout(&circles_layout(figures), dir.path(), &d3_dir);
        ask_keyed(&running.runtime, 0..figures);

        write_fake_files(&d3_dir);

        let all: Vec<String> = (0..figures).map(|i| format!("k{i}@10x20")).collect();
        assert_eq!(
            drain_keys(&running.drawn),
            all,
            "every figure is still on screen, so none may be dropped"
        );
    }

    fn drawn(generation: u64, key: &str) -> Drawn {
        Drawn {
            generation,
            key: key.into(),
            width: 10.0,
            height: 20.0,
            svg: "<svg/>".into(),
        }
    }

    #[test]
    fn only_drawings_of_the_current_generation_are_accepted() {
        let accepted = current_only(
            2,
            [
                drawn(1, "k"),
                drawn(2, "k"),
                drawn(1, "old"),
                drawn(2, "new"),
            ],
        );

        assert_eq!(
            accepted,
            [drawn(2, "k"), drawn(2, "new")],
            "a dropped runtime's late drawing must not overwrite the current runtime's"
        );
    }

    /// Asks for `out`'s figures the way the loop does (`Figures::request` in
    /// `app.rs`): plan them, then request them with `out`'s inputs.
    fn ask_from(
        runtime: &DrawRuntime,
        requests: &mut crate::d3_figure::requests::Requests,
        out: &crate::jsx::EvalOutput,
    ) {
        let plan = requests.plan(&out.figure_calls);
        runtime.request(requests.named(), plan.draw, Arc::clone(&out.inputs));
    }

    /// The render evaluator over `layout`, as the bar builds it.
    fn render_evaluator(layout: &str, dir: &std::path::Path) -> JsxEvaluator {
        JsxEvaluator::new(layout, serde_json::Value::Null, Some(dir))
            .expect("the render evaluator builds")
    }

    #[test]
    fn a_render_that_writes_globals_still_draws() {
        let layout = r#"export default function render() {
globals.n = (globals.n ?? 0) + 1;
return <div><D3Figure key={"g" + globals.n} width={10} height={20}
  draw={(svg) => svg.append("circle").attr("r", 3)} /></div>;
}"#;
        let dir = tempfile::tempdir().expect("tempdir");
        let d3_dir = dir.path().join("d3");
        write_fake_files(&d3_dir);
        let running = spawn_layout(layout, dir.path(), &d3_dir);
        let render = render_evaluator(layout, dir.path());
        let mut requests = crate::d3_figure::requests::Requests::new();

        for _ in 0..2 {
            let out = render.eval(&HashMap::new()).expect("render eval");
            ask_from(&running.runtime, &mut requests, &out);
        }

        let deadline = Instant::now() + DEADLINE;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let drawn = running.drawn.recv_timeout(left).expect(
                "the second eval's figure should be drawn: the draw runtime evaluates \
                 with the globals that eval started from, not the ones it left behind",
            );
            if drawn.key == "g2@10x20" {
                break;
            }
        }
    }

    #[test]
    fn a_stream_faster_than_a_draw_round_draws_the_latest_batch() {
        let layout = r#"export default function render() {
return <div><D3Figure key={"v" + useStringStream("/bin/sh", "v")} width={10} height={20}
  draw={(svg) => svg.append("circle").attr("r", 3)} /></div>;
}"#;
        let dir = tempfile::tempdir().expect("tempdir");
        let d3_dir = dir.path().join("d3");
        std::fs::create_dir_all(&d3_dir).expect("create empty d3 dir");
        let running = spawn_layout(layout, dir.path(), &d3_dir);
        let render = render_evaluator(layout, dir.path());
        let mut requests = crate::d3_figure::requests::Requests::new();
        let stream = ("/bin/sh".to_string(), Some("v".to_string()));

        // Twenty Ticks while the runtime cannot draw yet.
        for v in 1..=20 {
            let values = HashMap::from([(stream.clone(), v.to_string())]);
            let out = render.eval(&values).expect("render eval");
            ask_from(&running.runtime, &mut requests, &out);
        }

        write_fake_files(&d3_dir);

        assert_eq!(
            drain_keys(&running.drawn),
            ["v20@10x20"],
            "the newest key is drawn with its batch's inputs; superseded keys are not"
        );
    }

    /// Many distinct keys, as a figure of live data produces over time.
    const MANY: usize = 300;

    #[test]
    fn a_key_requested_again_is_drawn_again() {
        let layout = format!(
            r#"export default function render() {{
return <div>{{Array.from({{ length: {MANY} }}, (_, i) =>
  <D3Figure key={{"k" + i}} width={{10}} height={{20}}
    draw={{(svg) => svg.append("circle").attr("r", 3)}} />)}}</div>;
}}"#
        );
        let dir = tempfile::tempdir().expect("tempdir");
        let d3_dir = dir.path().join("d3");
        write_fake_files(&d3_dir);
        let running = spawn_layout(&layout, dir.path(), &d3_dir);
        ask_keyed(&running.runtime, 0..MANY);
        for _ in 0..MANY {
            running
                .drawn
                .recv_timeout(DEADLINE)
                .expect("precondition: every distinct key is drawn once");
        }

        ask_keyed(&running.runtime, 0..1);
        let again = running.drawn.recv_timeout(DEADLINE).expect(
            "the thread keeps no memory of drawn keys, so asking for one again \
             draws it again",
        );
        assert_eq!(again.key, "k0@10x20");
    }

    #[test]
    #[tracing_test::traced_test]
    fn unknown_keys_warn_once_per_runtime_even_when_every_key_is_new() {
        let dir = tempfile::tempdir().expect("tempdir");
        let d3_dir = dir.path().join("d3");
        write_fake_files(&d3_dir);
        let evaluator = JsxEvaluator::new_draw(
            CIRCLE_LAYOUT,
            serde_json::Value::Null,
            Some(dir.path()),
            Some(d3_only_pkg_ctx(dir.path(), fake_d3_fetcher(&d3_dir))),
        )
        .expect("the draw evaluator builds");
        evaluator
            .ensure_drawing_ready()
            .expect("precondition: the fake d3 files load");
        evaluator
            .eval_inputs(&Default::default())
            .expect("the draw runtime's eval");
        let (drawn_tx, drawn) = mpsc::channel();
        let (notify_tx, _notified) = mpsc::sync_channel(1);
        let mut warned_unknown_key = false;

        // A key built from Date.now or a counter is new on every eval.
        for i in 0..3 {
            let request = DrawRequest {
                key: format!("r{i}@10x20"),
                width: 10.0,
                height: 20.0,
            };
            draw_one(
                &evaluator,
                &request,
                &mut warned_unknown_key,
                &drawn_tx,
                &notify_tx,
                GENERATION,
            );
        }

        logs_assert(|lines: &[&str]| {
            let n = lines
                .iter()
                .filter(|l| l.contains("WARN") && l.contains("nondeterministic"))
                .count();
            match n {
                1 => Ok(()),
                _ => Err(format!(
                    "keys this runtime's render does not name mean a nondeterministic render: \
                     warn once per runtime, got {n} warnings"
                )),
            }
        });
        assert!(drawn.try_recv().is_err(), "an unknown key draws nothing");
    }
}
