//! What the reconciler runtime found in the layout file.
//!
//! A Unit is declared by a `unit()` call and used as a component, so an Item is a
//! node whose `type` is the Unit descriptor rather than a tag name — see ADR 0033.
//! The collecting happens inside the reconciler runtime, where that descriptor is
//! still an object with callable hooks; the render runtime evaluates the same file
//! but its Items go nowhere (ADR 0034).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::RwLock;
use std::thread::JoinHandle;
use std::time::Duration;

use optative::{Lifecycle, OptativeSet, Reconcile};
use serde_json::Value;

use crate::jsx::JsxEvaluator;

/// Every Item of one Unit, as the reconciler runtime sees them.
///
/// `unit_index` addresses the live `unit()` object in that runtime's
/// `__tauler_units` array — which is the whole point of collecting there rather
/// than trusting a number from the render runtime. `__estoId` cannot serve: it
/// comes from a process-global counter, so two runtimes in one process number
/// the same Unit differently (ADR 0034).
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct UnitBatch {
    /// Index into the reconciler runtime's `__tauler_units`.
    pub unit_index: usize,
    /// Each Item's props, in document order.
    pub items: Vec<Value>,
}

/// How long a Unit waits before its next Sweep when it has nothing to do and the
/// layout file did not say. Short enough that a light switched by hand comes back
/// within a breath, long enough that an idle desktop is not running `observe` in a
/// loop.
const DEFAULT_REFRESH_INTERVAL: Duration = Duration::from_secs(5);

/// What one Sweep did, and when the next one is due.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SweepReport {
    pub entered: usize,
    pub updated: usize,
    pub exited: usize,
    /// How long to wait before sweeping again. Zero after a Sweep that changed
    /// something: the observation it worked from describes a world that no longer
    /// exists, so the next thing to do is look again (ADR 0035).
    pub next_due: Duration,
}

impl SweepReport {
    /// Whether the Sweep changed anything.
    pub fn made_progress(&self) -> bool {
        self.entered + self.updated + self.exited > 0
    }

    fn merge(self, other: SweepReport) -> SweepReport {
        SweepReport {
            entered: self.entered + other.entered,
            updated: self.updated + other.updated,
            exited: self.exited + other.exited,
            // One thread sweeps every Unit together, so the wake has to be soon
            // enough for the most impatient of them (ADR 0034).
            next_due: self.next_due.min(other.next_due),
        }
    }
}

/// One Item of one Unit, as the diff sees it.
///
/// `key` decides which Items are the same Item across a Sweep; `value` decides
/// whether one has changed. Both come from the Unit's own projections, so what
/// counts as "changed" is the layout file's business, not tauler's.
struct SweepItem {
    key: String,
    value: Value,
    props: Value,
}

/// What the set carries between Sweeps: the value the world last showed, and the
/// props to hand `exit` once the Item is gone from the layout and only the store
/// remembers it.
type ItemState = (Value, Value);

/// The three batches a diff fills. They are flushed to JavaScript after
/// `reconcile` returns rather than during it, because `optative`'s lifecycle is
/// per-Item and tauler's hooks take a batch (ADR 0033) — the diff decides *which*
/// Items, one call decides *when*.
#[derive(Default)]
struct Batches {
    enter: Vec<Value>,
    update: Vec<Value>,
    exit: Vec<Value>,
}

impl Lifecycle for SweepItem {
    type Key = String;
    type State = ItemState;
    type Context = Batches;
    type Output = ();
    type Error = std::convert::Infallible;

    fn key(&self) -> String {
        self.key.clone()
    }

    fn enter(self, ctx: &mut Batches, _: &mut ()) -> Result<ItemState, Self::Error> {
        ctx.enter.push(self.props.clone());
        Ok((self.value, self.props))
    }

    fn reconcile_self(
        self,
        state: &mut ItemState,
        ctx: &mut Batches,
        _: &mut (),
    ) -> Result<(), Self::Error> {
        if state.0 != self.value {
            ctx.update.push(self.props.clone());
            *state = (self.value, self.props);
        }
        Ok(())
    }

    fn exit(state: ItemState, ctx: &mut Batches, _: &mut ()) -> Result<(), Self::Error> {
        ctx.exit.push(state.1);
        Ok(())
    }
}

/// One turn of reconciliation for every Unit the layout declares: observe, diff,
/// run the hooks the diff calls for.
///
/// The layout is evaluated here, in the reconciler runtime, which is what makes
/// the Items' hooks callable at all (ADR 0034). `stream_values` is the render
/// loop's, so both evaluations see the same data — but they happen at different
/// moments, and only this one decides what gets reconciled.
pub fn sweep(
    evaluator: &JsxEvaluator,
    stream_values: &HashMap<(String, Option<String>), String>,
) -> SweepReport {
    let Ok(batches) = evaluator.eval_units(stream_values) else {
        tracing::error!("layout file failed to evaluate in the reconciler runtime");
        return SweepReport::default();
    };
    let start = SweepReport {
        next_due: Duration::MAX,
        ..SweepReport::default()
    };
    let report = batches
        .iter()
        .fold(start, |acc, batch| acc.merge(sweep_unit(evaluator, batch)));
    if report.next_due == Duration::MAX {
        // No Units at all: nothing will ever change until the layout file does,
        // and that arrives as a reload rather than as a Sweep.
        return SweepReport {
            next_due: DEFAULT_REFRESH_INTERVAL,
            ..report
        };
    }
    report
}

fn sweep_unit(evaluator: &JsxEvaluator, batch: &UnitBatch) -> SweepReport {
    let unit = batch.unit_index;
    let project = |name: &str, item: &Value| evaluator.call_unit_projection(unit, name, item);
    let key_of = |item: &Value| project("key", item).map(|k| json_key(&k));
    let value_of = |item: &Value| project("value", item).unwrap_or(Value::Null);

    // The store is rebuilt from the observation every Sweep rather than carried
    // over, because the observation is the only thing that knows the truth —
    // a hook's return value does not (ADR 0035).
    let observed = evaluator
        .observe(unit)
        .and_then(|v| match v {
            Value::Array(a) => Some(a),
            _ => None,
        })
        .unwrap_or_default();
    let mut set = OptativeSet::<SweepItem>::with_initial_state(
        observed
            .into_iter()
            .filter_map(|item| Some((key_of(&item)?, (value_of(&item), item)))),
    );

    let desired: Vec<SweepItem> = batch
        .items
        .iter()
        .filter_map(|props| {
            Some(SweepItem {
                key: key_of(props)?,
                value: value_of(props),
                props: props.clone(),
            })
        })
        .collect();

    let mut acc = Batches::default();
    set.reconcile(desired, &mut acc, &mut ());

    // Exits first, so a Unit that has to free something before claiming it again
    // — a port, a lock, a single physical device — can.
    //
    // A batch whose hook the Unit does not define is dropped rather than run and
    // does not count: a Unit with no `exit` is not managing what it did not
    // declare, and an `observe` that reports the whole world would otherwise put
    // every stranger's Item in a batch and call that progress, forever.
    let mut ran = [0usize; 3];
    for (slot, (hook, items)) in [
        ("exit", &acc.exit),
        ("update", &acc.update),
        ("enter", &acc.enter),
    ]
    .into_iter()
    .enumerate()
    {
        if items.is_empty() || !evaluator.has_unit_hook(unit, hook) {
            continue;
        }
        evaluator.call_unit_hook(unit, hook, items);
        ran[slot] = items.len();
    }
    let report = SweepReport {
        exited: ran[0],
        updated: ran[1],
        entered: ran[2],
        next_due: Duration::ZERO,
    };
    if report.made_progress() {
        return report;
    }
    SweepReport {
        next_due: refresh_interval(evaluator, unit),
        ..report
    }
}

/// A Unit's `refreshInterval`, in milliseconds, or the default. It sits on the
/// Unit rather than on its reconciler because it is a statement about how fast
/// the world behind the Unit moves, not about which backend reads it.
fn refresh_interval(evaluator: &JsxEvaluator, unit: usize) -> Duration {
    evaluator
        .unit_property(unit, "refreshInterval")
        .and_then(|v| v.as_u64())
        .map_or(DEFAULT_REFRESH_INTERVAL, Duration::from_millis)
}

/// A key is whatever `key` returned, rendered as JSON so that a string, a number
/// and a tuple all compare the way the layout file meant them to.
fn json_key(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// The Stream values a layout file is evaluated against, shared with the render
/// loop so both runtimes read the same data.
pub type SharedStreamValues = Arc<RwLock<HashMap<(String, Option<String>), String>>>;

/// How often a sleeping reconciler thread notices it has been asked to stop.
const STOP_POLL: Duration = Duration::from_millis(50);

/// The shortest gap between two Sweeps, however eager the previous one's report.
///
/// A Sweep that made progress asks for the next one immediately (ADR 0035), which
/// is right when it converges after a turn or two and ruinous when it never
/// converges — a Unit whose `enter` silently fails to change what `observe` reads
/// would otherwise spin a core for as long as tauler runs. This bounds that at
/// something the machine does not notice while still converging inside one frame.
const MIN_SWEEP_GAP: Duration = Duration::from_millis(10);

/// The reconciler thread: it owns the second QuickJS runtime and does nothing but
/// Sweep (ADR 0034).
///
/// Nothing here is on the render loop's path. The loop never waits for a Sweep
/// and a Sweep never touches the Dom, so a hook that takes forty seconds shows up
/// as "converged late", not as a dropped frame. Dropping this stops the thread.
pub struct Reconciler {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Reconciler {
    /// Start sweeping `source` on its own thread.
    ///
    /// `stream_values` is the render loop's, read fresh each Sweep, so the two
    /// runtimes evaluate the same layout against the same data — as nearly as two
    /// evaluations at different moments can.
    pub fn spawn(
        source: String,
        ctx: Value,
        base_dir: Option<PathBuf>,
        stream_values: SharedStreamValues,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = Arc::clone(&stop);
        let handle = std::thread::Builder::new()
            .name("tauler-reconciler".into())
            .spawn(move || {
                let evaluator =
                    match JsxEvaluator::new_reconciler(&source, ctx, base_dir.as_deref()) {
                        Ok(e) => e,
                        Err(e) => {
                            tracing::error!(error = ?e, "reconciler runtime failed to start");
                            return;
                        }
                    };
                while !stop_thread.load(Ordering::Relaxed) {
                    let values = stream_values.read().unwrap().clone();
                    let report = sweep(&evaluator, &values);
                    sleep_until_due(report.next_due.max(MIN_SWEEP_GAP), &stop_thread);
                }
            })
            .expect("spawning the reconciler thread");
        Self {
            stop,
            handle: Some(handle),
        }
    }
}

impl Drop for Reconciler {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            // A Sweep in flight is waited out rather than cut short: a hook is
            // half-way through changing the world and nothing can undo that
            // safely (ADR 0035).
            let _ = handle.join();
        }
    }
}

/// Sleep in slices, so a stop is noticed within [`STOP_POLL`] however long the
/// refresh interval is.
fn sleep_until_due(due: Duration, stop: &AtomicBool) {
    let mut left = due;
    while !left.is_zero() && !stop.load(Ordering::Relaxed) {
        let slice = left.min(STOP_POLL);
        std::thread::sleep(slice);
        left -= slice;
    }
}

#[cfg(test)]
mod tests {
    use crate::jsx::JsxEvaluator;
    use std::collections::HashMap;
    use std::sync::{Arc, RwLock};

    /// The reconciler runtime diffs against its *own* evaluation, not the render
    /// runtime's, so it needs the Items grouped by the live `unit()` object that
    /// declared them — the object whose hooks it is about to call. Grouping by
    /// `__estoId` is not an option: that counter is a process-global static, so
    /// the two runtimes never agree on a number (ADR 0034).
    #[test]
    fn a_reconciler_runtime_groups_its_items_by_unit() {
        let evaluator = JsxEvaluator::new_reconciler(
            r#"
            const Light = unit({
              key: (i) => i.entity,
              value: (i) => i.state,
              reconciler: optativeSet({ observe: () => [] }),
            });
            const Volume = unit({
              key: (i) => i.sink,
              value: (i) => i.level,
              reconciler: optativeSet({ observe: () => [] }),
            });
            export default function render() {
              return (
                <root>
                  <Light entity="light.desk" state="on" />
                  <panel><Light entity="light.hall" state="off" /></panel>
                  <Volume sink="analog" level={30} />
                </root>
              );
            }"#,
            serde_json::Value::Null,
            None,
        )
        .unwrap();

        let batches = evaluator.eval_units(&HashMap::new()).unwrap();

        assert_eq!(batches.len(), 2, "one batch per Unit: got {batches:?}");
        assert_eq!(batches[0].unit_index, 0);
        assert_eq!(
            batches[0].items.len(),
            2,
            "both Lights belong to the same Unit, wherever they sit in the tree"
        );
        assert_eq!(batches[0].items[0]["entity"], "light.desk");
        assert_eq!(batches[0].items[1]["entity"], "light.hall");
        assert_eq!(batches[1].unit_index, 1);
        assert_eq!(batches[1].items[0]["sink"], "analog");
    }

    /// A hook takes the whole batch, not one Item — ADR 0033 makes batch the
    /// primitive and per-item the sugar, so a Unit that talks to an API can make
    /// one request for ten lights instead of ten.
    #[test]
    fn a_units_hook_is_called_with_the_whole_batch() {
        let evaluator = JsxEvaluator::new_reconciler(
            r#"
            const Light = unit({
              key: (i) => i.entity,
              value: (i) => i.state,
              reconciler: optativeSet({ observe: () => [] }),
              enter: (items) => items.map((i) => `${i.entity}=${i.state}`),
            });
            export default function render() {
              return (
                <root>
                  <Light entity="light.desk" state="on" />
                  <Light entity="light.hall" state="off" />
                </root>
              );
            }"#,
            serde_json::Value::Null,
            None,
        )
        .unwrap();

        let batches = evaluator.eval_units(&HashMap::new()).unwrap();
        let out = evaluator
            .call_unit_hook(batches[0].unit_index, "enter", &batches[0].items)
            .unwrap();

        assert_eq!(
            out,
            serde_json::json!(["light.desk=on", "light.hall=off"]),
            "the hook must see every Item at once"
        );
    }

    /// `observe` is the truth channel (ADR 0035): the reconciler asks the world
    /// what it holds rather than believing a hook's return value. It sits on the
    /// reconciler backend, not the Unit, and it is the one hook that has a real
    /// reason to shell out.
    #[test]
    fn a_units_observe_reports_the_world_and_may_shell_out() {
        let evaluator = JsxEvaluator::new_reconciler(
            r#"
            const Light = unit({
              key: (i) => i.entity,
              value: (i) => i.state,
              reconciler: optativeSet({
                observe: () => [{ entity: sh`printf light.desk`, state: "on" }],
              }),
            });
            export default function render() {
              return <root><Light entity="light.desk" state="on" /></root>;
            }"#,
            serde_json::Value::Null,
            None,
        )
        .unwrap();

        let batches = evaluator.eval_units(&HashMap::new()).unwrap();
        let observed = evaluator.observe(batches[0].unit_index).unwrap();

        assert_eq!(
            observed,
            serde_json::json!([{ "entity": "light.desk", "state": "on" }]),
            "observe must run in the runtime that has a shell"
        );
    }

    /// The whole loop in one turn: observe the world, diff it against what the
    /// layout declared, call the hooks the diff asks for. `light.hall` exists but
    /// holds the wrong value, `light.desk` does not exist yet, and `light.attic`
    /// exists but nobody declared it — one Item for each of the three hooks.
    #[test]
    fn a_sweep_enters_updates_and_exits_from_one_observation() {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("state.json");
        let log = dir.path().join("log");
        std::fs::write(
            &state,
            r#"[{"entity":"light.hall","state":"off"},{"entity":"light.attic","state":"on"}]"#,
        )
        .unwrap();

        let source = light_layout(&state, &log, "");
        let evaluator =
            JsxEvaluator::new_reconciler(&source, serde_json::Value::Null, None).unwrap();

        let report = crate::units::sweep(&evaluator, &HashMap::new());

        assert_eq!(report.entered, 1, "light.desk is not out there yet");
        assert_eq!(report.updated, 1, "light.hall is out there but off");
        assert_eq!(report.exited, 1, "nobody declared light.attic");
        assert!(report.made_progress());

        let mut lines: Vec<String> = std::fs::read_to_string(&log)
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect();
        lines.sort();
        assert_eq!(
            lines,
            vec!["enter light.desk", "exit light.attic", "update light.hall"],
            "each hook must see the Items the diff put in its batch"
        );
    }

    /// Nothing to do is the steady state, and the Sweep has to say so — that is
    /// what decides whether the next one waits out the refresh interval (ADR 0035).
    #[test]
    fn a_sweep_that_matches_the_world_makes_no_progress() {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("state.json");
        let log = dir.path().join("log");
        std::fs::write(
            &state,
            r#"[{"entity":"light.desk","state":"on"},{"entity":"light.hall","state":"on"}]"#,
        )
        .unwrap();

        let source = light_layout(&state, &log, "");
        let evaluator =
            JsxEvaluator::new_reconciler(&source, serde_json::Value::Null, None).unwrap();

        let report = crate::units::sweep(&evaluator, &HashMap::new());

        assert!(!report.made_progress(), "got {report:?}");
        assert!(!log.exists(), "no hook should have run");
    }

    /// A Sweep says when the next one is due, and that is the whole scheduler: a
    /// Sweep that changed something has made the world it just observed stale, so
    /// the next one runs now; one that changed nothing waits out the Unit's
    /// refresh interval (ADR 0035).
    #[test]
    fn a_sweep_says_when_the_next_one_is_due() {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("state.json");
        let log = dir.path().join("log");
        let source = light_layout(&state, &log, "refreshInterval: 250,");

        std::fs::write(&state, "[]").unwrap();
        let evaluator =
            JsxEvaluator::new_reconciler(&source, serde_json::Value::Null, None).unwrap();
        let busy = crate::units::sweep(&evaluator, &HashMap::new());
        assert!(busy.made_progress());
        assert_eq!(
            busy.next_due,
            std::time::Duration::ZERO,
            "after doing something, observe again before deciding anything else"
        );

        std::fs::write(
            &state,
            r#"[{"entity":"light.desk","state":"on"},{"entity":"light.hall","state":"on"}]"#,
        )
        .unwrap();
        let idle = crate::units::sweep(&evaluator, &HashMap::new());
        assert!(!idle.made_progress());
        assert_eq!(
            idle.next_due,
            std::time::Duration::from_millis(250),
            "an idle Sweep waits exactly as long as the layout file asked for"
        );
    }

    /// A Unit with no `exit` hook does not manage what it does not declare, and a
    /// diff full of Items it will never act on is not progress — without this, a
    /// Unit whose `observe` reports the whole world sweeps flat out forever.
    #[test]
    fn a_transition_with_no_hook_is_not_progress() {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("state.json");
        let log = dir.path().join("log");
        std::fs::write(
            &state,
            r#"[{"entity":"light.desk","state":"on"},
                {"entity":"light.hall","state":"on"},
                {"entity":"light.someone.elses","state":"on"}]"#,
        )
        .unwrap();
        let source = light_layout(&state, &log, "")
            .lines()
            .filter(|l| !l.trim_start().starts_with("exit:"))
            .collect::<Vec<_>>()
            .join("\n");
        let evaluator =
            JsxEvaluator::new_reconciler(&source, serde_json::Value::Null, None).unwrap();

        let report = crate::units::sweep(&evaluator, &HashMap::new());

        assert_eq!(report.exited, 0, "there is no exit hook to run: {report:?}");
        assert!(!report.made_progress());
        assert_ne!(report.next_due, std::time::Duration::ZERO);
    }

    /// A Unit is declared next to the panels, and the render side has to ignore
    /// it: an Item is a statement about the world, not about the bar. Rendering
    /// one is not a thing, and failing to parse one would take the whole layout
    /// down with it.
    #[test]
    fn an_item_under_root_is_not_a_surface() {
        let evaluator = JsxEvaluator::new(
            r#"
            const Light = unit({
              key: (i) => i.entity,
              value: (i) => i.state,
              reconciler: optativeSet({ observe: () => [] }),
            });
            export default function render() {
              return (
                <root>
                  <Light entity="light.desk" state="on" />
                  <panel id="bar" anchor="top" width={1920} height={32} outer_gap={0} />
                </root>
              );
            }"#,
            serde_json::Value::Null,
            None,
        )
        .unwrap();
        let layout = evaluator.eval(&HashMap::new()).unwrap().layout;

        let surfaces = crate::parse_root_node(&layout).unwrap();

        assert_eq!(surfaces.len(), 1, "only the panel is a Surface");
        assert_eq!(surfaces[0].id, "bar");
    }

    /// The thread keeps sweeping on its own. It is given a world that never
    /// converges — `observe` always reports the light off, the layout always asks
    /// for it on — so a correct loop keeps calling `update` and a loop that
    /// sweeps once does not.
    #[test]
    fn the_reconciler_thread_sweeps_until_it_is_stopped() {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("state.json");
        let log = dir.path().join("log");
        std::fs::write(&state, r#"[{"entity":"light.desk","state":"off"}]"#).unwrap();
        let source = light_layout(&state, &log, "");

        let reconciler = crate::units::Reconciler::spawn(
            source,
            serde_json::Value::Null,
            None,
            Arc::new(RwLock::new(HashMap::new())),
        );

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let updates = loop {
            let n = std::fs::read_to_string(&log)
                .map(|s| s.lines().filter(|l| l.starts_with("update")).count())
                .unwrap_or(0);
            if n >= 3 || std::time::Instant::now() > deadline {
                break n;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        drop(reconciler);

        assert!(
            updates >= 3,
            "the thread must keep sweeping while the world disagrees: saw {updates}"
        );
    }

    /// The Home Assistant Unit from `docs/src/content/docs/docs/units.md`, with
    /// only its `hass` helper swapped for one that reads and writes files. The
    /// parts a reader can get wrong — filtering `observe` down to the entities
    /// this Unit manages, and picking `turn_on` against `turn_off` — are the
    /// parts under test.
    #[test]
    fn the_home_assistant_example_from_the_docs_works() {
        let dir = tempfile::tempdir().unwrap();
        let states = dir.path().join("states.json");
        let calls = dir.path().join("calls");
        // Two lights this Unit manages, one it does not.
        std::fs::write(
            &states,
            r#"[{"entity_id":"light.desk","state":"off"},
                {"entity_id":"light.hall","state":"on"},
                {"entity_id":"light.someone.elses","state":"on"}]"#,
        )
        .unwrap();

        let source = HASS_LAYOUT
            .replace("__STATES__", states.to_str().unwrap())
            .replace("__CALLS__", calls.to_str().unwrap());
        let evaluator =
            JsxEvaluator::new_reconciler(&source, serde_json::Value::Null, None).unwrap();

        let report = crate::units::sweep(&evaluator, &HashMap::new());

        assert_eq!(report.updated, 1, "only light.desk is wrong: {report:?}");
        assert_eq!(report.entered, 0);
        assert_eq!(report.exited, 0, "the stranger's light is not ours to exit");
        assert_eq!(
            std::fs::read_to_string(&calls).unwrap().trim(),
            "/api/services/light/turn_on light.desk"
        );
    }

    /// Builds a layout from [`LIGHT_LAYOUT`], pointed at this test's files and
    /// with `extra` spliced into the `unit()` call.
    fn light_layout(state: &std::path::Path, log: &std::path::Path, extra: &str) -> String {
        LIGHT_LAYOUT
            .replace("__STATE__", state.to_str().unwrap())
            .replace("__LOG__", log.to_str().unwrap())
            .replace("__EXTRA__", extra)
    }

    /// The docs' Home Assistant Unit, verbatim apart from `hass`.
    const HASS_LAYOUT: &str = r#"
        const MINE = ["light.desk", "light.hall"];

        const hass = (path, body) =>
          body
            ? sh`printf '%s %s\n' ${path} ${body.entity_id} >> __CALLS__`
            : sh`cat __STATES__`;

        const Light = unit({
          refreshInterval: 5000,

          key: (light) => light.entity,
          value: (light) => light.state,

          reconciler: optativeSet({
            observe: () =>
              JSON.parse(hass("/api/states"))
                .filter((s) => MINE.includes(s.entity_id))
                .map((s) => ({ entity: s.entity_id, state: s.state })),
          }),

          enter: (lights) => apply(lights),
          update: (lights) => apply(lights),
        });

        function apply(lights) {
          for (const light of lights) {
            hass(`/api/services/light/turn_${light.state === "on" ? "on" : "off"}`, {
              entity_id: light.entity,
            });
          }
        }

        export default function render() {
          return (
            <root>
              <Light entity="light.desk" state="on" />
              <Light entity="light.hall" state="on" />
            </root>
          );
        }"#;

    /// A Unit whose `observe` reads a file and whose hooks append to a log, so a
    /// Sweep's effects are checkable from outside the runtime.
    const LIGHT_LAYOUT: &str = r#"
        const Light = unit({
          __EXTRA__
          key: (i) => i.entity,
          value: (i) => i.state,
          reconciler: optativeSet({ observe: () => JSON.parse(sh`cat __STATE__`) }),
          enter: (items) => items.forEach((i) => sh`printf 'enter %s\n' ${i.entity} >> __LOG__`),
          update: (items) => items.forEach((i) => sh`printf 'update %s\n' ${i.entity} >> __LOG__`),
          exit: (items) => items.forEach((i) => sh`printf 'exit %s\n' ${i.entity} >> __LOG__`),
        });
        export default function render() {
          return (
            <root>
              <Light entity="light.desk" state="on" />
              <Light entity="light.hall" state="on" />
            </root>
          );
        }"#;
}
