//! What the reconciler runtime found in the layout file.
//!
//! A Unit is declared by a `unit()` call and used as a component, so an Item is a
//! node whose `type` is the Unit descriptor rather than a tag name — see ADR 0033.
//! The collecting happens inside the reconciler runtime, where that descriptor is
//! still an object with callable hooks; the render runtime evaluates the same file
//! but its Items go nowhere (ADR 0034).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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

/// Remove every Item from a layout tree.
///
/// An Item draws nothing — a Unit is a statement about the world, not about the
/// bar — and the reconciler finds it in its own evaluation rather than in this
/// tree (ADR 0034). So on the render side it is not merely inert, it is in the
/// way: `build_element` demands a string tag and an Item's `type` is the Unit
/// descriptor, and `layout::first_child` takes a panel's first child as its whole
/// content, so a Unit written above the UI it belongs to would silently swallow
/// that UI.
///
/// Stripping once, here, is what lets a Unit be declared next to the thing it
/// drives instead of hoisted to `<root>` by hand.
pub fn strip_items(value: Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.into_iter().map(strip_items).collect()),
        Value::Object(mut map) => {
            if let Some(Value::Array(children)) = map.remove("children") {
                map.insert(
                    "children".to_string(),
                    Value::Array(
                        children
                            .into_iter()
                            .filter(|c| !is_item(c))
                            .map(strip_items)
                            .collect(),
                    ),
                );
            }
            Value::Object(map)
        }
        other => other,
    }
}

/// Whether a node is an Item — an instance of a `unit()`-declared Unit.
fn is_item(node: &Value) -> bool {
    node.get("type")
        .and_then(|t| t.get(optative_script::tags::ESTO_KIND))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// How often a Unit sweeps when the layout file does not say. Short enough that a
/// light switched by hand comes back within a breath, long enough that an idle
/// desktop is not running `observe` in a loop.
pub(crate) const DEFAULT_REFRESH_INTERVAL: Duration = Duration::from_secs(5);

/// How often a layout that declares no Units is re-checked.
///
/// The check is a `HashMap` comparison, not an evaluation — so this can be short
/// without costing anything, and a Unit that only appears once some Stream has
/// reported starts reconciling a quarter-second later rather than five seconds
/// later.
const IDLE_POLL: Duration = Duration::from_millis(250);

/// What one Sweep did, and when the next one is due.
///
/// `next_sweep` does not depend on what happened. A Sweep that acted does not
/// earn an earlier next turn: making a Sweep's outcome decide its own cadence is
/// what turned "a hook failed" into "a hook fails a hundred times a second", and
/// the rule that did it has been removed from ADR 0035. One knob — the Unit's
/// `refreshInterval` — sets both how fast it converges and how hard it can hammer
/// when it is broken.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SweepReport {
    pub entered: usize,
    pub updated: usize,
    pub exited: usize,
    /// How long until the next Sweep: the shortest `refreshInterval` among the
    /// Units this one swept, since one thread sweeps them together.
    pub next_sweep: Duration,
    /// How many Units the layout declared. Zero means there is nothing to
    /// reconcile and no reason to evaluate again until the data moves.
    pub units: usize,
}

impl SweepReport {
    /// Whether the Sweep changed anything. A log line, not a scheduling input.
    pub fn made_progress(&self) -> bool {
        self.entered + self.updated + self.exited > 0
    }

    fn merge(self, other: SweepReport) -> SweepReport {
        SweepReport {
            entered: self.entered + other.entered,
            updated: self.updated + other.updated,
            exited: self.exited + other.exited,
            next_sweep: self.next_sweep.min(other.next_sweep),
            units: self.units + other.units,
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
    /// Where this Item sat in the layout. `optative` diffs through a `HashMap`,
    /// so without carrying the position a batch arrives in hash order — which is
    /// arbitrary, varies run to run, and would make any hook whose output depends
    /// on order (writing a file, say) nondeterministic.
    order: usize,
}

/// What the set carries between Sweeps: the value the world last showed, the
/// props to hand `exit` once the Item is gone from the layout and only the store
/// remembers it, and where `observe` reported it — an exiting Item has no
/// position in the layout, so its batch is ordered by the observation instead.
type ItemState = (Value, Value, usize);

/// The three batches a diff fills. They are flushed to JavaScript after
/// `reconcile` returns rather than during it, because `optative`'s lifecycle is
/// per-Item and tauler's hooks take a batch (ADR 0033) — the diff decides *which*
/// Items, one call decides *when*.
///
/// `update` carries pairs rather than bare Items: `esto`'s `update(new, old)`
/// hands the hook the previous Item so it can compute a delta, and dropping that
/// would make tauler's `update` strictly weaker. Pairs rather than two aligned
/// arrays, so nothing depends on two `Vec`s staying the same length.
#[derive(Default)]
struct Batches {
    enter: Vec<(usize, Value)>,
    update: Vec<(usize, (Value, Value))>,
    exit: Vec<(usize, Value)>,
}

impl Batches {
    /// Drop the ordering keys once they have done their job.
    fn ordered<T>(mut batch: Vec<(usize, T)>) -> Vec<T> {
        batch.sort_by_key(|(order, _)| *order);
        batch.into_iter().map(|(_, item)| item).collect()
    }
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
        ctx.enter.push((self.order, self.props.clone()));
        Ok((self.value, self.props, self.order))
    }

    fn reconcile_self(
        self,
        state: &mut ItemState,
        ctx: &mut Batches,
        _: &mut (),
    ) -> Result<(), Self::Error> {
        if state.0 != self.value {
            ctx.update
                .push((self.order, (self.props.clone(), state.1.clone())));
            *state = (self.value, self.props, self.order);
        }
        Ok(())
    }

    fn exit(state: ItemState, ctx: &mut Batches, _: &mut ()) -> Result<(), Self::Error> {
        ctx.exit.push((state.2, state.1));
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
    if batches.is_empty() {
        return SweepReport {
            next_sweep: IDLE_POLL,
            ..SweepReport::default()
        };
    }
    let start = SweepReport {
        next_sweep: Duration::MAX,
        ..SweepReport::default()
    };
    batches
        .iter()
        .fold(start, |acc, batch| acc.merge(sweep_unit(evaluator, batch)))
}

fn sweep_unit(evaluator: &JsxEvaluator, batch: &UnitBatch) -> SweepReport {
    let unit = batch.unit_index;
    let project = |name: &str, item: &Value| evaluator.call_unit_projection(unit, name, item);
    let key_of = |item: &Value| project("key", item).map(|k| json_key(&k));
    let value_of = |item: &Value| project("value", item).unwrap_or(Value::Null);

    // `optativeJsonSet` persists state to a file instead of observing, which is
    // the one thing a Unit in a layout file cannot do (ADR 0035). Left
    // unguarded it looks like a Unit whose world is permanently empty, so every
    // Item enters on every Sweep forever, silently.
    if evaluator.reconciler_kind(unit).as_deref() == Some("optativeJsonSet") {
        tracing::error!(
            "optativeJsonSet is not supported in a layout file: a Unit needs an \
             `observe` for tauler to know what the world holds (ADR 0035)"
        );
        return SweepReport {
            next_sweep: refresh_interval(evaluator, unit),
            units: 1,
            ..SweepReport::default()
        };
    }

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
            .enumerate()
            .filter_map(|(order, item)| Some((key_of(&item)?, (value_of(&item), item, order)))),
    );

    let desired: Vec<SweepItem> = batch
        .items
        .iter()
        .enumerate()
        .filter_map(|(order, props)| {
            Some(SweepItem {
                key: key_of(props)?,
                value: value_of(props),
                props: props.clone(),
                order,
            })
        })
        .collect();

    let mut acc = Batches::default();
    set.reconcile(desired, &mut acc, &mut ());

    // Exits first, so a Unit that has to free something before claiming it again
    // — a port, a lock, a single physical device — can.
    //
    // A batch whose hook the Unit defines neither spelling of runs nothing and
    // counts nothing: a Unit with no `exit` is not managing what it did not
    // declare, and an `observe` that reports the whole world would otherwise hand
    // `exit` every stranger's Item on every Sweep.
    let exit = json_items(&Batches::ordered(acc.exit));
    let update = json_pairs(&Batches::ordered(acc.update));
    let enter = json_items(&Batches::ordered(acc.enter));
    let exited = evaluator.dispatch_unit_hook(unit, "exit", "exitOne", &exit);
    let updated = evaluator.dispatch_unit_hook(unit, "update", "updateOne", &update);
    let entered = evaluator.dispatch_unit_hook(unit, "enter", "enterOne", &enter);

    SweepReport {
        exited,
        updated,
        entered,
        next_sweep: refresh_interval(evaluator, unit),
        units: 1,
    }
}

/// The payload a batch hook receives: the Items themselves, in the order the
/// layout declared them.
fn json_items(items: &[Value]) -> Value {
    Value::Array(items.to_vec())
}

/// The payload `update` receives: `{item, old}` per changed Item, so a hook can
/// diff against what the world had before without depending on two arrays lining
/// up.
fn json_pairs(pairs: &[(Value, Value)]) -> Value {
    Value::Array(
        pairs
            .iter()
            .map(|(item, old)| serde_json::json!({ "item": item, "old": old }))
            .collect(),
    )
}

/// How often this Unit sweeps, in milliseconds, or the default.
///
/// It sits on the Unit rather than on its reconciler because it is a statement
/// about how fast the world behind the Unit moves, not about which backend reads
/// it. It is also the Unit's blast radius: a Unit that can never converge retries
/// exactly this often, so the same number that buys responsiveness bounds the
/// damage.
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

/// How long a Sweep may run before the watchdog starts saying so.
///
/// A hook is allowed to be slow — `gaming_mode_exit.sh` on this desktop polls for
/// up to forty seconds and that is correct behaviour — so this is not a timeout
/// and nothing is cancelled. It only makes a stuck reconciler visible: without it
/// a hook that never returns stops every Unit for the life of the process and the
/// sole symptom is that nothing converges.
const SWEEP_WATCHDOG_AFTER: Duration = Duration::from_secs(60);

/// The reconciler thread: it owns the second QuickJS runtime and does nothing but
/// Sweep (ADR 0034).
///
/// Nothing here is on the render loop's path. The loop never waits for a Sweep
/// and a Sweep never touches the Dom, so a hook that takes forty seconds shows up
/// as "converged late", not as a dropped frame. Dropping this stops the thread.
pub struct Reconciler {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    watchdog: Option<JoinHandle<()>>,
}

impl Reconciler {
    /// Start sweeping `source` on its own thread.
    ///
    /// `stream_values` and `globals` are the render loop's, read fresh each Sweep,
    /// so the two runtimes evaluate the same layout against the same data — as
    /// nearly as two evaluations at different moments can.
    pub fn spawn(
        source: String,
        ctx: Value,
        base_dir: Option<PathBuf>,
        stream_values: SharedStreamValues,
        globals: crate::jsx::SharedGlobals,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = Arc::clone(&stop);
        // Milliseconds since the epoch at which the Sweep in flight began, or 0
        // when none is. Read by the watchdog, which cannot ask the thread.
        let started = Arc::new(AtomicU64::new(0));
        let started_watchdog = Arc::clone(&started);
        let stop_watchdog = Arc::clone(&stop);
        let watchdog = std::thread::Builder::new()
            .name("tauler-reconciler-watchdog".into())
            .spawn(move || watch_sweeps(&started_watchdog, &stop_watchdog))
            .expect("spawning the reconciler watchdog");
        let handle = std::thread::Builder::new()
            .name("tauler-reconciler".into())
            .spawn(move || {
                let evaluator = match JsxEvaluator::new_reconciler(
                    &source,
                    ctx,
                    base_dir.as_deref(),
                    globals,
                ) {
                    Ok(e) => e,
                    Err(e) => {
                        tracing::error!(error = ?e, "reconciler runtime failed to start");
                        return;
                    }
                };
                let mut swept: Option<(HashMap<_, _>, SweepReport)> = None;
                while !stop_thread.load(Ordering::Relaxed) {
                    let values = stream_values.read().unwrap().clone();
                    // A layout that declared no Units cannot start declaring one
                    // until its data changes, so re-evaluating it is pure waste —
                    // and the layout being evaluated is the user's, in a runtime
                    // that has a shell.
                    let idle = matches!(&swept, Some((last, r)) if r.units == 0 && *last == values);
                    if idle {
                        sleep_until_due(IDLE_POLL, &stop_thread);
                        continue;
                    }
                    started.store(now_millis(), Ordering::Relaxed);
                    let report = sweep(&evaluator, &values);
                    started.store(0, Ordering::Relaxed);
                    tracing::debug!(
                        entered = report.entered,
                        updated = report.updated,
                        exited = report.exited,
                        units = report.units,
                        next_sweep_ms = report.next_sweep.as_millis(),
                        "sweep"
                    );
                    sleep_until_due(report.next_sweep, &stop_thread);
                    swept = Some((values, report));
                }
            })
            .expect("spawning the reconciler thread");
        Self {
            stop,
            handle: Some(handle),
            watchdog: Some(watchdog),
        }
    }
}

/// Says so, repeatedly, while a Sweep has been running longer than
/// [`SWEEP_WATCHDOG_AFTER`].
///
/// It cannot cancel anything: the hook is blocked in a subprocess inside Rust,
/// not in the interpreter, so `rquickjs`'s interrupt handler never gets a turn.
/// Turning "nothing converges and nobody knows why" into a log line naming the
/// Unit is the whole job.
fn watch_sweeps(started: &AtomicU64, stop: &AtomicBool) {
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(STOP_POLL);
        let at = started.load(Ordering::Relaxed);
        if at == 0 {
            continue;
        }
        let running = now_millis().saturating_sub(at);
        if running >= SWEEP_WATCHDOG_AFTER.as_millis() as u64 {
            tracing::error!(
                running_secs = running / 1000,
                "a Sweep has not returned; every Unit is blocked behind it"
            );
        }
    }
}

/// Milliseconds since the epoch. Only ever subtracted from itself.
fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl Drop for Reconciler {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(watchdog) = self.watchdog.take() {
            let _ = watchdog.join();
        }
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
    /// primitive, so a Unit that talks to an API can make one request for ten
    /// lights instead of ten requests.
    #[test]
    fn a_units_hook_is_called_with_the_whole_batch() {
        let evaluator = JsxEvaluator::new_reconciler(
            r#"
            globalThis.seen = null;
            const Light = unit({
              key: (i) => i.entity,
              value: (i) => i.state,
              reconciler: optativeSet({ observe: () => [] }),
              enter: (items) => { globalThis.seen = items.map((i) => i.entity); },
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
        let items = serde_json::Value::Array(batches[0].items.clone());
        let ran = evaluator.dispatch_unit_hook(batches[0].unit_index, "enter", "enterOne", &items);

        assert_eq!(ran, 2);
        assert_eq!(
            evaluator.unit_property(usize::MAX, "ignored"),
            None,
            "an out-of-range Unit is nothing to do, not a panic"
        );
    }

    /// `enterOne` is the per-Item spelling, and which one a Unit means cannot be
    /// guessed — `(p) => …` and `(ps) => …` are the same JavaScript. The name is
    /// the whole signal (ADR 0033).
    #[test]
    fn the_one_suffixed_hooks_are_called_per_item() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("log");
        let source = format!(
            r#"
            const Light = unit({{
              key: (i) => i.entity,
              value: (i) => i.state,
              reconciler: optativeSet({{ observe: () => [] }}),
              enterOne: (i) => sh`printf '%s\n' ${{i.entity}} >> {}`,
            }});
            export default function render() {{
              return (
                <root>
                  <Light entity="light.desk" state="on" />
                  <Light entity="light.hall" state="off" />
                </root>
              );
            }}"#,
            log.to_str().unwrap()
        );
        let evaluator = JsxEvaluator::new_reconciler(
            &source,
            serde_json::Value::Null,
            None,
            Default::default(),
        )
        .unwrap();

        let report = crate::units::sweep(&evaluator, &HashMap::new());

        assert_eq!(report.entered, 2);
        assert_eq!(
            std::fs::read_to_string(&log).unwrap(),
            "light.desk\nlight.hall\n",
            "each Item its own call, in document order"
        );
    }

    /// A hook written per-Item and handed a batch reads `undefined` off an Array
    /// and runs a command with a missing argument. The guard turns that into an
    /// error that names the fix.
    #[test]
    fn a_per_item_hook_bound_to_the_batch_name_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("log");
        let source = format!(
            r#"
            const Light = unit({{
              key: (i) => i.entity,
              value: (i) => i.state,
              reconciler: optativeSet({{ observe: () => [] }}),
              enter: (i) => sh`printf '%s\n' ${{i.entity}} >> {}`,
            }});
            export default function render() {{
              return <root><Light entity="light.desk" state="on" /></root>;
            }}"#,
            log.to_str().unwrap()
        );
        let evaluator = JsxEvaluator::new_reconciler(
            &source,
            serde_json::Value::Null,
            None,
            Default::default(),
        )
        .unwrap();

        let report = crate::units::sweep(&evaluator, &HashMap::new());

        assert_eq!(report.entered, 0, "the hook threw rather than half-running");
        assert!(!log.exists(), "and nothing reached the world");
    }

    /// Defining both spellings is an authoring mistake with no sensible reading,
    /// so it fails rather than silently preferring one.
    #[test]
    fn a_unit_may_not_define_both_spellings_of_a_hook() {
        let evaluator = JsxEvaluator::new_reconciler(
            r#"
            const Light = unit({
              key: (i) => i.entity,
              value: (i) => i.state,
              reconciler: optativeSet({ observe: () => [] }),
              enter: (items) => {},
              enterOne: (i) => {},
            });
            export default function render() {
              return <root><Light entity="light.desk" state="on" /></root>;
            }"#,
            serde_json::Value::Null,
            None,
        )
        .unwrap();

        let report = crate::units::sweep(&evaluator, &HashMap::new());

        assert_eq!(report.entered, 0);
        assert!(!report.made_progress());
    }

    /// `update` gets the previous Item alongside the new one, as `esto`'s
    /// `update(new, old)` does — a Unit that has to compute a delta cannot do it
    /// from the desired value alone.
    #[test]
    fn update_carries_the_value_the_world_had_before() {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("state.json");
        let log = dir.path().join("log");
        std::fs::write(&state, r#"[{"entity":"light.desk","state":"off"}]"#).unwrap();
        let source = format!(
            r#"
            const Light = unit({{
              key: (i) => i.entity,
              value: (i) => i.state,
              reconciler: optativeSet({{ observe: () => JSON.parse(sh`cat {}`) }}),
              update: (pairs) =>
                pairs.forEach(({{ item, old }}) =>
                  sh`printf '%s->%s\n' ${{old.state}} ${{item.state}} >> {}`),
            }});
            export default function render() {{
              return <root><Light entity="light.desk" state="on" /></root>;
            }}"#,
            state.to_str().unwrap(),
            log.to_str().unwrap()
        );
        let evaluator = JsxEvaluator::new_reconciler(
            &source,
            serde_json::Value::Null,
            None,
            Default::default(),
        )
        .unwrap();

        let report = crate::units::sweep(&evaluator, &HashMap::new());

        assert_eq!(report.updated, 1);
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "off->on\n");
    }

    /// `optativeJsonSet` has no `observe`, so tauler cannot know what the world
    /// holds. Left unguarded it looks like a permanently empty world and every
    /// Item enters on every Sweep (ADR 0035).
    #[test]
    fn a_unit_backed_by_optative_json_set_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("log");
        let source = format!(
            r#"
            const Light = unit({{
              key: (i) => i.entity,
              value: (i) => i.state,
              reconciler: optativeJsonSet({{ file: "/tmp/nope.jsonl" }}),
              enter: (items) => sh`printf x >> {}`,
            }});
            export default function render() {{
              return <root><Light entity="light.desk" state="on" /></root>;
            }}"#,
            log.to_str().unwrap()
        );
        let evaluator = JsxEvaluator::new_reconciler(
            &source,
            serde_json::Value::Null,
            None,
            Default::default(),
        )
        .unwrap();

        let report = crate::units::sweep(&evaluator, &HashMap::new());

        assert!(!report.made_progress(), "got {report:?}");
        assert!(
            !log.exists(),
            "no hook may run for a Unit tauler cannot observe"
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
        let evaluator = JsxEvaluator::new_reconciler(
            &source,
            serde_json::Value::Null,
            None,
            Default::default(),
        )
        .unwrap();

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
        let evaluator = JsxEvaluator::new_reconciler(
            &source,
            serde_json::Value::Null,
            None,
            Default::default(),
        )
        .unwrap();

        let report = crate::units::sweep(&evaluator, &HashMap::new());

        assert!(!report.made_progress(), "got {report:?}");
        assert!(!log.exists(), "no hook should have run");
    }

    /// `refreshInterval` is the whole scheduler. A Sweep that acted does not earn
    /// an earlier next turn — letting the outcome pick the cadence is what let a
    /// failing hook retry a hundred times a second.
    #[test]
    fn a_sweep_is_due_after_the_refresh_interval_whatever_it_did() {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("state.json");
        let log = dir.path().join("log");
        let source = light_layout(&state, &log, "refreshInterval: 250,");

        std::fs::write(&state, "[]").unwrap();
        let evaluator = JsxEvaluator::new_reconciler(
            &source,
            serde_json::Value::Null,
            None,
            Default::default(),
        )
        .unwrap();

        let busy = crate::units::sweep(&evaluator, &HashMap::new());
        assert!(busy.made_progress());
        assert_eq!(busy.next_sweep, std::time::Duration::from_millis(250));

        std::fs::write(
            &state,
            r#"[{"entity":"light.desk","state":"on"},{"entity":"light.hall","state":"on"}]"#,
        )
        .unwrap();
        let idle = crate::units::sweep(&evaluator, &HashMap::new());
        assert!(!idle.made_progress());
        assert_eq!(
            idle.next_sweep,
            std::time::Duration::from_millis(250),
            "same interval, whatever the last Sweep did"
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
        let evaluator = JsxEvaluator::new_reconciler(
            &source,
            serde_json::Value::Null,
            None,
            Default::default(),
        )
        .unwrap();

        let report = crate::units::sweep(&evaluator, &HashMap::new());

        assert_eq!(report.exited, 0, "there is no exit hook to run: {report:?}");
        assert!(!report.made_progress());
        assert_eq!(report.next_sweep, crate::units::DEFAULT_REFRESH_INTERVAL);
    }

    /// A layout with no Units must cost nothing. The thread has to evaluate once
    /// to find that out — only the reconciler runtime can answer "are there
    /// Units?" — but re-evaluating a Unit-less layout every five seconds forever
    /// is 0.25% of a core spent on a question whose answer cannot change until
    /// the data does.
    #[test]
    fn a_layout_with_no_units_is_evaluated_once() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("evals");
        let source = format!(
            r#"export default function render() {{
                 sh`printf x >> {}`;
                 return <root />;
               }}"#,
            log.to_str().unwrap()
        );

        let reconciler = crate::units::Reconciler::spawn(
            source,
            serde_json::Value::Null,
            None,
            Arc::new(RwLock::new(HashMap::new())),
        );
        std::thread::sleep(std::time::Duration::from_millis(900));
        drop(reconciler);

        let evals = std::fs::read_to_string(&log).unwrap_or_default().len();
        eprintln!("EVALS={evals}");
        assert_eq!(
            evals, 1,
            "evaluated {evals} times with nothing to reconcile"
        );
    }

    /// A Unit declared next to the UI it drives must not disturb it. Before this,
    /// the Item became the panel's content, `build_element` demanded a string tag,
    /// and the whole panel failed to draw.
    #[test]
    fn an_item_inside_a_panel_leaves_the_panel_alone() {
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
                  <panel id="bar" anchor="top" width={100} height={20}>
                    <Light entity="light.desk" state="on" />
                    <div>hello</div>
                  </panel>
                </root>
              );
            }"#,
            serde_json::Value::Null,
            None,
        )
        .unwrap();
        let layout = evaluator.eval(&HashMap::new()).unwrap().layout;
        let surfaces = crate::parse_root_node(&layout).unwrap();

        assert_eq!(
            surfaces[0].content["type"], "div",
            "the Item is gone; the panel's real content is its first child: {}",
            surfaces[0].content
        );
        crate::parse_layout(&surfaces[0].content)
            .expect("a panel holding an Item must still lay out");
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
        // Short interval so the loop's cadence is what the test measures, not the
        // five-second default.
        let source = light_layout(&state, &log, "refreshInterval: 50,");

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
        let evaluator = JsxEvaluator::new_reconciler(
            &source,
            serde_json::Value::Null,
            None,
            Default::default(),
        )
        .unwrap();

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

          enterOne: (light) => apply(light),
          updateOne: (light) => apply(light),
        });

        function apply(light) {
          hass(`/api/services/light/turn_${light.state === "on" ? "on" : "off"}`, {
            entity_id: light.entity,
          });
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
          update: (pairs) =>
            pairs.forEach(({ item }) => sh`printf 'update %s\n' ${item.entity} >> __LOG__`),
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
