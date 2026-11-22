//! Evaluating a layout file: JSX in, a JSON tree out.
//!
//! Two phases, at very different rates:
//!
//! 1. **On load or change of the layout file.** `optative-script` transforms the JSX —
//!    turning tags into `_jsx(...)` calls — and QuickJS declares the result as an ES
//!    module. The module's *default export* is the render function, which is saved as a
//!    [`rquickjs::Persistent`] and reused. A layout file that does not export one fails
//!    here with a type error about `undefined` not being a function.
//! 2. **On every data tick.** Stream values are written into a shared map and the saved
//!    render function is called. No reparse, no recompile. It returns a JS object tree
//!    that Rust walks to extract surfaces and build takumi nodes.
//!
//! The `Runtime` and `Context` are created once and live forever, which is what keeps the
//! second phase to about 2ms — small enough that re-rendering everything on every tick is
//! affordable (ADR 0007). The transform is under a millisecond and happens only in the
//! first phase. Rasterization still dominates both, by roughly an order of magnitude.
//!
//! `_jsx` is registered from Rust as a global. It takes `(tag, props, ...children)` and
//! returns `{ type, ...props, children }` — a plain object, no intermediate
//! representation.
//!
//! Everything a layout file can reach is registered here — in `JSX_GLOBALS_JS` and in the
//! setup below: `useStringStream`, `useJSONStream`, `useDispatch`, `useService`, `I3Layout`,
//! `Panel`, `themeColor`, `measureText` and `ctx`, and `store` as the one export of the
//! module `"tauler"`. `useEvents` and `Module` are the deprecated names of `useDispatch`
//! and of `useService` as a render prop: the same implementation, kept for layout files
//! not yet migrated. rquickjs grants nothing by default, so that list is exhaustive by
//! construction rather than by audit (ADR 0008).
//!
//! Each module also gets its own `import.meta.dirname`, `.filename` and `.url`: the
//! layout file when it is declared, every import in `SchemaAwareLoader`. They are set
//! from the canonical path, so a layout and its imports agree through a symlink, and a
//! Package's point into its pinned checkout. With no layout path they stay `undefined`.

use std::collections::HashMap;

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use tauler_core::flatten::flatten_passthrough;
use tauler_core::globals::{JSX_GLOBALS_JS, TAULER_MODULE, TAULER_MODULE_JS};

/// Shared map of stream values: keyed by `(bin, script)`, holds the latest stdout line.
type StreamValues = Arc<RwLock<HashMap<(String, Option<String>), String>>>;
/// Recorded `useStringStream` calls made during the last render invocation.
type StreamCalls = Arc<Mutex<Vec<(String, Option<String>)>>>;

/// Return type of a successful JSX evaluation.
#[derive(Clone)]
pub struct EvalOutput {
    pub layout: serde_json::Value,
    pub stream_calls: Vec<(String, Option<String>)>,
    pub module_calls: Vec<(String, serde_json::Value)>,
    /// One per `<D3Figure>` in this eval's tree, in document order
    /// ([`figures_in`]): the draw requests the loop hands to the draw runtime
    /// (see `crate::d3_figure`).
    pub figure_calls: Vec<FigureCall>,
    /// What this eval ran with: see [`EvalInputs`].
    pub inputs: Arc<EvalInputs>,
}

/// The inputs an eval ran with, so the draw runtime can evaluate the same render
/// again: every stream value, since a figure's `draw` may read streams the render
/// did not. A tree with no `<D3Figure>` keeps no stream values, since nothing
/// draws from its snapshot. `ctx` is not here: every runtime is built from the same
/// one. Neither are the Stores: the draw runtime reads the app's store as it is when
/// it evaluates (the last `RISK:` in `src/state.rs`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EvalInputs {
    pub stream_values: HashMap<(String, Option<String>), String>,
}

/// A `<D3Figure>` in an evaluated tree: the Picture it names and the size to
/// draw it at. Its `draw` function stays behind in the runtime that built it.
#[derive(Debug, Clone, PartialEq)]
pub struct FigureCall {
    /// The Picture's name: the figure's key and size, `cpu@248x100`.
    pub key: String,
    pub width: f64,
    pub height: f64,
}

/// The `<D3Figure>`s an evaluated tree shows, in document order: every `img`
/// whose `src` names a Picture and whose style gives a numeric size, which is
/// what the `D3Figure` global returns (`tauler-core/src/globals.rs`).
///
/// Read from the tree rather than recorded as `D3Figure` is called, because an
/// element can be built before the render that returns it — at module scope,
/// once, before the first eval.
fn figures_in(layout: &serde_json::Value) -> Vec<FigureCall> {
    crate::render::picture::named_pictures(layout)
        .into_iter()
        .filter_map(|named| {
            let (width, height) = named.size?;
            Some(FigureCall {
                key: named.key.to_string(),
                width,
                height,
            })
        })
        .collect()
}

use crate::d3_figure::draw::DrawError;
use rquickjs::function::Function;
use rquickjs::loader::{Loader, Resolver};
use rquickjs::{CatchResultExt, CaughtError, Persistent};

/// What a layout file did wrong, in its own words.
///
/// rquickjs's `Error::Exception` carries no data — its Display is "Exception
/// generated by QuickJS" — and the thrown value is only readable while the `Ctx`
/// that threw it is alive. So the message is captured at the `context.with`
/// boundary and carried out as text; past that point nothing else survives.
#[derive(Debug)]
pub struct JsError(String);

impl std::fmt::Display for JsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for JsError {}

/// Read the thrown value (message and stack) while `ctx` still holds it.
fn describe(ctx: &rquickjs::Ctx<'_>, err: rquickjs::Error) -> JsError {
    JsError(CaughtError::from_error(ctx, err).to_string())
}

/// Run `f` in `context`; a failure comes out described, because after this returns
/// the `Ctx` is gone and so is the thrown value.
fn with_described<T>(
    context: &rquickjs::Context,
    f: impl for<'js> FnOnce(rquickjs::Ctx<'js>) -> rquickjs::Result<T>,
) -> Result<T, JsError> {
    context.with(|ctx| f(ctx.clone()).map_err(|e| describe(&ctx, e)))
}

use std::path::PathBuf;

/// The live `unit()` object at `unit_index` in this runtime's `__tauler_units`.
///
/// Every Unit lookup goes through here so there is one place that knows where the
/// array lives and what an out-of-range index means (nothing to do).
fn unit_at<'js>(qjs_ctx: &rquickjs::Ctx<'js>, unit_index: usize) -> Option<rquickjs::Object<'js>> {
    let units: rquickjs::Array<'js> = qjs_ctx.globals().get("__tauler_units").ok()?;
    units.get(unit_index).ok()
}

/// A JS value as plain JSON, or `None` if it does not survive the trip
/// (`undefined`, a bare function, a cycle).
fn json_of<'js>(
    qjs_ctx: &rquickjs::Ctx<'js>,
    value: rquickjs::Value<'js>,
) -> Option<serde_json::Value> {
    let json = qjs_ctx.json_stringify(value).ok()??.to_string().ok()?;
    serde_json::from_str(&json).ok()
}

/// The `<Workspaces>` shim's native half: measure `wrapper` and slice it into edge
/// panels. A plain `fn`, not a closure, because `rquickjs::Function::new` wants an
/// `impl for<'js> Fn(...)` and a closure literal can't express that HRTB — the same
/// reason `UiComponent::js_fn` (`tauler-core/src/ui/mod.rs`) is a named fn too.
///
/// `misplaced` is `true` when the `I3Layout` shim found more than one `<Workspaces>`,
/// or one that wasn't the last child — reported here rather than from JS because
/// there is no `console` in this runtime (ADR 0008's globals list is exhaustive by
/// construction, and it isn't on it). The layout still degrades to "use the last one
/// declared" rather than failing the whole render — the same "a mistake never takes
/// the bar down" rule `i3_layout` uses for an unknown anchor — but a misuse this
/// specific (the issue names it a hard constraint) is worth a warning, not silence.
fn workspaces_layout_js_fn<'js>(
    ctx: rquickjs::Ctx<'js>,
    wrapper: rquickjs::Value<'js>,
    width: u32,
    height: u32,
    misplaced: bool,
) -> rquickjs::Result<rquickjs::Value<'js>> {
    if misplaced {
        tracing::warn!(
            "<Workspaces> may only be used once, as the last child of <I3Layout> — \
             using the last one declared, ignoring the rest"
        );
    }
    let wrapper = json_of(&ctx, wrapper).unwrap_or_default();
    let layout = crate::workspaces::lay_out("workspaces", &wrapper, width, height);
    rquickjs_serde::to_value(ctx, layout).map_err(|_| rquickjs::Error::Unknown)
}

/// Renders a minijinja template against arbitrary JSON-serialisable data — the pure
/// half of what a `tauler-configgen`-generated root component does with its own
/// schema's template: validated Item data in, config text out.
/// A syntax error in `source`, or a render-time error (an undefined filter, say),
/// throws with minijinja's own message rather than panicking — matching every other
/// builtin's error convention in this codebase.
fn render_template_js_fn<'js>(
    ctx: rquickjs::Ctx<'js>,
    source: String,
    data: rquickjs::Value<'js>,
) -> rquickjs::Result<String> {
    let data: serde_json::Value = json_of(&ctx, data).unwrap_or_default();
    let mut env = minijinja::Environment::new();
    env.add_template("_", &source)
        .map_err(|e| rquickjs::Exception::throw_message(&ctx, &format!("renderTemplate: {e}")))?;
    env.get_template("_")
        .and_then(|tmpl| tmpl.render(minijinja::value::Serde(data)))
        .map_err(|e| rquickjs::Exception::throw_message(&ctx, &format!("renderTemplate: {e}")))
}

/// The `warn` a layout file can call when it has something worth surfacing but no error
/// to throw. There is deliberately no `console` in this runtime (see `JSX_GLOBALS_JS`'s
/// `<I3Layout>` comment, the only prior place this need came up): every warning crosses
/// into Rust explicitly, through a builtin like this one, rather than a JS-side no-op that
/// looks like it does something. Registered unconditionally, like `renderTemplate` — it
/// touches nothing in the world, so ADR 0034's frame-budget reasoning for keeping `sh` out
/// of the render runtime doesn't apply here either.
///
/// Not currently called by any builtin global — `ConfigFile` has no `exitOne` to call it
/// from (a dropped `<ConfigFile/>` currently has no hook that fires at all; see
/// `globals.rs`'s comment on `ConfigFile` and `docs/units.md`'s exit-limit section). Kept
/// as tested, working JS-to-`tracing::warn!` infrastructure for the next caller that needs
/// it — e.g. the hand-edit-detection sidecar sketched for `ConfigFile` — rather than
/// removed and rebuilt from scratch when one arrives.
fn warn_js_fn(message: String) {
    tracing::warn!("{message}");
}

/// `__tauler_theme_color`, behind `themeColor` (`JSX_GLOBALS_JS`). The colours come from JS
/// (`ctx.theme.colors`) rather than being captured here, so it reads the theme exactly as
/// `ctx.theme` does.
fn theme_color_js_fn(
    colors: std::collections::HashMap<String, String>,
    token: String,
    alpha: rquickjs::function::Opt<Option<f64>>,
) -> String {
    tauler_core::theme::resolver::theme_color(&colors, &token, alpha.0.flatten())
}

/// `__tauler_measure_text`, behind `measureText` (`JSX_GLOBALS_JS`), which checks the
/// arguments and rounds the answer: `[width, ascent, descent]` in the bar's fonts, or
/// `null` for a family that is not a CSS family list, which `measureText` throws for.
fn measure_text_js_fn(
    ctx: rquickjs::Ctx<'_>,
    text: String,
    family: String,
    size: f64,
    weight: f64,
    style: String,
) -> rquickjs::Result<Option<Vec<f64>>> {
    let font = crate::render::measure::TextFont {
        family,
        size,
        weight,
        style,
    };
    let m = crate::render::measure::measure_with_bar_fonts(&text, &font).ok_or_else(|| {
        rquickjs::Exception::throw_message(&ctx, "measureText: no fonts are loaded yet")
    })?;
    Ok(m.ok().map(|m| vec![m.width, m.ascent, m.descent]))
}

/// Dispatches a lifecycle hook, picking the batch spelling or the per-Item sugar
/// by whichever one the Unit defined.
///
/// The array handed to a batch hook is wrapped in a `Proxy` that throws on any
/// property an array does not have. Without it, a hook written per-Item and handed
/// a batch reads `undefined` off an `Array` and runs a command with a missing
/// argument — no throw, no warning, wrong result. With it, the mistake names its
/// own fix.
fn dispatch_hook_source() -> &'static str {
    r#"
    globalThis.__tauler_one_item_guard = (arr, hook, one) =>
      new Proxy(arr, {
        get(target, prop) {
          if (typeof prop === "symbol" || prop in target) return Reflect.get(target, prop);
          throw new TypeError(
            "`" + hook + "` receives an array of Items, not one Item. " +
            "Did you mean `" + one + "`?"
          );
        },
      });

    globalThis.__tauler_dispatch_hook = (unitIndex, hook, one, payload) => {
      const unit = globalThis.__tauler_units[unitIndex];
      if (!unit) return 0;
      const batchFn = unit[hook];
      const oneFn = unit[one];
      if (batchFn && oneFn) {
        throw new TypeError(
          "a unit() must define `" + hook + "` or `" + one + "`. " +
          "It cannot define both."
        );
      }
      if (oneFn) {
        for (const entry of payload) {
          if (hook === "update") oneFn(entry.item, entry.old);
          else oneFn(entry);
        }
        return payload.length;
      }
      if (!batchFn) return 0;
      batchFn(globalThis.__tauler_one_item_guard(payload, hook, one));
      return payload.length;
    };
    "#
}

/// Groups the Items in an evaluated tree by the Unit object that declared them,
/// parking those objects in `__tauler_units` for later hook calls. Unit identity
/// is the object itself — two `<Light/>` elements share one because `unit()` ran
/// once — so no id or name has to be agreed on.
fn collect_units_source() -> String {
    format!(
        r#"
        globalThis.__tauler_collect_units = (tree) => {{
          const units = [];
          const batches = [];
          const walk = (node) => {{
            if (node === null || typeof node !== "object") return;
            const t = node.type;
            if (t && typeof t === "object" && t.{kind} === true) {{
              let i = units.indexOf(t);
              if (i < 0) {{
                i = units.length;
                units.push(t);
                batches.push({{ unit_index: i, items: [] }});
              }}
              const props = {{}};
              for (const k of Object.keys(node)) {{
                if (k !== "type" && k !== "children") props[k] = node[k];
              }}
              batches[i].items.push(props);
            }}
            if (Array.isArray(node.children)) node.children.forEach(walk);
          }};
          walk(tree);
          globalThis.__tauler_units = units;
          return batches;
        }};
        "#,
        kind = optative_script::tags::ESTO_KIND,
    )
}

/// Fold a later module registration's props into the ones already recorded.
///
/// Additive: a key already declared is kept, so a later registration can only
/// fill in what an earlier one left out. That ordering matters because children
/// are evaluated before their parent — a wrapper that derives props from its
/// children (layout geometry, say) can only register after them, and must not be
/// able to clobber what the author wrote by hand.
fn merge_missing(existing: &mut serde_json::Value, incoming: serde_json::Value) {
    let (Some(target), serde_json::Value::Object(source)) = (existing.as_object_mut(), incoming)
    else {
        return;
    };
    for (k, v) in source {
        target.entry(k).or_insert(v);
    }
}

/// The id that reaches the handler a press captured rather than one in this tick's
/// registry. Outside the range the registry issues, which only counts upward.
const CAPTURED_HANDLER: i64 = -1;

/// Handler ids already reported as throwing, so a failing mapper reports once rather
/// than once per motion event.
static WARNED_HANDLERS: std::sync::Mutex<Option<std::collections::HashSet<i64>>> =
    std::sync::Mutex::new(None);

fn warn_handler_once(id: i64, error: &impl std::fmt::Display) {
    let mut warned = WARNED_HANDLERS.lock().unwrap();
    let warned = warned.get_or_insert_with(std::collections::HashSet::new);
    if warned.insert(id) {
        tracing::error!(exception = %error, handler = id, "handler raised; it dispatches nothing");
    }
}

/// A JSON value as a JS one. A number, string, boolean or null is built directly; only an
/// object or array goes through serde.
fn js_of_state<'js>(
    ctx: &rquickjs::Ctx<'js>,
    value: &serde_json::Value,
) -> rquickjs::Result<rquickjs::Value<'js>> {
    use rquickjs::IntoJs;
    use serde_json::Value as J;
    match value {
        J::Null => Ok(rquickjs::Value::new_null(ctx.clone())),
        J::Bool(b) => Ok(rquickjs::Value::new_bool(ctx.clone(), *b)),
        J::Number(n) => match n.as_i64().and_then(|i| i32::try_from(i).ok()) {
            Some(i) => Ok(rquickjs::Value::new_int(ctx.clone(), i)),
            None => Ok(rquickjs::Value::new_float(
                ctx.clone(),
                n.as_f64().unwrap_or(f64::NAN),
            )),
        },
        J::String(s) => s.as_str().into_js(ctx),
        J::Array(_) | J::Object(_) => {
            rquickjs_serde::to_value(ctx.clone(), value).map_err(|_| rquickjs::Error::Unknown)
        }
    }
}

/// A JS value as a JSON one; `None` for `undefined`. The reverse of [`js_of_state`].
fn state_of_js<'js>(
    ctx: &rquickjs::Ctx<'js>,
    value: rquickjs::Value<'js>,
) -> rquickjs::Result<Option<serde_json::Value>> {
    use serde_json::Value as J;
    if value.is_undefined() {
        return Ok(None);
    }
    if value.is_null() {
        return Ok(Some(J::Null));
    }
    if let Some(b) = value.as_bool() {
        return Ok(Some(J::Bool(b)));
    }
    if let Some(i) = value.as_int() {
        return Ok(Some(J::from(i)));
    }
    if let Some(s) = value.as_string() {
        return Ok(Some(J::String(s.to_string()?)));
    }
    rquickjs_serde::from_value(value).map(Some).map_err(|e| {
        rquickjs::Exception::throw_type(ctx, &format!("state holds only JSON values: {e}"))
    })
}

/// What one runtime may do to the Stores it is handed (issue #271).
///
/// DECISION: the reconciler and draw runtimes are refused by the host functions, not by
/// the Proxy in `JSX_GLOBALS_JS`. `__tauler_state_set` throws there, and
/// `__tauler_state_declare` stores nothing, so a default never replaces or fills what
/// the render runtime saved. Reason: every write a layout file can make ends in
/// `__tauler_state_set` — an assignment, a `delete`, and a nested value changed in place
/// (the flush after a render) — so one check covers all of them, and the Proxy stays
/// the same string in every runtime and on a page. Rejected: a read-only Proxy for
/// these runtimes — a nested in-place write never passes through a Proxy trap.
///
/// DECISION: in those two runtimes a write made by the layout's own evaluation (its
/// module scope or its render) is not refused. `__tauler_state_set` stores nothing and
/// answers [`UNSAVED`]; the Proxy keeps the value in its mirror, so the rest of that
/// evaluation reads it, and `__tauler_state_drop_unsaved` empties those mirrors when
/// the evaluation ends ([`JsxEvaluator::end_evaluation`]), so the next one starts from
/// the store. A write from a Unit hook or from `draw` code still throws: both run
/// after the evaluation ended. Reason: these runtimes run the same render the render
/// runtime does, and a render that writes a Store is allowed there (workspace memory,
/// a scroll clamp); throwing here left that layout's Units unreconciled and its
/// D3Figures undrawn. Tests: `store_render_time_write_in_the_reconciler_runtime_*`,
/// `store_render_time_write_in_the_draw_runtime_*`,
/// `store_module_scope_write_in_the_reconciler_runtime_loads_and_stores_nothing`,
/// `store_write_from_a_unit_hook_changes_nothing` and
/// `store_written_by_a_handler_is_read_by_the_draw_runtime_which_cannot_write_it`.
/// Rejected: a copy of the whole store for each such evaluation, written freely and
/// thrown away — exact with no JavaScript, and the per-evaluation copy issue #271
/// removes.
///
/// These runtimes also open no transaction: a store has one journal, and it belongs to
/// the render runtime's handler or render, which may be in the middle of one on another
/// thread.
#[derive(Clone, Copy, PartialEq)]
enum StateAccess {
    /// The render runtime: declares, reads and writes.
    ReadWrite,
    /// Reads only. `runtime` names who was refused, in the error a write throws.
    ReadOnly { runtime: &'static str },
}

impl StateAccess {
    const RECONCILER: Self = Self::ReadOnly {
        runtime: "the reconciler runtime, where Unit hooks run",
    };
    const DRAW: Self = Self::ReadOnly {
        runtime: "the draw runtime, where a D3Figure's draw runs",
    };
}

/// What the compile pass writes after every State identity, and what
/// `__tauler_state_declare` asks for (the `DECISION` in `tauler-core/src/store_pass.rs`): new for
/// every evaluator, and never handed to JavaScript except inside that rewritten call.
fn new_store_token() -> String {
    use std::hash::BuildHasher;
    // `RandomState` is seeded by the operating system once per process and differs
    // per instance; no random-number crate is a dependency of this one.
    format!(
        "{:016x}",
        std::collections::hash_map::RandomState::new().hash_one(0u8)
    )
}

/// What `__tauler_state_set` answers in place of a version when it stored nothing
/// ([`StateAccess`]); `JSX_GLOBALS_JS` tests for "below zero".
const UNSAVED: f64 = -1.0;

/// The version a runtime that only reads answers for a key it reads from a default
/// ([`Defaults`]). No stored key has it: the store's clock starts above zero.
const DEFAULT_VERSION: f64 = -2.0;

/// The default of every Store one runtime declared and did not store: what a runtime
/// that only reads ([`StateAccess`]) answers while the store holds nothing under an
/// identity. Empty in the render runtime, which stores every default.
///
/// DECISION: a runtime that only reads keeps each declaration's default here, beside
/// its host functions, and reads from it only while the store holds no state at all
/// under that identity. Reason: a local Store is declared to the store by the render
/// runtime's first render that reaches it, and a reconciler or draw evaluation can
/// reach the declaration first (its Streams are newer); it then read `undefined` for
/// every key, and `s.items.length` threw. The rule is "no state held", not "this key
/// is missing": a key the render runtime deleted must stay deleted here too. Nothing
/// is written to the store, and the store's clock moves when the render runtime
/// declares, which empties the runtime's mirror, so the next read is the stored value.
/// A default is forgotten when this runtime's render stops declaring the Store
/// (`__tauler_state_drop`). Test:
/// `local_store_a_read_only_runtime_reads_the_default_until_the_store_holds_a_value`.
/// A handler or render that throws forgets the defaults it added
/// ([`Defaults::forget_added`]): that drop visits only what the last render that
/// returned declared. Test:
/// `local_store_a_read_only_runtime_keeps_no_default_from_a_render_that_threw`.
/// Rejected: storing the default from these runtimes — the store has one journal, the
/// render runtime's, and its drop rule would not know the entry. A page needs none of
/// this: it has one runtime, which stores every default (`tauler-web/js/state.js`).
#[derive(Default)]
struct Defaults(std::sync::Mutex<KeptDefaults>);

#[derive(Default)]
struct KeptDefaults {
    by_identity: HashMap<String, serde_json::Map<String, serde_json::Value>>,
    /// The identities that got their first default since [`Defaults::begin`].
    added: Vec<String>,
}

impl Defaults {
    /// The identities that have a default, sorted.
    #[cfg(test)]
    fn identities(&self) -> Vec<String> {
        let kept = self.0.lock().unwrap();
        let mut identities: Vec<String> = kept.by_identity.keys().cloned().collect();
        identities.sort();
        identities
    }

    fn insert(&self, identity: String, default: serde_json::Map<String, serde_json::Value>) {
        let mut kept = self.0.lock().unwrap();
        if kept.by_identity.insert(identity.clone(), default).is_none() {
            kept.added.push(identity);
        }
    }

    fn remove(&self, identity: &str) {
        self.0.lock().unwrap().by_identity.remove(identity);
    }

    /// One handler or one render starts: every default kept so far stays.
    fn begin(&self) {
        self.0.lock().unwrap().added.clear();
    }

    /// The handler or render threw: forget the defaults it added.
    fn forget_added(&self) {
        let mut kept = self.0.lock().unwrap();
        for identity in std::mem::take(&mut kept.added) {
            kept.by_identity.remove(&identity);
        }
    }

    /// `key` of `identity`'s default, when the store holds nothing under `identity`.
    fn get(
        &self,
        store: &crate::state::StateStore,
        identity: &str,
        key: &str,
    ) -> Option<serde_json::Value> {
        if store.holds(identity) {
            return None;
        }
        let kept = self.0.lock().unwrap();
        kept.by_identity.get(identity)?.get(key).cloned()
    }

    /// The same for the sorted keys of the default.
    fn keys(&self, store: &crate::state::StateStore, identity: &str) -> Option<Vec<String>> {
        if store.holds(identity) {
            return None;
        }
        let kept = self.0.lock().unwrap();
        let mut keys: Vec<String> = kept.by_identity.get(identity)?.keys().cloned().collect();
        keys.sort();
        Some(keys)
    }
}

/// The host half of `store(...)` (`JSX_GLOBALS_JS` documents the host functions).
fn register_state_host<'js>(
    ctx: &rquickjs::Ctx<'js>,
    state: &crate::state::SharedState,
    access: StateAccess,
    token: &str,
    evaluating: &Arc<AtomicBool>,
    defaults: &Arc<Defaults>,
) -> rquickjs::Result<()> {
    use rquickjs::{Ctx, Function, Value};
    let globals = ctx.globals();
    let defaults = Arc::clone(defaults);

    let store = Arc::clone(state);
    let kept = Arc::clone(&defaults);
    let expected = token.to_string();
    globals.set(
        "__tauler_state_declare",
        Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>,
                  identity: String,
                  default: Value<'js>,
                  token: rquickjs::function::Opt<Value<'js>>|
                  -> rquickjs::Result<()> {
                let written_by_the_pass = token
                    .0
                    .and_then(|token| token.as_string().and_then(|t| t.to_string().ok()))
                    .is_some_and(|token| token == expected);
                if !written_by_the_pass {
                    return Err(rquickjs::Exception::throw_type(
                        &ctx,
                        "store() takes exactly one argument, the default value, and must be \
                         the whole value of a const: `const name = store({...})`; \
                         tauler adds the identity",
                    ));
                }
                let default: serde_json::Map<String, serde_json::Value> =
                    rquickjs_serde::from_value(default).map_err(|e| {
                        rquickjs::Exception::throw_type(
                            &ctx,
                            &format!("store(): the default must be a JSON object: {e}"),
                        )
                    })?;
                if access == StateAccess::ReadWrite {
                    store.lock().unwrap().declare(&identity, default);
                } else {
                    kept.insert(identity, default);
                }
                Ok(())
            },
        )?,
    )?;

    let store = Arc::clone(state);
    let kept = Arc::clone(&defaults);
    globals.set(
        "__tauler_state_get",
        Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>,
                  identity: String,
                  key: String,
                  have: f64,
                  same: Value<'js>|
                  -> rquickjs::Result<Value<'js>> {
                let store = store.lock().unwrap();
                match store.get(&identity, &key) {
                    None => match kept.get(&store, &identity, &key) {
                        None => Ok(Value::new_undefined(ctx)),
                        Some(value)
                            if (value.is_object() || value.is_array())
                                && have == DEFAULT_VERSION =>
                        {
                            Ok(same)
                        }
                        Some(value) => js_of_state(&ctx, &value),
                    },
                    Some((value, version))
                        if (value.is_object() || value.is_array()) && version as f64 == have =>
                    {
                        Ok(same)
                    }
                    Some((value, _)) => js_of_state(&ctx, value),
                }
            },
        )?,
    )?;

    let store = Arc::clone(state);
    let kept = Arc::clone(&defaults);
    globals.set(
        "__tauler_state_version",
        Function::new(ctx.clone(), move |identity: String, key: String| -> f64 {
            let store = store.lock().unwrap();
            match store.version(&identity, &key) {
                0 if kept.get(&store, &identity, &key).is_some() => DEFAULT_VERSION,
                version => version as f64,
            }
        })?,
    )?;

    let store = Arc::clone(state);
    let evaluating = Arc::clone(evaluating);
    globals.set(
        "__tauler_state_set",
        Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>,
                  identity: String,
                  key: String,
                  value: Value<'js>|
                  -> rquickjs::Result<f64> {
                if let StateAccess::ReadOnly { runtime } = access {
                    if evaluating.load(Ordering::Relaxed) {
                        return Ok(UNSAVED);
                    }
                    return Err(rquickjs::Exception::throw_type(
                        &ctx,
                        &format!(
                            "a Store cannot be written from {runtime}: \
                             `{identity}` key `{key}`"
                        ),
                    ));
                }
                let value = state_of_js(&ctx, value)?;
                let mut store = store.lock().unwrap();
                match value {
                    Some(value) => store.set(&identity, &key, value),
                    None => store.remove(&identity, &key),
                };
                Ok(store.version(&identity, &key) as f64)
            },
        )?,
    )?;

    // A local Store the render that just returned did not declare
    // (`__tauler_local_settle` in `tauler-core/src/local_store.rs`). Only the render
    // runtime decides: the other two never drop ([`StateAccess`]); they forget the
    // default they kept for it ([`Defaults`]).
    let store = Arc::clone(state);
    let kept = Arc::clone(&defaults);
    globals.set(
        "__tauler_state_drop",
        Function::new(ctx.clone(), move |identity: String| {
            if access == StateAccess::ReadWrite {
                store.lock().unwrap().drop_identity(&identity);
            } else {
                kept.remove(&identity);
            }
        })?,
    )?;

    let store = Arc::clone(state);
    globals.set(
        "__tauler_state_keys",
        Function::new(ctx.clone(), move |identity: String| -> Vec<String> {
            let store = store.lock().unwrap();
            defaults
                .keys(&store, &identity)
                .unwrap_or_else(|| store.keys(&identity))
        })?,
    )?;
    Ok(())
}

/// What calling a handler did (see [`JsxEvaluator::invoke_handler_outcome`]).
#[derive(Debug, Default, PartialEq)]
pub struct HandlerOutcome {
    /// The intents it returned, if any.
    pub intents: Option<serde_json::Value>,
    /// Whether any Store changed. A handler that threw changed none.
    pub state_changed: bool,
}

/// A persistent JSX evaluator that compiles the layout source once and re-evaluates
/// cheaply on each tick by calling the pre-compiled render function.
/// The owner of every Store's value, above any evaluator (issue #271): the one
/// [`crate::state::StateStore`]. Every constructor but [`JsxEvaluator::new`] takes one.
pub use crate::state::SharedState;

pub struct JsxEvaluator {
    context: rquickjs::Context,
    runtime: rquickjs::Runtime,
    stream_values: StreamValues,
    calls: StreamCalls,
    module_calls: Arc<Mutex<Vec<(String, serde_json::Value)>>>,
    /// What `store(...)` in the layout file reads and writes (`crate::state`): the
    /// caller's, except for [`Self::new`], which builds its own.
    state: SharedState,
    state_access: StateAccess,
    /// Whether the layout's own code is running: its module scope while the evaluator
    /// is built, its render during an eval. What a runtime that only reads does with a
    /// write depends on it ([`StateAccess`]).
    evaluating: Arc<AtomicBool>,
    /// The defaults a runtime that only reads keeps ([`Defaults`]).
    defaults: Arc<Defaults>,
    /// Always `Some` after construction; `None` only transiently during `drop`.
    render_fn: Option<Persistent<Function<'static>>>,
    loaded_paths: Arc<Mutex<Vec<PathBuf>>>,
    /// `Some` only for the draw runtime ([`JsxEvaluator::new_draw`]). It also
    /// changes [`JsxEvaluator::eval`]: figures its tree does not show are
    /// forgotten.
    draw: Option<DrawState>,
}

/// The draw runtime's own state. Atomics only so the evaluator stays `Sync`;
/// a draw runtime is used by one thread.
#[derive(Default)]
struct DrawState {
    /// d3 and linkedom are loaded into the context (see [`DRAW_BOOTSTRAP_JS`]).
    loaded: std::sync::atomic::AtomicBool,
    /// Bootstrap modules tried so far. Each attempt declares a fresh module name,
    /// because QuickJS keeps a module that failed to link under its name.
    attempts: std::sync::atomic::AtomicU32,
}

impl Drop for JsxEvaluator {
    fn drop(&mut self) {
        // Must restore (and drop) the Persistent<Function> inside context.with() before
        // the runtime is freed — otherwise QuickJS aborts with a GC assertion.
        if let Some(persistent_fn) = self.render_fn.take() {
            self.context.with(|ctx| {
                let _ = persistent_fn.restore(&ctx);
            });
        }
    }
}

/// Makes `import { store } from "tauler"` resolve, in every runtime and with or without
/// a filesystem: tried before the resolver and the loader of the layout's own imports.
/// The module's source is `TAULER_MODULE_JS`; the `DECISION` beside `__tauler_store` in
/// `tauler-core/src/globals.rs` says why it is source and not a native module.
struct TaulerModule;

impl Resolver for TaulerModule {
    fn resolve<'js>(
        &mut self,
        _ctx: &rquickjs::Ctx<'js>,
        base: &str,
        name: &str,
        _attrs: Option<rquickjs::loader::ImportAttributes<'js>>,
    ) -> rquickjs::Result<String> {
        if name == TAULER_MODULE {
            Ok(TAULER_MODULE.to_string())
        } else {
            Err(rquickjs::Error::new_resolving(base, name))
        }
    }
}

impl Loader for TaulerModule {
    fn load<'js>(
        &mut self,
        ctx: &rquickjs::Ctx<'js>,
        name: &str,
        _attrs: Option<rquickjs::loader::ImportAttributes<'js>>,
    ) -> rquickjs::Result<rquickjs::Module<'js, rquickjs::module::Declared>> {
        if name == TAULER_MODULE {
            rquickjs::Module::declare(ctx.clone(), TAULER_MODULE, TAULER_MODULE_JS)
        } else {
            Err(rquickjs::Error::new_loading(name))
        }
    }
}

/// The suffix every `tauler-configgen` schema file uses, here and in every schema under
/// `tauler-configgen/examples/`. The one place this convention is spelled out in tauler's
/// own code, not just followed by naming files consistently.
const SCHEMA_FILE_SUFFIX: &str = ".schema.yaml";

/// Wraps `ConfinedFsLoader`, intercepting `*.schema.yaml` imports before they'd
/// otherwise be handed to QuickJS as literal (invalid) source text: a
/// `tauler-configgen` schema is parsed and codegen'd into the same JS a hand-copied
/// `*.generated.jsx` file used to contain, right here at import time.
///
/// This exists so a layout file can `import { Rofi, Selector } from
/// './my-theme.schema.yaml'` directly — no `cargo run --example verify_*`, no `cp`
/// into a separate `*.generated.jsx` file, nothing to keep in sync by hand. tauler
/// already owns JS module resolution (that's what `ConfinedFsResolver`/
/// `ConfinedFsLoader` are), so there's nothing stopping it from treating a schema
/// file as a module source the same way it already treats `.jsx` as one via
/// `transform_source` — codegen *is* that transform, just for a different source
/// language. The existing import-watch autoreload (this session's earlier fix)
/// covers `.schema.yaml` files for free: they're pushed into the same
/// `loaded_paths` list any other import is, so editing a schema hot-reloads the
/// same way editing a `.jsx` component already does.
///
/// `ConfinedFsResolver` needs no matching change — confirmed by a standalone test
/// against the real `optative-script` crate before writing this: an import
/// specifier with an explicit extension (`'./x.schema.yaml'`, as opposed to an
/// extension-less `'./x'` relying on fallback resolution) resolves directly
/// against the real file, regardless of the resolver's configured extension list.
///
/// It also loads [`D3_MODULE_NAME`], the module `GitPackageResolver` resolves the
/// `"d3"` specifier to: [`crate::d3_figure::import::es_module_source`] over the d3
/// file `d3` supplies. `d3` is `None` exactly when `GitPackageResolver` has no
/// `PackageContext`, so nothing resolves to that name then.
struct SchemaAwareLoader {
    loaded_paths: Arc<Mutex<Vec<PathBuf>>>,
    d3: Option<Arc<crate::d3_figure::fetch::Fetcher>>,
    modules: ModuleNames,
    /// What the Store pass writes after each identity ([`new_store_token`]).
    store_token: String,
}

/// Names a loaded file the way a State identity spells it (issue #271): stable across
/// machines, checkouts and Package updates, so no absolute path and no commit.
struct ModuleNames {
    /// The layout file's directory, canonical — imports arrive canonical.
    layout_dir: PathBuf,
    /// Where Package checkouts live, canonical. `None` without Package support.
    cache_root: Option<PathBuf>,
}

impl ModuleNames {
    fn new(layout_dir: &Path, cache_root: Option<&Path>) -> Self {
        let canonical = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
        Self {
            layout_dir: canonical(layout_dir),
            cache_root: cache_root.map(canonical),
        }
    }

    /// `components/Desk.jsx` for a file under the layout's directory;
    /// `@gh/owner/repo/index.jsx` for a file in a Package's checkout
    /// (`<cache_root>/gh/<owner>/<repo>/<ref>/index.jsx`): the import specifier, then
    /// the path inside the Package, and never the ref.
    ///
    /// SPEC: updating a Package to a new commit keeps the state of every Store whose
    /// file and name did not move.
    fn name_of(&self, path: &Path) -> String {
        let joined = |parts: std::path::Components<'_>| {
            parts
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/")
        };
        let in_package = self
            .cache_root
            .as_ref()
            .and_then(|root| path.strip_prefix(root).ok())
            .and_then(|relative| {
                let mut parts = relative.components();
                let gh = parts.next()?.as_os_str().to_str()?;
                let owner = parts.next()?.as_os_str().to_str()?.to_owned();
                let repo = parts.next()?.as_os_str().to_str()?.to_owned();
                let _ref = parts.next()?;
                (gh == "gh").then(|| format!("@gh/{owner}/{repo}/{}", joined(parts)))
            });
        if let Some(name) = in_package {
            return name;
        }
        match path.strip_prefix(&self.layout_dir) {
            Ok(relative) => joined(relative.components()),
            // The resolver confines every import to one of the two roots above, so this
            // is unreachable today; the full path keeps identities distinct if it is not.
            Err(_) => path.to_string_lossy().into_owned(),
        }
    }
}

/// Runs the Store pass on one file; a refusal becomes a thrown `SyntaxError` that reads
/// `<module>:<line>: <what is wrong>`, which is what the load then fails with.
fn name_stores<'js>(
    ctx: &rquickjs::Ctx<'js>,
    source: &str,
    path: &str,
    module: &str,
    token: &str,
) -> rquickjs::Result<String> {
    tauler_core::store_pass::rewrite(source, path, module, token).map_err(|e| {
        rquickjs::Exception::throw_syntax(ctx, &format!("{module}:{}: {}", e.line, e.message))
    })
}

/// The reserved module name the `"d3"` specifier resolves to. Not a path, so it
/// can never collide with a file the layout imports.
const D3_MODULE_NAME: &str = crate::scheme::D3_MODULE;

/// The reserved module name (and specifier) for linkedom, imported only by the
/// draw runtime's bootstrap ([`DRAW_BOOTSTRAP_JS`]). Resolved and loaded exactly
/// like [`D3_MODULE_NAME`]; linkedom's file is already an ES module, so its
/// source is used as-is.
const LINKEDOM_MODULE_NAME: &str = crate::scheme::LINKEDOM_MODULE;

impl SchemaAwareLoader {
    fn new(
        loaded_paths: Arc<Mutex<Vec<PathBuf>>>,
        d3: Option<Arc<crate::d3_figure::fetch::Fetcher>>,
        modules: ModuleNames,
        store_token: String,
    ) -> Self {
        Self {
            loaded_paths,
            d3,
            modules,
            store_token,
        }
    }

    /// The `"d3"` module: the fetched UMD file wrapped as an ES module. Fails to
    /// load if the files went missing between resolve and load. Declared from
    /// generated source, the same way a `*.schema.yaml` import is.
    fn load_d3<'js>(
        &self,
        ctx: &rquickjs::Ctx<'js>,
    ) -> rquickjs::Result<rquickjs::Module<'js, rquickjs::module::Declared>> {
        let sources = self
            .d3
            .as_ref()
            .and_then(|fetcher| fetcher.sources())
            .ok_or_else(|| rquickjs::Error::new_loading(D3_MODULE_NAME))?;
        let source = crate::d3_figure::import::es_module_source(
            &sources.d3,
            &crate::d3_figure::import::d3_exports(),
        );
        rquickjs::Module::declare(ctx.clone(), D3_MODULE_NAME, source)
    }

    /// The [`LINKEDOM_MODULE_NAME`] module: the fetched linkedom file, as-is.
    fn load_linkedom<'js>(
        &self,
        ctx: &rquickjs::Ctx<'js>,
    ) -> rquickjs::Result<rquickjs::Module<'js, rquickjs::module::Declared>> {
        let sources = self
            .d3
            .as_ref()
            .and_then(|fetcher| fetcher.sources())
            .ok_or_else(|| rquickjs::Error::new_loading(LINKEDOM_MODULE_NAME))?;
        rquickjs::Module::declare(ctx.clone(), LINKEDOM_MODULE_NAME, sources.linkedom)
    }
}

impl Loader for SchemaAwareLoader {
    fn load<'js>(
        &mut self,
        ctx: &rquickjs::Ctx<'js>,
        name: &str,
        _attrs: Option<rquickjs::loader::ImportAttributes<'js>>,
    ) -> rquickjs::Result<rquickjs::Module<'js, rquickjs::module::Declared>> {
        if name == D3_MODULE_NAME {
            return self.load_d3(ctx);
        }
        if name == LINKEDOM_MODULE_NAME {
            return self.load_linkedom(ctx);
        }
        if !name.ends_with(SCHEMA_FILE_SUFFIX) {
            // QuickJS asks for an import only after the importer's source is declared,
            // so the Store pass runs on a file before any file it imports has been read.
            // The pass needs nothing from another file, and runs on a plain `.js`
            // import too, which gets no transform (`tauler-core/src/store_pass.rs`).
            // DECISION (issue #271): this branch is `ConfinedFsLoader::load` written out
            // (read, record the path, transform, declare) with the Store pass before the
            // transform. Reason: that loader transforms and declares in one call with no
            // hook between, but `transform_source` is public, so its five lines are
            // cheaper to own than a new `optative-script` release. Rejected: a
            // `transform` hook on `ConfinedFsLoader` — right eventually, not needed now.
            let source =
                std::fs::read_to_string(name).map_err(|_| rquickjs::Error::new_loading(name))?;
            self.loaded_paths.lock().unwrap().push(PathBuf::from(name));
            let module_name = self.modules.name_of(Path::new(name));
            let named = name_stores(ctx, &source, name, &module_name, &self.store_token)?;
            let source = if tauler_core::store_pass::is_script_file(name) {
                optative_script::jsx::transform_source(&named, name)
            } else {
                named
            };
            let module = rquickjs::Module::declare(ctx.clone(), name, source)?;
            set_import_meta(&module, Path::new(name))?;
            return Ok(module);
        }
        let source =
            std::fs::read_to_string(name).map_err(|_| rquickjs::Error::new_loading(name))?;
        let schema = tauler_configgen::parse(&source).map_err(|e| {
            tracing::error!(error = %e, path = name, "schema failed to parse");
            rquickjs::Error::new_loading(name)
        })?;
        let generated = tauler_configgen::generate(&schema);
        self.loaded_paths.lock().unwrap().push(PathBuf::from(name));
        let module = rquickjs::Module::declare(ctx.clone(), name, generated)?;
        set_import_meta(&module, Path::new(name))?;
        Ok(module)
    }
}

/// Gives a module its own `import.meta.dirname`, `.filename` and `.url`, the way Node,
/// Deno and Bun do, so a Package can name a program it ships next to its source.
///
/// Per module, not a global: a global cannot tell which file is asking. Only strings —
/// nothing here reads the filesystem or lets the render runtime touch it (ADR 0034).
fn set_import_meta<'js>(
    module: &rquickjs::Module<'js, rquickjs::module::Declared>,
    path: &Path,
) -> rquickjs::Result<()> {
    // Imports arrive already canonical from the resolver; the layout file does not.
    let path = &path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let meta = module.meta()?;
    if let Some(dir) = path.parent() {
        meta.set("dirname", dir.to_string_lossy().into_owned())?;
    }
    meta.set("filename", path.to_string_lossy().into_owned())?;
    if let Ok(url) = url::Url::from_file_path(path) {
        meta.set("url", url.to_string())?;
    }
    Ok(())
}

/// Intercepts `@gh/owner/repo` Package specifiers, delegating everything else to
/// `ConfinedFsResolver` — the same wrap-and-delegate shape `SchemaAwareLoader`
/// already establishes. `pkg_ctx` is `None` for every evaluator built without
/// package support (every existing caller of `JsxEvaluator::new`/`new_reconciler`
/// — a `@gh/...` import then fails to resolve exactly as it does today, since
/// nothing recognizes the prefix), and `Some` only for the two production call
/// sites that opt in via `new_with_packages`/`new_reconciler_with_packages`. See
/// ADR 0041's "Discovery lives in the resolver, not a separate scan step".
///
/// With a `pkg_ctx` it also resolves the bare specifier `"d3"`
/// ([`crate::d3_figure::import::SPECIFIER`]) to [`D3_MODULE_NAME`], which
/// `SchemaAwareLoader` loads. d3 is not a Package, but it fails the same way: no
/// files yet (`pkg_ctx.d3.sources()` is `None`) fails resolution this cycle, and
/// the Fetcher's own background download wakes the reload that retries it.
struct GitPackageResolver {
    inner: optative_script::loader::ConfinedFsResolver,
    pkg_ctx: Option<crate::pkg::PackageContext>,
}

impl GitPackageResolver {
    fn new(base_dir: PathBuf, pkg_ctx: Option<crate::pkg::PackageContext>) -> Self {
        Self {
            inner: optative_script::loader::ConfinedFsResolver::new(base_dir),
            pkg_ctx,
        }
    }

    /// Resolves an already-recognized `@gh/owner/repo` specifier.
    ///
    /// No Lockfile entry yet → this is the first time this layout file has
    /// imported it (req #1/#4): kick a fetch that pins whatever commit `HEAD` is
    /// right now and writes the new Lockfile entry, then fail resolution this
    /// cycle, same as any other missing import. A Lockfile entry but no checkout
    /// on disk yet (cold) → kick a fetch for the pinned commit (or the
    /// Development-mode ref), same failure. A checkout already on disk (warm —
    /// existence alone is a safe, sufficient check, since the atomic-rename write
    /// path in `pkg::git` guarantees a directory is only ever there once fully
    /// formed) → resolve the entry file inside it via a fresh
    /// `ConfinedFsResolver` scoped to that one checkout, reusing its extension
    /// fallback (`.js`/`.jsx`/`.ts`/`.tsx`) rather than reimplementing it.
    fn resolve_package<'js>(
        &self,
        qjs_ctx: &rquickjs::Ctx<'js>,
        base: &str,
        name: &str,
        owner: &str,
        repo: &str,
        pkg_ctx: &crate::pkg::PackageContext,
    ) -> rquickjs::Result<String> {
        let lockfile = crate::pkg::lockfile::Lockfile::load_from_path(&pkg_ctx.lockfile_path)
            .map_err(|_| rquickjs::Error::new_resolving(base, name))?;

        let Some(entry) = lockfile.get(owner, repo) else {
            pkg_ctx.fetch_manager.request_first_fetch(
                owner.to_string(),
                repo.to_string(),
                crate::pkg::remote_url(owner, repo),
                pkg_ctx.cache_root.clone(),
                pkg_ctx.lockfile_path.clone(),
            );
            return Err(rquickjs::Error::new_resolving(base, name));
        };

        let package_dir = crate::pkg::cache::package_cache_path(
            &pkg_ctx.cache_root,
            owner,
            repo,
            entry,
            &pkg_ctx.lockfile_path,
        )
        .map_err(|_| rquickjs::Error::new_resolving(base, name))?;

        if !package_dir.exists() {
            if entry.development {
                let r#ref = crate::pkg::cache::development_ref(&pkg_ctx.lockfile_path)
                    .map_err(|_| rquickjs::Error::new_resolving(base, name))?;
                // No commit to check out — that's the whole point of
                // Development mode — so this clones the default branch's
                // current tip, not `entry.commit` (which may be stale or
                // absent) and not the `development-<hash>` string above
                // (that's a cache-path key, never a real git ref).
                pkg_ctx.fetch_manager.request_development_fetch(
                    crate::pkg::fetch_manager::FetchKey {
                        owner: owner.to_string(),
                        repo: repo.to_string(),
                        r#ref,
                    },
                    crate::pkg::remote_url(owner, repo),
                    pkg_ctx.cache_root.clone(),
                    package_dir,
                );
            } else {
                pkg_ctx.fetch_manager.request_fetch(
                    crate::pkg::fetch_manager::FetchKey {
                        owner: owner.to_string(),
                        repo: repo.to_string(),
                        r#ref: entry.commit.clone(),
                    },
                    crate::pkg::remote_url(owner, repo),
                    pkg_ctx.cache_root.clone(),
                    package_dir,
                );
            }
            return Err(rquickjs::Error::new_resolving(base, name));
        }

        // A synthetic base whose parent is `package_dir` — `ConfinedFsResolver`
        // only ever reads `.parent()` off `base` to find where to resolve a
        // relative specifier from, never checks that the file itself exists.
        let synthetic_base = package_dir.join("__package_entry__");
        let mut package_resolver = optative_script::loader::ConfinedFsResolver::new(package_dir);
        package_resolver.resolve(qjs_ctx, &synthetic_base.to_string_lossy(), "./index", None)
    }
}

impl GitPackageResolver {
    /// Resolves the `"d3"` specifier: [`D3_MODULE_NAME`] when the Fetcher has
    /// both files, otherwise a resolution failure (which also starts its
    /// download, unless offline). Cold fails exactly like a cold Package (ADR
    /// 0041): the download's completion triggers the reload that retries it.
    fn resolve_d3(
        base: &str,
        name: &str,
        pkg_ctx: &crate::pkg::PackageContext,
    ) -> rquickjs::Result<String> {
        Self::resolve_fetched(base, name, D3_MODULE_NAME, pkg_ctx)
    }

    /// `module_name` when the Fetcher has both files, otherwise a resolution
    /// failure — the same rule for d3 and for [`LINKEDOM_MODULE_NAME`].
    fn resolve_fetched(
        base: &str,
        name: &str,
        module_name: &str,
        pkg_ctx: &crate::pkg::PackageContext,
    ) -> rquickjs::Result<String> {
        match pkg_ctx.d3.sources() {
            Some(_) => Ok(module_name.to_string()),
            None => Err(rquickjs::Error::new_resolving(base, name)),
        }
    }
}

/// If `base` — an already-resolved, canonicalized absolute path, since that's
/// all a `Resolver` is ever handed as `base` — lives inside a Package's
/// checkout under `cache_root`, returns that checkout's own root directory
/// (`<cache_root>/gh/<owner>/<repo>/<ref>`). Without this, a relative import
/// *inside* a Package's own source (`./components/Foo.jsx`, say) would fall
/// through to the layout's own `ConfinedFsResolver` and be rejected as
/// escaping its confinement root — a Package would only ever work if its
/// entire source fit in one file.
fn package_root_for(cache_root: &Path, base: &str) -> Option<PathBuf> {
    let canonical_cache_root = cache_root.canonicalize().ok()?;
    let relative = Path::new(base).strip_prefix(&canonical_cache_root).ok()?;
    let mut components = relative.components();
    let gh = components.next()?;
    if gh.as_os_str() != "gh" {
        return None;
    }
    let owner = components.next()?;
    let repo = components.next()?;
    let r#ref = components.next()?;
    Some(
        canonical_cache_root
            .join(gh)
            .join(owner)
            .join(repo)
            .join(r#ref),
    )
}

impl Resolver for GitPackageResolver {
    fn resolve<'js>(
        &mut self,
        ctx: &rquickjs::Ctx<'js>,
        base: &str,
        name: &str,
        attrs: Option<rquickjs::loader::ImportAttributes<'js>>,
    ) -> rquickjs::Result<String> {
        if let Some(pkg_ctx) = self.pkg_ctx.clone() {
            if name == crate::d3_figure::import::SPECIFIER {
                return Self::resolve_d3(base, name, &pkg_ctx);
            }
            if name == LINKEDOM_MODULE_NAME {
                return Self::resolve_fetched(base, name, LINKEDOM_MODULE_NAME, &pkg_ctx);
            }
            if let Some((owner, repo)) = crate::pkg::specifier::parse(name) {
                return self.resolve_package(ctx, base, name, owner, repo, &pkg_ctx);
            }
            if let Some(package_root) = package_root_for(&pkg_ctx.cache_root, base) {
                let mut package_resolver =
                    optative_script::loader::ConfinedFsResolver::new(package_root);
                return package_resolver.resolve(ctx, base, name, attrs);
            }
        }
        self.inner.resolve(ctx, base, name, attrs)
    }
}

/// Whether an evaluator's runtime may touch the world. The render runtime may not:
/// a Tick runs on the loop, and `sh` blocking it for the length of a subprocess is
/// what the latency budgets forbid. The reconciler runtime must, because that is
/// where hooks and `observe` run. See ADR 0034.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Effects {
    Denied,
    Allowed,
}

/// Installed by [`JsxEvaluator::new_draw`] before the layout module runs, so a
/// D3Figure built at module scope registers too: the figures Map
/// `registerFigure` fills (latest registration per key), the never-
/// firing timers (SPEC in `d3_figure/mod.rs`), and the function
/// [`JsxEvaluator::draw_figure`] calls, which hands `draw` the Picture's name
/// as a third argument, `{ name }`.
///
/// A draw that was interrupted leaves its half-built `<svg>` in the body, so
/// every draw empties the body first.
const DRAW_RUNTIME_JS: &str = r#"
    globalThis.__tauler_figures = new Map();
    globalThis.registerFigure = (key, width, height, draw) => {
        __tauler_figures.set(key, { draw, width, height });
    };
    if (typeof globalThis.setTimeout === "undefined") globalThis.setTimeout = () => 0;
    if (typeof globalThis.setInterval === "undefined") globalThis.setInterval = () => 0;
    if (typeof globalThis.requestAnimationFrame === "undefined") globalThis.requestAnimationFrame = () => 0;
    if (typeof globalThis.clearTimeout === "undefined") globalThis.clearTimeout = () => {};
    if (typeof globalThis.clearInterval === "undefined") globalThis.clearInterval = () => {};
    if (typeof globalThis.cancelAnimationFrame === "undefined") globalThis.cancelAnimationFrame = () => {};
    globalThis.__tauler_draw_figure = (key, w, h) => {
        const figure = __tauler_figures.get(key);
        const d3 = globalThis.__tauler_d3;
        const body = globalThis.__tauler_document.body;
        for (const child of [...body.childNodes]) body.removeChild(child);
        const svg = d3.select(body).append("svg")
            .attr("xmlns", "http://www.w3.org/2000/svg")
            .attr("width", w)
            .attr("height", h)
            .attr("viewBox", `0 0 ${w} ${h}`)
            .attr("font-family", "sans-serif");
        figure.draw(svg, d3, { name: key });
        const markup = svg.node().outerHTML;
        svg.remove();
        return markup;
    };
"#;

/// Loads d3 and linkedom into the draw runtime, on its first draw. Evaluated as
/// a module (under a fresh name per attempt) so it can `import` them.
const DRAW_BOOTSTRAP_JS: &str = r#"
    import * as d3 from "d3";
    import { parseHTML } from "tauler:linkedom";
    globalThis.__tauler_d3 = d3;
    globalThis.__tauler_document = parseHTML("<!doctype html><html><head></head><body></body></html>").document;
"#;

impl JsxEvaluator {
    /// The render runtime: evaluated every Tick, and unable to touch the world.
    ///
    /// `layout_path` is the layout file itself: relative imports resolve from its
    /// directory, and it is what the layout's `import.meta` reports. `None` means no
    /// filesystem at all — no imports, and an `import.meta` with nothing on it.
    ///
    /// Its Stores live in a store of its own, gone with the evaluator: for a caller
    /// that never rebuilds one and runs no other runtime (a screenshot, a test).
    /// Anything else owns a store and uses [`Self::new_with_state`].
    pub fn new(
        source: &str,
        ctx: serde_json::Value,
        layout_path: Option<&Path>,
    ) -> Result<Self, JsError> {
        Self::new_with_state(source, ctx, layout_path, SharedState::default())
    }

    /// The render runtime, reading and declaring its Stores in `state`, which the
    /// caller owns and keeps across reloads (issue #271).
    ///
    /// DECISION: the owner arrives through the constructor, not through a setter after
    /// it. Reason: a Store is
    /// declared while the layout module loads, inside the constructor, so a setter
    /// would be too late for the saved value to win over the default.
    pub fn new_with_state(
        source: &str,
        ctx: serde_json::Value,
        layout_path: Option<&Path>,
        state: SharedState,
    ) -> Result<Self, JsError> {
        Self::with_effects(
            source,
            ctx,
            layout_path,
            Effects::Denied,
            None,
            StateAccess::ReadWrite,
            state,
        )
    }

    /// The State identities this evaluator holds a declaration of: what its load
    /// declared, in declaration order, then the local Stores its last render that
    /// returned declared (`tauler-core/src/local_store.rs`).
    pub fn state_identities(&self) -> Vec<String> {
        self.context.with(|ctx| {
            ctx.eval::<Vec<String>, _>("globalThis.__tauler_state_declared()")
                .unwrap_or_default()
        })
    }

    /// The render runtime, with `@gh/owner/repo` Package imports resolved through
    /// `pkg_ctx`. Every other caller keeps using plain `new` — this exists only
    /// for the production call sites that actually construct a `PackageContext`
    /// (see ADR 0041), so a `@gh/...` import stays exactly as unresolvable as it
    /// always was for every existing test and call site that doesn't opt in.
    ///
    /// Public on purpose: the example binaries (`examples/figures`, `d3_charts_tracer`,
    /// `weather_map_tracer`) and outside tests may use it. Not semver-stable.
    pub fn new_with_packages(
        source: &str,
        ctx: serde_json::Value,
        layout_path: Option<&Path>,
        pkg_ctx: crate::pkg::PackageContext,
        state: SharedState,
    ) -> Result<Self, JsError> {
        Self::with_effects(
            source,
            ctx,
            layout_path,
            Effects::Denied,
            Some(pkg_ctx),
            StateAccess::ReadWrite,
            state,
        )
    }

    /// The [`crate::state::StateStore`] behind this evaluator's `store(...)` calls.
    pub fn state_handle(&self) -> SharedState {
        Arc::clone(&self.state)
    }

    /// The reconciler runtime: the same layout file, evaluated off the loop, with
    /// `sh` and friends registered so a Unit's hooks can run, and reading the bar's
    /// Stores. `state` is the store the render runtime was built with; here it is
    /// read only ([`StateAccess`]).
    pub fn new_reconciler(
        source: &str,
        ctx: serde_json::Value,
        layout_path: Option<&Path>,
        state: SharedState,
    ) -> Result<Self, JsError> {
        Self::with_effects(
            source,
            ctx,
            layout_path,
            Effects::Allowed,
            None,
            StateAccess::RECONCILER,
            state,
        )
    }

    /// The reconciler runtime, with `@gh/owner/repo` Package imports resolved
    /// through `pkg_ctx`. See [`Self::new_with_packages`].
    pub fn new_reconciler_with_packages(
        source: &str,
        ctx: serde_json::Value,
        layout_path: Option<&Path>,
        pkg_ctx: crate::pkg::PackageContext,
        state: SharedState,
    ) -> Result<Self, JsError> {
        Self::with_effects(
            source,
            ctx,
            layout_path,
            Effects::Allowed,
            Some(pkg_ctx),
            StateAccess::RECONCILER,
            state,
        )
    }

    /// The draw runtime (see `crate::d3_figure`): the same layout file, with no
    /// shell or filesystem builtins. Its
    /// `registerFigure` keeps each D3Figure's `draw` by key for
    /// [`Self::draw_figure`], and its timers never fire. `pkg_ctx` supplies the
    /// d3 and linkedom files.
    ///
    /// Without a `layout_path` there is no package resolver at all, so d3 cannot
    /// load and every draw is [`DrawError::NotReady`]. Production always passes
    /// one.
    ///
    /// Its [`Self::eval`] differs in one way. A figure stays registered across
    /// evals (one built at module scope registers only once, at load) until an
    /// eval's tree no longer shows its key, so [`DrawError::UnknownKey`] means
    /// "not in the latest eval".
    ///
    /// `state` is the store the render runtime was built with, read live and read
    /// only ([`StateAccess`]).
    pub fn new_draw(
        source: &str,
        ctx: serde_json::Value,
        layout_path: Option<&Path>,
        pkg_ctx: Option<crate::pkg::PackageContext>,
        state: SharedState,
    ) -> Result<Self, JsError> {
        let mut evaluator = Self::with_effects(
            source,
            ctx,
            layout_path,
            Effects::Denied,
            pkg_ctx,
            StateAccess::DRAW,
            state,
        )?;
        evaluator.draw = Some(DrawState::default());
        Ok(evaluator)
    }

    /// Loads d3 and linkedom into the draw runtime unless they are loaded
    /// already. [`DrawError::NotReady`] while their files are not downloaded;
    /// every failed attempt leaves a module behind in QuickJS, so a caller with
    /// many figures to draw asks this once, not once per figure. Not a draw
    /// runtime: [`DrawError::NotReady`].
    pub fn ensure_drawing_ready(&self) -> Result<(), DrawError> {
        use std::sync::atomic::Ordering;
        let Some(state) = &self.draw else {
            return Err(DrawError::NotReady);
        };
        if !state.loaded.load(Ordering::Relaxed) {
            let attempt = state.attempts.fetch_add(1, Ordering::Relaxed);
            self.load_draw_libraries(attempt)?;
            state.loaded.store(true, Ordering::Relaxed);
        }
        Ok(())
    }

    /// Runs the `draw` of the D3Figure with `key` from the latest [`Self::eval`]
    /// against a fresh `<svg width height>` and returns its markup. d3 and
    /// linkedom are loaded on the first call ([`Self::ensure_drawing_ready`]).
    /// A `draw` still running after `time_limit` is interrupted.
    pub fn draw_figure(
        &self,
        key: &str,
        width: f64,
        height: f64,
        time_limit: std::time::Duration,
    ) -> Result<String, DrawError> {
        if self.draw.is_none() {
            return Err(DrawError::UnknownKey(key.to_string()));
        }
        self.ensure_drawing_ready()?;
        let known = self.context.with(|ctx| {
            ctx.eval::<bool, _>(format!(
                "__tauler_figures.has({})",
                serde_json::Value::String(key.to_string())
            ))
            .unwrap_or(false)
        });
        if !known {
            return Err(DrawError::UnknownKey(key.to_string()));
        }

        // Outside `context.with`: setting the handler takes the runtime lock.
        let deadline = std::time::Instant::now() + time_limit;
        self.runtime
            .set_interrupt_handler(Some(Box::new(move || std::time::Instant::now() > deadline)));
        let result = self.context.with(|ctx| {
            let draw: Function = ctx
                .globals()
                .get("__tauler_draw_figure")
                .map_err(|e| CaughtError::from_error(&ctx, e).to_string())?;
            draw.call::<_, String>((key, width, height))
                .catch(&ctx)
                .map_err(|e| e.to_string())
        });
        self.runtime.set_interrupt_handler(None);
        result.map_err(|message| {
            if std::time::Instant::now() > deadline {
                DrawError::TimedOut
            } else {
                DrawError::Threw(message)
            }
        })
    }

    /// Evaluates [`DRAW_BOOTSTRAP_JS`] under a module name unique to `attempt`.
    /// A module that cannot be resolved or loaded means the files are not here
    /// yet: [`DrawError::NotReady`]. Anything else the bootstrap throws is
    /// [`DrawError::Threw`].
    fn load_draw_libraries(&self, attempt: u32) -> Result<(), DrawError> {
        self.context.with(|ctx| {
            let name = crate::scheme::draw_bootstrap_module(attempt);
            rquickjs::Module::declare(ctx.clone(), name, DRAW_BOOTSTRAP_JS)
                .and_then(|module| module.eval())
                .and_then(|(_, promise)| promise.finish::<()>())
                .catch(&ctx)
                .map_err(|e| {
                    let message = e.to_string();
                    if message.contains("Error resolving module")
                        || message.contains("Error loading module")
                    {
                        DrawError::NotReady
                    } else {
                        DrawError::Threw(message)
                    }
                })
        })
    }

    fn with_effects(
        source: &str,
        ctx: serde_json::Value,
        layout_path: Option<&Path>,
        effects: Effects,
        pkg_ctx: Option<crate::pkg::PackageContext>,
        state_access: StateAccess,
        state: SharedState,
    ) -> Result<Self, JsError> {
        let draw = state_access == StateAccess::DRAW;
        let loaded_paths: Arc<Mutex<Vec<PathBuf>>> = Arc::new(Mutex::new(Vec::new()));
        let store_token = new_store_token();
        // The layout's module scope runs inside this constructor.
        let evaluating = Arc::new(AtomicBool::new(true));
        let defaults = Arc::new(Defaults::default());

        let (runtime, context) = if let Some(path) = layout_path {
            // Relative imports resolve from, and stay confined to, the layout file's
            // own directory.
            let dir = path.parent().unwrap_or(path);
            optative_script::build_runtime(
                crate::ui::registry::UI_COMPONENTS,
                (
                    TaulerModule,
                    GitPackageResolver::new(dir.to_path_buf(), pkg_ctx.clone()),
                ),
                (
                    TaulerModule,
                    SchemaAwareLoader::new(
                        Arc::clone(&loaded_paths),
                        pkg_ctx.as_ref().map(|pkg_ctx| Arc::clone(&pkg_ctx.d3)),
                        ModuleNames::new(dir, pkg_ctx.as_ref().map(|c| c.cache_root.as_path())),
                        store_token.clone(),
                    ),
                ),
            )
        } else {
            optative_script::build_runtime(
                crate::ui::registry::UI_COMPONENTS,
                TaulerModule,
                TaulerModule,
            )
        }
        // No `Ctx` exists yet, so nothing was thrown: rquickjs's own text is all there is.
        .map_err(|e| JsError(e.to_string()))?;
        let stream_values: StreamValues = Arc::new(RwLock::new(HashMap::new()));
        let calls: StreamCalls = Arc::new(Mutex::new(Vec::new()));
        let module_calls: Arc<Mutex<Vec<(String, serde_json::Value)>>> =
            Arc::new(Mutex::new(Vec::new()));

        let mut stored_render_fn: Option<Persistent<Function<'static>>> = None;

        {
            let sv = Arc::clone(&stream_values);
            let calls_inner = Arc::clone(&calls);
            let module_calls_inner = Arc::clone(&module_calls);
            with_described(&context, |qjs_ctx| {
                qjs_ctx.eval::<(), _>(JSX_GLOBALS_JS)?;
                // Behind `store` (`JSX_GLOBALS_JS`): the first sight of an
                // identity saves its default, every later one finds the saved value.
                register_state_host(
                    &qjs_ctx,
                    &state,
                    state_access,
                    &store_token,
                    &evaluating,
                    &defaults,
                )?;
                optative_script::register_h(&qjs_ctx)?;
                // The reconciler vocabulary, registered but not wired as an `esto`
                // module: a layout file reaches `unit` the way it reaches
                // `useJSONStream`, as a global. The effectful half of that crate's
                // builtins (`sh`, `read`, `ls`) is deliberately absent — evaluating a
                // layout file must not touch the world, and leaving them unregistered
                // is what makes that a guarantee rather than a convention (ADR 0034).
                optative_script::builtins::register_all(
                    &qjs_ctx,
                    optative_script::builtins::RECONCILER_BUILTINS,
                )?;
                qjs_ctx.eval::<(), _>(
                    "globalThis.unit = __esto_unit;
                    globalThis.optativeSet = __esto_optative_set;
                    globalThis.optativeJsonSet = __esto_optative_json_set;",
                )?;
                if effects == Effects::Allowed {
                    optative_script::builtins::register_all(
                        &qjs_ctx,
                        optative_script::builtins::EFFECTFUL_BUILTINS,
                    )?;
                    qjs_ctx.eval::<(), _>(
                        "globalThis.sh = __esto_sh;
                        globalThis.read = __esto_read;
                        globalThis.ls = __esto_ls;
                        globalThis.exists = __esto_exists;
                        globalThis.hash = __esto_hash;",
                    )?;
                }
                // `h` isn't aliased directly to `__esto_h`: its generic-tag output nests
                // props under a `props` key (`{type, props, children}`), but Rust-backed UI
                // components (e.g. `@ui/card`) deserialize their `children: Vec<Node>` prop
                // eagerly, mid-render, expecting the flat shape — so each node must be
                // reshaped as soon as it's produced, not just once at the very end.
                //
                // The reshape itself is `JSX_GLOBALS_JS`'s, evaluated above: both engines
                // run the same source (ADR 0027). Only `Fragment` needs interpolating.
                //
                // DECISION (local Stores, `tauler-core/src/local_store.rs`): the line that
                // assembled `h` here and in `tauler-web/js/runtime.js` is now the shared
                // `__tauler_h`, given each engine's factory. It also keeps the component
                // now running on the stack a local Store is named from. Reason: the two
                // assemblies cannot differ when there is one. Rejected: one more global
                // between `type` and `__esto_h`, as first sketched — a closure per
                // component call, where pushing around the factory call needs none.
                qjs_ctx.eval::<(), _>(format!(
                    "globalThis.h = __tauler_h(__esto_h);
                    globalThis.Fragment = {{ {}: true }};",
                    optative_script::tags::ESTO_FRAGMENT
                ))?;
                qjs_ctx.globals().set(
                    "useStringStream",
                    rquickjs::Function::new(
                        qjs_ctx.clone(),
                        // `Opt<Option<_>>`, because neither half covers both calls and a
                        // layout file makes both. rquickjs distinguishes an *absent*
                        // argument from one explicitly passed as `undefined`: `Opt` accepts
                        // the first and rejects the second, `Option` the reverse. So
                        // `useStringStream("my-bin")` used to throw a type error while
                        // `useStringStream("my-bin", undefined)` worked — and the natural
                        // one-argument call for a Stream with no script was the broken one.
                        // `useJSONStream` and `<Module>` never hit it because they forward
                        // their own `script` either way, which is why it went unnoticed.
                        move |bin: String, script: rquickjs::function::Opt<Option<String>>| {
                            let script = script.0.flatten();
                            calls_inner
                                .lock()
                                .unwrap()
                                .push((bin.clone(), script.clone()));
                            sv.read()
                                .unwrap()
                                .get(&(bin, script))
                                .cloned()
                                .unwrap_or_default()
                        },
                    )?,
                )?;
                qjs_ctx.globals().set(
                    "registerModule",
                    rquickjs::Function::new(
                        qjs_ctx.clone(),
                        move |bin: String, props: rquickjs::Value| {
                            let props: serde_json::Value = rquickjs_serde::from_value(props)
                                .unwrap_or(serde_json::Value::Null);
                            let mut mc = module_calls_inner.lock().unwrap();
                            match mc.iter_mut().find(|(b, _)| b == &bin) {
                                Some((_, existing)) => merge_missing(existing, props),
                                None => mc.push((bin, props)),
                            }
                        },
                    )?,
                )?;
                qjs_ctx.globals().set(
                    "__workspaces_layout",
                    rquickjs::Function::new(qjs_ctx.clone(), workspaces_layout_js_fn)?,
                )?;
                // The runtime half of `tauler-configgen`: a
                // generated root component embeds its schema's own minijinja template as
                // a string constant and calls this to turn collected Item data into the
                // final config text. Registered unconditionally rather than gated behind
                // `Effects::Allowed` like `sh`/`read` — unlike those, it touches nothing
                // in the world, so there is no frame-budget reason to keep it out of the
                // render runtime the way ADR 0034 keeps `sh` out.
                qjs_ctx.globals().set(
                    "renderTemplate",
                    rquickjs::Function::new(qjs_ctx.clone(), render_template_js_fn)?,
                )?;
                qjs_ctx.globals().set(
                    "warn",
                    rquickjs::Function::new(qjs_ctx.clone(), warn_js_fn)?,
                )?;
                // Names an object/array `<D3Figure>` key's Picture (`JSX_GLOBALS_JS`).
                // Outside the `Effects::Allowed` gate: it touches nothing, and the
                // render runtime is the one that names Pictures.
                qjs_ctx.globals().set(
                    "__tauler_sha256",
                    rquickjs::Function::new(qjs_ctx.clone(), |text: String| {
                        crate::d3_figure::assets::sha256_hex(text.as_bytes())
                    })?,
                )?;
                // Behind `measureText` (`JSX_GLOBALS_JS`). Outside the
                // `Effects::Allowed` gate for the same reason: it is a pure
                // function of the registered fonts (ADR 0046), and a chart lays
                // out in whichever runtime builds it.
                qjs_ctx.globals().set(
                    "__tauler_measure_text",
                    rquickjs::Function::new(qjs_ctx.clone(), measure_text_js_fn)?,
                )?;
                qjs_ctx.globals().set(
                    "__tauler_theme_color",
                    rquickjs::Function::new(qjs_ctx.clone(), theme_color_js_fn)?,
                )?;
                crate::ui::registry::register_ui_components(&qjs_ctx)?;
                if !ctx.is_null() {
                    let js_ctx = rquickjs_serde::to_value(qjs_ctx.clone(), &ctx)
                        .map_err(|_| rquickjs::Error::Unknown)?;
                    qjs_ctx.globals().set("ctx", js_ctx)?;
                }
                // A captured constant, not a live env read — safe in the render
                // runtime the same way `ctx` already is. Lets `ConfigFile`
                // (tauler-core/src/globals.rs) expand a leading `~/` in `path`
                // the same way `crate::config::expand_tilde` already does for
                // `theme.file`, `fonts.extra` and a Service's `bin` — a layout
                // author writes a path the way they'd write it in a shell,
                // never a hardcoded `/home/<user>/...`.
                qjs_ctx
                    .globals()
                    .set("HOME", std::env::var("HOME").unwrap_or_default())?;

                // Installed once, before the layout file can run: the collector and
                // the hook dispatcher are the only JavaScript tauler adds to a
                // reconciler runtime, and both are needed before the first Sweep.
                qjs_ctx.eval::<(), _>(collect_units_source())?;
                qjs_ctx.eval::<(), _>(dispatch_hook_source())?;
                // Before the layout file runs too: a D3Figure built at module scope
                // calls `registerFigure` while the module loads. Only the draw
                // runtime keeps anything; elsewhere the figures are read back from
                // the evaluated tree ([`figures_in`]), so `draw` stays behind.
                qjs_ctx.eval::<(), _>(if draw {
                    DRAW_RUNTIME_JS
                } else {
                    "globalThis.registerFigure = () => {};"
                })?;

                // DECISION (issue #271): the Store pass (`tauler_core::store_pass`) runs here on
                // the layout file and in `SchemaAwareLoader::load` on every import, both
                // before `transform_source`. Reason: tauler already owns both call sites —
                // `optative-script` only supplies the public `transform_source` — so no
                // new release of it is needed. Evidence: the `store_*` tests below load a
                // layout, an import, a re-export, a `.ts` file and a Package through
                // these two sites (`tauler-core/src/store_pass.rs` holds what the pass
                // costs). The six `oxc_*` crates it uses are already in `Cargo.lock`
                // through `optative-script`: no new package.
                // Rejected: identity from the loader alone (one Store per file, as its
                // default export) — it needs no parser, but a file could hold one Store
                // and would lose the default export a component file needs.
                //
                // SPEC: `<module>` is the path relative to the layout file's directory —
                // for the layout file itself its file name — or `@gh/owner/repo/<path
                // in the Package>` for a Package's file (`ModuleNames::name_of`).
                //
                // The line in a refusal counts lines of `source`. For a
                // `layout.op.mdx` that is the body after the frontmatter
                // (`layout_source::parse_source` hands it over byte for byte), so the
                // line is low by the frontmatter's length — the same offset every
                // QuickJS error in a layout file already has. Imports are exact.
                // Both are fixed together in
                // https://github.com/kantord/tauler/issues/837.
                //
                // The docs site runs the same pass on every example before its own
                // transform (`example_js` in `tauler-docgen/src/web.rs`).
                let layout_module = layout_path
                    .and_then(|path| path.file_name())
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "layout.jsx".to_string());
                let named =
                    name_stores(&qjs_ctx, source, "layout.jsx", &layout_module, &store_token)?;
                let js_source = optative_script::jsx::transform_source(&named, "layout.jsx");
                let module = rquickjs::Module::declare(qjs_ctx.clone(), "layout.jsx", js_source)?;
                if let Some(path) = layout_path {
                    set_import_meta(&module, path)?;
                }
                let (module, promise) = module.eval()?;
                promise.finish::<()>()?;
                if draw {
                    // What the layout's module scope built, kept for good: such an
                    // element registers once, so a figure that leaves the tree and
                    // comes back is found here. Bounded by the module's code.
                    qjs_ctx.eval::<(), _>(
                        "globalThis.__tauler_module_figures = new Map(__tauler_figures);",
                    )?;
                }
                let render_fn: Function = module.get("default")?;
                // Gives each render its stack of component instances and its set of
                // local Store declarations (`tauler-core/src/local_store.rs`).
                let render_fn: Function = qjs_ctx
                    .globals()
                    .get::<_, Function>("__tauler_rendering")?
                    .call((render_fn,))?;
                stored_render_fn = Some(Persistent::save(&qjs_ctx, render_fn));

                Ok(())
            })?;
        }

        // DECISION (issue #271): state outlives this evaluator because its owner is an
        // argument (`state`), built above every evaluator and handed to each one; a
        // reload builds a new evaluator around the same owner. Evidence:
        // `a_store_keeps_its_value_across_a_rebuilt_evaluator` below, and every
        // construction site is handed the app's owner: `App::initial_load`,
        // `App::handle_layout_reload` and the two `spawn_*` beside them in `src/app.rs`,
        // `Reconciler::spawn` in `src/units.rs`, `DrawRuntime::spawn` in
        // `src/d3_figure/draw.rs`, and `taulerbox/src/main.rs`.
        // The Kill did not fire: no evaluator is built where an `Arc` cannot be passed.
        // Rejected: reading the old evaluator's state out before dropping it — a failed
        // reload keeps the old evaluator, so two would briefly own the state.
        //
        // A state whose identity the new load did not declare is dropped only after
        // that load succeeded (`drop_undeclared_states` in `src/app.rs`).
        let evaluator = Self {
            context,
            runtime,
            stream_values,
            calls,
            module_calls,
            state,
            state_access,
            evaluating,
            defaults,
            render_fn: stored_render_fn,
            loaded_paths,
            draw: None,
        };
        evaluator.context.with(|ctx| evaluator.end_evaluation(&ctx));
        Ok(evaluator)
    }

    /// The layout's own code has stopped running. In a runtime that only reads, the
    /// writes it made were kept in this runtime alone, and are forgotten here
    /// ([`StateAccess`]).
    fn end_evaluation<'js>(&self, ctx: &rquickjs::Ctx<'js>) {
        self.evaluating.store(false, Ordering::Relaxed);
        if self.state_access == StateAccess::ReadWrite {
            return;
        }
        if let Ok(drop_unsaved) = ctx
            .globals()
            .get::<_, Function>("__tauler_state_drop_unsaved")
        {
            let _ = drop_unsaved.call::<_, ()>(());
        }
    }

    /// The identities this runtime keeps a default for ([`Defaults`]), sorted.
    #[cfg(test)]
    fn kept_defaults(&self) -> Vec<String> {
        self.defaults.identities()
    }

    /// Drop last tick's handler functions. The capture slot holds its own reference,
    /// so a drag in progress is unaffected (ADR 0020).
    fn reset_handlers(&self) {
        self.context.with(|ctx| {
            let _ = ctx.eval::<(), _>("__tauler_handlers.length = 0;");
        });
    }

    /// Move handler `id` into the capture slot, where it survives the ticks a drag
    /// spans. A handler that is a plain intent array has no id and needs no capture.
    pub fn capture_handler(&self, id: i64) {
        self.context.with(|ctx| {
            if let Ok(f) = ctx.globals().get::<_, Function>("__tauler_capture_handler") {
                let _ = f.call::<_, ()>((id,));
            }
        });
    }

    pub fn release_handler(&self) {
        self.context.with(|ctx| {
            if let Ok(f) = ctx.globals().get::<_, Function>("__tauler_release_handler") {
                let _ = f.call::<_, ()>(());
            }
        });
    }

    /// Call a handler and return the intents it produced. `id` below zero means the
    /// captured one. A handler that throws is reported and dispatches nothing — a
    /// gesture that does nothing beats a bar that dies (ADR 0021).
    /// Call the handler a press captured, whichever tick registered it.
    pub fn invoke_captured_handler(
        &self,
        pointer: &serde_json::Value,
    ) -> Option<serde_json::Value> {
        self.invoke_handler(CAPTURED_HANDLER, pointer)
    }

    /// [`Self::invoke_captured_handler`], as [`Self::invoke_handler_outcome`].
    pub fn invoke_captured_handler_outcome(&self, pointer: &serde_json::Value) -> HandlerOutcome {
        self.invoke_handler_outcome(CAPTURED_HANDLER, pointer)
    }

    /// A handler that throws is reported and dispatches nothing — a gesture that does
    /// nothing beats a bar that dies (ADR 0021). Reported once per handler, because this
    /// sits on the input path: a mapper that throws would otherwise log on every motion
    /// event, which is once a pixel.
    pub fn invoke_handler(
        &self,
        id: i64,
        pointer: &serde_json::Value,
    ) -> Option<serde_json::Value> {
        self.invoke_handler_outcome(id, pointer).intents
    }

    /// [`Self::invoke_handler`], and whether the handler changed any state. The handler
    /// runs inside one [`crate::state::StateStore`] transaction: nested values it was
    /// handed are compared and written back when it returns, and everything is rolled
    /// back when it throws.
    pub fn invoke_handler_outcome(&self, id: i64, pointer: &serde_json::Value) -> HandlerOutcome {
        // DECISION: this only reports; the caller (`App::resolve`) turns a change into
        // a flag and a Notifier ping, and the next Pass ticks. The Kill (a click's write
        // not on screen within one Pass of the click) is not met and the loop keeps its
        // order: see `StateWake` in `src/app.rs` for the order of events and the test.
        self.context.with(|ctx| {
            let Ok(f) = ctx.globals().get::<_, Function>("__tauler_invoke_handler") else {
                return HandlerOutcome::default();
            };
            let Ok(arg) = rquickjs_serde::to_value(ctx.clone(), pointer) else {
                return HandlerOutcome::default();
            };
            if self.begin_state(&ctx).is_err() {
                return HandlerOutcome::default();
            }
            let out = f
                .call::<_, rquickjs::Value>((id, arg))
                .and_then(|out| self.flush_state(&ctx).map(|()| out))
                .catch(&ctx);
            match out {
                Ok(out) => {
                    let state_changed = self.commit_state();
                    let intents = rquickjs_serde::from_value::<serde_json::Value>(out)
                        .ok()
                        .filter(|intents| !intents.is_null());
                    HandlerOutcome {
                        intents,
                        state_changed,
                    }
                }
                Err(e) => {
                    warn_handler_once(id, &e);
                    self.abort_state(&ctx);
                    HandlerOutcome::default()
                }
            }
        })
    }

    /// Start one handler's or one render's transaction, and tell the runtime where the
    /// store's clock stands so it knows whether what it mirrors is still true.
    ///
    /// A runtime that only reads ([`StateAccess`]) opens no transaction: it has nothing
    /// to undo, and the journal is the render runtime's.
    fn begin_state<'js>(&self, ctx: &rquickjs::Ctx<'js>) -> rquickjs::Result<()> {
        let clock = {
            let mut state = self.state.lock().unwrap();
            if self.state_access == StateAccess::ReadWrite {
                state.begin();
            }
            state.clock()
        };
        self.defaults.begin();
        ctx.globals().set("__tauler_state_now", clock as f64)
    }

    /// Keep what one handler or one render wrote. `true` when any state changed.
    fn commit_state(&self) -> bool {
        self.state_access == StateAccess::ReadWrite && self.state.lock().unwrap().commit()
    }

    /// Write back every nested value that was handed out and changed in place.
    fn flush_state<'js>(&self, ctx: &rquickjs::Ctx<'js>) -> rquickjs::Result<()> {
        ctx.globals()
            .get::<_, Function>("__tauler_state_flush")?
            .call::<_, ()>(())
    }

    /// Undo a handler or render that threw: the store rolls back, and the runtime
    /// forgets the nested values it handed out, which may hold half a change, and the
    /// defaults it kept since [`Self::begin_state`] ([`Defaults`]).
    fn abort_state<'js>(&self, ctx: &rquickjs::Ctx<'js>) {
        if let Ok(abort) = ctx.globals().get::<_, Function>("__tauler_state_abort") {
            let _ = abort.call::<_, ()>(());
        }
        if self.state_access == StateAccess::ReadWrite {
            self.state.lock().unwrap().rollback();
        }
        self.defaults.forget_added();
    }

    /// Call the layout's default export inside one [`crate::state::StateStore`]
    /// transaction: committed when it returns, rolled back when it throws.
    ///
    /// `render_fn` is the layout's default export inside `__tauler_rendering`, and the
    /// flush drops local Stores ("Dropping" in `tauler-core/src/local_store.rs`).
    fn render_in_transaction<'js>(
        &self,
        ctx: &rquickjs::Ctx<'js>,
    ) -> rquickjs::Result<rquickjs::Value<'js>> {
        let render_fn = self.render_fn.as_ref().unwrap().clone().restore(ctx)?;
        self.begin_state(ctx)?;
        self.evaluating.store(true, Ordering::Relaxed);
        let rendered = render_fn
            .call::<_, rquickjs::Value>(())
            .and_then(|value| self.flush_state(ctx).map(|()| value));
        if rendered.is_err() {
            // Before `end_evaluation`: it forgets the nested values handed out.
            self.abort_state(ctx);
        }
        self.end_evaluation(ctx);
        match rendered {
            Ok(value) => {
                // A render-time write pings nothing, so whether it changed anything
                // is not asked.
                self.commit_state();
                Ok(value)
            }
            Err(e) => Err(e),
        }
    }

    pub fn eval(
        &self,
        new_stream_values: &HashMap<(String, Option<String>), String>,
    ) -> Result<EvalOutput, JsError> {
        self.stream_values
            .write()
            .unwrap()
            .clone_from(new_stream_values);
        self.calls.lock().unwrap().clear();
        self.module_calls.lock().unwrap().clear();
        self.reset_handlers();

        with_described(&self.context, |qjs_ctx| {
            // For a Store nothing is copied in here and only what changed is written
            // back (`render_in_transaction`; tests `store_*`).
            // SPEC: a render-time write pings nothing.
            // SPEC: the draw runtime's inputs carry state by version, so a draw
            // evaluates with the Stores the eval that asked read, and an unchanged
            // state is not cloned per tick. Not built: [`EvalInputs`] carries no state
            // and the draw runtime reads the store as it is (the last `RISK:` in
            // `src/state.rs`).
            let value = self.render_in_transaction(&qjs_ctx)?;

            let json_str = qjs_ctx
                .json_stringify(value)?
                .ok_or(rquickjs::Error::Unknown)?
                .to_string()?;
            let layout: serde_json::Value =
                serde_json::from_str(&json_str).map_err(|_| rquickjs::Error::Unknown)?;
            let layout = crate::units::strip_items(flatten_passthrough(layout));
            let figure_calls = figures_in(&layout);
            if self.draw.is_some() {
                // Registrations outlive the eval that made them (a module-scope
                // figure registers once, at load), so the ones whose key this tree
                // does not show are dropped here, and a module-scope figure back in
                // the tree is restored: the Map stays bounded by the figures on
                // screen, and an unknown key is one the latest eval does not render.
                let keys: Vec<&str> = figure_calls.iter().map(|c| c.key.as_str()).collect();
                let keys = serde_json::to_string(&keys).map_err(|_| rquickjs::Error::Unknown)?;
                qjs_ctx.eval::<(), _>(format!(
                    "{{ const keep = new Set({keys}); \
                     for (const k of [...__tauler_figures.keys()]) \
                     if (!keep.has(k)) __tauler_figures.delete(k); \
                     for (const k of keep) \
                     if (!__tauler_figures.has(k) && __tauler_module_figures.has(k)) \
                     __tauler_figures.set(k, __tauler_module_figures.get(k)); }}"
                ))?;
            }
            // Every stream, not only the ones this render read: a figure's `draw`
            // runs later against this snapshot and may read others. Nothing reads
            // the snapshot of a tree with no figure, so that one stays empty.
            let stream_values = if figure_calls.is_empty() {
                HashMap::new()
            } else {
                new_stream_values.clone()
            };
            Ok(EvalOutput {
                layout,
                stream_calls: self.calls.lock().unwrap().clone(),
                module_calls: self.module_calls.lock().unwrap().clone(),
                figure_calls,
                inputs: Arc::new(EvalInputs { stream_values }),
            })
        })
    }

    /// Evaluate for the reconciler: the same render call, but what comes back is
    /// the Items grouped by the live `unit()` object that declared them, not a
    /// JSON tree.
    ///
    /// The walk happens in JavaScript because that is the only place a Unit is
    /// still an object with callable hooks; once it goes through
    /// `json_stringify` it is a bag of props and a number nobody else can
    /// resolve. The Unit objects are parked in `__tauler_units` so a hook call
    /// can address one by index (ADR 0034).
    pub fn eval_units(
        &self,
        new_stream_values: &HashMap<(String, Option<String>), String>,
    ) -> Result<Vec<crate::units::UnitBatch>, JsError> {
        self.stream_values
            .write()
            .unwrap()
            .clone_from(new_stream_values);
        self.calls.lock().unwrap().clear();
        self.module_calls.lock().unwrap().clear();
        self.reset_handlers();

        with_described(&self.context, |qjs_ctx| {
            // A Store is the bar's: this render reads it, and what it writes stays in
            // this runtime ([`StateAccess`]).
            let tree = self.render_in_transaction(&qjs_ctx)?;

            let collect: Function = qjs_ctx.globals().get("__tauler_collect_units")?;
            let batches: rquickjs::Value = collect.call((tree,))?;
            let json = qjs_ctx
                .json_stringify(batches)?
                .ok_or(rquickjs::Error::Unknown)?
                .to_string()?;
            serde_json::from_str(&json).map_err(|_| rquickjs::Error::Unknown)
        })
    }

    /// Run one of a Unit's lifecycle hooks over a whole batch, returning how many
    /// Items it actually acted on.
    ///
    /// `hook` is the batch spelling (`enter`), `one` the per-Item sugar
    /// (`enterOne`). Which of the two a Unit defines is the Unit's choice and
    /// cannot be guessed — `(p) => …` and `(ps) => …` are the same JavaScript —
    /// so the name carries the contract (ADR 0033). Zero means the Unit defines
    /// neither, which is a transition it is not managing, not a failure.
    pub fn dispatch_unit_hook(
        &self,
        unit_index: usize,
        hook: &str,
        one: &str,
        payload: &serde_json::Value,
    ) -> usize {
        self.context.with(|qjs_ctx| {
            let Ok(dispatch) = qjs_ctx
                .globals()
                .get::<_, Function>("__tauler_dispatch_hook")
            else {
                return 0;
            };
            let Ok(payload) = rquickjs_serde::to_value(qjs_ctx.clone(), payload) else {
                return 0;
            };
            dispatch
                .call::<_, usize>((unit_index, hook, one, payload))
                .catch(&qjs_ctx)
                .map_err(
                    |e| tracing::error!(exception = %e, hook, "the Unit hook threw an exception"),
                )
                .unwrap_or(0)
        })
    }

    /// Call one of a Unit's projections — `key` or `value` — on a single Item.
    ///
    /// Projections stay per-Item where the lifecycle hooks do not: they name a
    /// part of the Item rather than act on the world, and `esto` writes them that
    /// way (`key: (i) => i.entity`). Batching them would buy nothing and break
    /// every Unit that already exists.
    pub fn call_unit_projection(
        &self,
        unit_index: usize,
        projection: &str,
        item: &serde_json::Value,
    ) -> Option<serde_json::Value> {
        self.context.with(|qjs_ctx| {
            let f: Function = unit_at(&qjs_ctx, unit_index)?.get(projection).ok()?;
            let arg = rquickjs_serde::to_value(qjs_ctx.clone(), item).ok()?;
            let out: rquickjs::Value = f
                .call((arg,))
                .catch(&qjs_ctx)
                .map_err(|e| tracing::error!(exception = %e, projection, "the projection threw an exception"))
                .ok()?;
            json_of(&qjs_ctx, out)
        })
    }

    /// Read a plain property off a Unit — anything that is data rather than a
    /// function, like `refreshInterval`.
    pub fn unit_property(&self, unit_index: usize, name: &str) -> Option<serde_json::Value> {
        self.context.with(|qjs_ctx| {
            let value: rquickjs::Value = unit_at(&qjs_ctx, unit_index)?.get(name).ok()?;
            json_of(&qjs_ctx, value)
        })
    }

    /// Which backend a Unit's `reconciler` names — `"optativeSet"` or
    /// `"optativeJsonSet"`.
    pub fn reconciler_kind(&self, unit_index: usize) -> Option<String> {
        self.context.with(|qjs_ctx| {
            let reconciler: rquickjs::Object =
                unit_at(&qjs_ctx, unit_index)?.get("reconciler").ok()?;
            reconciler
                .get(optative_script::tags::ESTO_RECONCILER_KIND)
                .ok()
        })
    }

    /// Ask a Unit's reconciler what the world currently holds.
    ///
    /// Unlike a hook this hangs off `reconciler`, because it belongs to the
    /// backend rather than to the Unit. `None` means the Unit has no `observe` to
    /// ask, which for `optativeSet` is a broken Unit — see [`Self::reconciler_kind`].
    pub fn observe(&self, unit_index: usize) -> Option<serde_json::Value> {
        self.context.with(|qjs_ctx| {
            let reconciler: rquickjs::Object =
                unit_at(&qjs_ctx, unit_index)?.get("reconciler").ok()?;
            let f: Function = reconciler.get("observe").ok()?;
            let out: rquickjs::Value = f
                .call(())
                .catch(&qjs_ctx)
                .map_err(|e| tracing::error!(exception = %e, "observe threw an exception"))
                .ok()?;
            json_of(&qjs_ctx, out)
        })
    }

    /// Returns the canonicalized paths of all files loaded via import statements
    /// during `new()`. Does not include the inline layout source itself.
    pub fn loaded_paths(&self) -> Vec<PathBuf> {
        self.loaded_paths.lock().unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(source: &str) -> EvalOutput {
        JsxEvaluator::new(source, serde_json::Value::Null, None)
            .unwrap()
            .eval(&std::collections::HashMap::new())
            .unwrap()
    }

    /// A layout file can declare a Unit and render an Item of it. `unit()` is a
    /// global here rather than an import, like every other name a layout file gets
    /// `unit()` works in a layout file, and its Items do not reach the layout
    /// tree: an Item is a statement about the world, not about the bar, and the
    /// reconciler reads it from its own evaluation (ADR 0034). Leaving it in was
    /// worse than useless — `layout::first_child` takes a panel's first child as
    /// its whole content, so a Unit written above the UI would swallow it.
    #[test]
    fn a_layout_file_can_declare_a_unit_and_the_item_does_not_render() {
        let result = eval(
            r#"
            const Light = unit({
              key: (i) => i.entity,
              value: (i) => i.state,
              reconciler: optativeSet({ observe: () => [] }),
              enterOne: (i) => `on ${i.entity}`,
            });
            export default function render() {
              return <root><Light entity="light.desk" state="on" /><div>bar</div></root>;
            }"#,
        )
        .layout;
        assert_eq!(
            result["children"].as_array().map(Vec::len),
            Some(1),
            "only the div survives: {result}"
        );
        assert_eq!(result["children"][0]["type"], "div");
    }

    /// `themeColor` reads `ctx.theme.colors` and applies the alpha the class `/NN` suffix would.
    #[test]
    fn theme_color_reads_the_active_theme() {
        let ctx = serde_json::json!({ "theme": { "colors": { "primary": "#7aa2f7" } } });
        let result = JsxEvaluator::new(
            r#"export default function render() {
                return <root>{[themeColor("primary"), themeColor("primary", 0.13), themeColor("nope"), themeColor("primary", "0.5"), themeColor("primary", null)].join(" ")}</root>;
            }"#,
            ctx,
            None,
        )
        .unwrap()
        .eval(&std::collections::HashMap::new())
        .unwrap()
        .layout;
        assert_eq!(
            result["children"][0],
            "#7aa2f7 #7aa2f721 nope #7aa2f780 #7aa2f7"
        );
    }

    /// The other half of ADR 0034: evaluating a layout file must not be able to
    /// touch the world. `sh` exists in `optative-script` and is deliberately not
    /// registered here, so a layout file that reaches for it fails rather than
    /// blocking the loop on a subprocess.
    #[test]
    fn the_render_runtime_has_no_shell() {
        let evaluator = JsxEvaluator::new(
            r#"export default function render() { return <root>{typeof sh}</root>; }"#,
            serde_json::Value::Null,
            None,
        )
        .unwrap();
        let result = evaluator
            .eval(&std::collections::HashMap::new())
            .unwrap()
            .layout;
        assert_eq!(
            result["children"][0], "undefined",
            "sh must not exist in the render runtime"
        );
    }

    /// The reconciler runtime is the same evaluator with the world switched on.
    /// A hook and an `observe` need `sh` (ADR 0034); this is the half the render
    /// runtime is not allowed to have.
    #[test]
    fn the_reconciler_runtime_has_a_shell() {
        let result = JsxEvaluator::new_reconciler(
            r#"export default function render() { return <root>{typeof sh}</root>; }"#,
            serde_json::Value::Null,
            None,
            Default::default(),
        )
        .unwrap()
        .eval(&std::collections::HashMap::new())
        .unwrap()
        .layout;
        assert_eq!(result["children"][0], "function");
    }

    /// `HOME` is a captured constant available in both runtimes, unlike `sh` —
    /// so a layout can write a portable `~/...` path (via `ConfigFile`,
    /// tauler-core/src/globals.rs) even in the render runtime, which never
    /// touches the world otherwise.
    #[test]
    fn home_is_available_in_both_runtimes() {
        let _guard = crate::config::HOME_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        unsafe { std::env::set_var("HOME", "/home/someone") };

        let render_result =
            eval(r#"export default function render() { return <root>{HOME}</root>; }"#).layout;
        assert_eq!(render_result["children"][0], "/home/someone");

        let reconciler_result = JsxEvaluator::new_reconciler(
            r#"export default function render() { return <root>{HOME}</root>; }"#,
            serde_json::Value::Null,
            None,
            Default::default(),
        )
        .unwrap()
        .eval(&std::collections::HashMap::new())
        .unwrap()
        .layout;
        assert_eq!(reconciler_result["children"][0], "/home/someone");
    }

    /// And it really runs: `typeof sh` only proves a binding exists, not that the
    /// subprocess half of it survived the trip through two crates.
    #[test]
    fn the_reconciler_runtimes_shell_actually_runs_a_command() {
        let result = JsxEvaluator::new_reconciler(
            r#"export default function render() { return <root>{sh`printf hi`}</root>; }"#,
            serde_json::Value::Null,
            None,
            Default::default(),
        )
        .unwrap()
        .eval(&std::collections::HashMap::new())
        .unwrap()
        .layout;
        assert_eq!(result["children"][0], "hi");
    }

    #[test]
    fn jsx_evaluator_returns_tag_props_and_children() {
        let result = eval(
            r#"export default function render() { return <span class="flex">{"hello"}</span>; }"#,
        )
        .layout;
        assert_eq!(result["type"], "span");
        assert_eq!(result["class"], "flex");
        assert_eq!(result["children"][0], "hello");
    }

    /// A `type` prop overrides the tag name. `<surface type="wallpaper">` relies
    /// on this to be long-hand for `<wallpaper>`; if flattening ever writes the
    /// tag last instead, that spelling silently becomes an unknown `surface` node.
    #[test]
    fn type_prop_overrides_the_tag_name() {
        let result = eval(
            r#"export default function render() { return <surface type="wallpaper" id="bg" />; }"#,
        )
        .layout;
        assert_eq!(
            result["type"], "wallpaper",
            "a type prop must win over the tag name"
        );
        assert_eq!(result["id"], "bg");
    }

    /// A prop value is data, even one shaped like a node: a row with `type` and `props`
    /// keys passed as a prop reaches the tree as written, with every key it had.
    #[test]
    fn a_prop_shaped_like_a_node_reaches_the_tree_untouched() {
        let result = eval(
            r#"
            const row = { type: "deploy", props: { env: "prod" }, at: 17 };
            export default function render() { return <div data={row} rows={[row]} />; }
            "#,
        )
        .layout;
        let row = serde_json::json!({ "type": "deploy", "props": { "env": "prod" }, "at": 17 });
        assert_eq!(result["data"], row);
        assert_eq!(result["rows"][0], row);
    }

    /// A `type` prop wins over the tag for any tag, and the children survive it —
    /// there is no tag whose children get folded into something else.
    #[test]
    fn type_prop_overriding_a_tag_keeps_the_children() {
        let result =
            eval(r#"export default function render() { return <span type="div">hi</span>; }"#)
                .layout;
        assert_eq!(result["type"], "div");
        assert_eq!(result["children"][0], "hi");
    }

    // Was `transform_jsx_self_closing_element_with_tw_prop`, exercising tauler's own
    // (now-deleted) `transform_jsx`. The transform itself moved to
    // `optative_script::jsx::transform_source` (see that crate's own
    // `pragma_is_h_not_jsx`/`self_closing_element` tests); this keeps a smoke test at
    // tauler's call site confirming it's wired up with the `h` pragma tauler now relies on.
    #[test]
    fn transform_source_self_closing_element_with_class_prop() {
        let result = optative_script::jsx::transform_source(r#"<span class="flex" />"#, "test.jsx");
        assert!(
            result.contains("h("),
            "expected 'h(' in output, got: {result}"
        );
        assert!(
            result.contains("\"span\""),
            "expected '\"span\"' in output, got: {result}"
        );
        assert!(
            result.contains("\"flex\""),
            "expected '\"flex\"' in output, got: {result}"
        );
    }

    #[test]
    fn jsx_evaluator_nested_tree_parses_to_node() {
        let result = eval(r#"export default function render() { return <div class="flex flex-col"><span class="text-white">{"hello"}</span></div>; }"#).layout;
        let node = crate::parse_layout(&result);
        assert!(node.is_ok(), "parse_layout failed: {:?}", node);
    }

    /// A Stream with no script is the ordinary case, and the one-argument call for it
    /// used to throw: rquickjs treats an *absent* argument differently from one passed as
    /// `undefined`, so `Option<String>` rejected the first. `useJSONStream` hid it by
    /// always forwarding a second argument.
    #[test]
    fn use_string_stream_may_be_called_with_only_a_bin() {
        let out = eval(
            r#"export default function render() { return <span class="text-white">{useStringStream("/bin/true")}</span>; }"#,
        );
        assert_eq!(
            out.stream_calls,
            vec![("/bin/true".to_string(), None)],
            "the one-argument call must record a stream with no script"
        );
    }

    #[test]
    fn use_string_stream_returns_injected_value() {
        let mut streams = std::collections::HashMap::new();
        streams.insert(
            ("/usr/bin/bash".to_string(), Some("echo hi".to_string())),
            "hello".to_string(),
        );
        let result = JsxEvaluator::new(
            r#"export default function render() { return <span class="text-white">{useStringStream("/usr/bin/bash", "echo hi")}</span>; }"#,
            serde_json::Value::Null,
            None,
        ).unwrap().eval(&streams).unwrap().layout;
        assert_eq!(result["children"][0], "hello");
    }

    #[test]
    fn jsx_evaluator_injects_ctx_into_script() {
        let ctx = serde_json::json!({ "output": "DP-4", "dpi": 96.0, "width": 250 });
        let value = JsxEvaluator::new(
            r#"export default function render() { return <span class="text-white">{ctx.output}</span>; }"#,
            ctx,
            None,
        ).unwrap().eval(&std::collections::HashMap::new()).unwrap().layout;
        let node = crate::parse_layout(&value);
        assert!(node.is_ok(), "parse_layout failed: {:?}", node);
    }

    #[test]
    fn jsx_evaluator_records_stream_calls() {
        let streams_called = eval(
            r#"export default function render() { return <span class="text-white">{useStringStream("/bin/bash", "script1")}{useStringStream("/bin/bash", "script2")}</span>; }"#,
        ).stream_calls;
        assert!(streams_called.contains(&("/bin/bash".to_string(), Some("script1".to_string()))));
        assert!(streams_called.contains(&("/bin/bash".to_string(), Some("script2".to_string()))));
    }

    #[test]
    fn module_render_prop_exposes_channel_in_events() {
        let result = eval(
            r#"export default function render() { return <Module bin="/usr/bin/test">{(data, events) => <span class="text-white">{events.doThing().channel}</span>}</Module>; }"#,
        ).layout;
        assert_eq!(result["children"][0], "/usr/bin/test");
    }

    /// `useEvents(bin, props)` returns a proxy whose properties are *functions*:
    /// calling one produces a dispatchable intent
    /// (`{ channel, event: { type, ...args } }`) — the shape `dispatch_click`
    /// consumes. No `<Module>` element required.
    #[test]
    fn use_events_property_call_produces_intent_with_merged_args() {
        let result = eval(
            r#"export default function render() {
const notify = useEvents("/usr/bin/notify", {});
return <div class="flex" on_click={[notify.dismiss({ id: 42 })]} />;
}"#,
        )
        .layout;
        assert_eq!(
            result["on_click"],
            serde_json::json!([
                { "channel": "/usr/bin/notify", "event": { "type": "dismiss", "id": 42 } }
            ]),
            "expected a single intent with merged args, got: {:?}",
            result["on_click"]
        );
    }

    /// Calling a proxy property with no argument is valid: the event carries only `type`.
    #[test]
    fn use_events_property_call_without_args_yields_type_only_event() {
        let result = eval(
            r#"export default function render() {
const notify = useEvents("/usr/bin/notify", {});
return <div class="flex" on_click={[notify.dismiss()]} />;
}"#,
        )
        .layout;
        assert_eq!(
            result["on_click"],
            serde_json::json!([
                { "channel": "/usr/bin/notify", "event": { "type": "dismiss" } }
            ]),
            "expected an event with `type` and no other keys, got: {:?}",
            result["on_click"]
        );
    }

    /// `useEvents` registers the module itself — a layout can spawn a module without
    /// ever rendering a `<Module>` element.
    #[test]
    fn use_events_registers_module_without_module_element() {
        let module_calls = eval(
            r#"export default function render() {
const notify = useEvents("/usr/bin/notify-module", { limit: 5 });
return <span class="text-white">hi</span>;
}"#,
        )
        .module_calls;
        assert!(
            module_calls
                .iter()
                .any(|(bin, _)| bin == "/usr/bin/notify-module"),
            "useEvents must register the bin as a module; got: {:?}",
            module_calls
        );
    }

    /// A bin registered more than once keeps every prop any registration declared.
    ///
    /// This used to be first-wins, which silently dropped the later props. A
    /// wrapper that computes something from its children — layout geometry, say —
    /// can only register *after* them, so first-wins made such a wrapper
    /// impossible to write.
    #[test]
    fn a_later_registration_contributes_its_props() {
        let module_calls = eval(
            r#"export default function render() {
useEvents("/usr/bin/mod");
useEvents("/usr/bin/mod", { gaps: { left: 272 } });
return <span class="text-white">hi</span>;
}"#,
        )
        .module_calls;
        let (_, props) = module_calls
            .iter()
            .find(|(bin, _)| bin == "/usr/bin/mod")
            .expect("the bin must be registered");
        assert_eq!(props["gaps"]["left"].as_u64(), Some(272));
    }

    /// One subprocess per bin, however many times it is registered.
    #[test]
    fn registering_a_bin_twice_still_yields_one_module() {
        let module_calls = eval(
            r#"export default function render() {
useEvents("/usr/bin/mod", { a: 1 });
useEvents("/usr/bin/mod", { b: 2 });
return <span class="text-white">hi</span>;
}"#,
        )
        .module_calls;
        assert_eq!(
            module_calls
                .iter()
                .filter(|(b, _)| b == "/usr/bin/mod")
                .count(),
            1
        );
    }

    /// Merging is additive: an earlier declaration is never overwritten by a
    /// later one, so a wrapper can only fill gaps, never clobber the author.
    #[test]
    fn the_first_registration_wins_a_conflicting_key() {
        let module_calls = eval(
            r#"export default function render() {
useEvents("/usr/bin/mod", { gaps: "mine" });
useEvents("/usr/bin/mod", { gaps: "theirs" });
return <span class="text-white">hi</span>;
}"#,
        )
        .module_calls;
        let (_, props) = module_calls
            .iter()
            .find(|(bin, _)| bin == "/usr/bin/mod")
            .unwrap();
        assert_eq!(props["gaps"].as_str(), Some("mine"));
    }

    /// The `<Module>` render prop's `events` argument must produce the very same
    /// intent shape as `useEvents` (it is implemented on top of it).
    #[test]
    fn module_render_prop_events_produce_same_intent_shape() {
        let result = eval(
            r#"export default function render() { return <Module bin="/usr/bin/test">{(data, events) => <div class="flex" on_click={[events.doThing({ id: 7 })]} />}</Module>; }"#,
        )
        .layout;
        assert_eq!(
            result["on_click"],
            serde_json::json!([
                { "channel": "/usr/bin/test", "event": { "type": "doThing", "id": 7 } }
            ]),
            "expected Module's events proxy to yield the new intent shape, got: {:?}",
            result["on_click"]
        );
    }

    /// `useDispatch(bin, props)` is the name for what `useEvents` did: its property
    /// calls build the intent `dispatch_click` consumes.
    #[test]
    fn use_dispatch_property_call_produces_intent_with_merged_args() {
        let result = eval(
            r#"export default function render() {
const notify = useDispatch("/usr/bin/notify");
return <div class="flex" on_click={[notify.dismiss({ id: 42 }), notify.clear()]} />;
}"#,
        )
        .layout;
        assert_eq!(
            result["on_click"],
            serde_json::json!([
                { "channel": "/usr/bin/notify", "event": { "type": "dismiss", "id": 42 } },
                { "channel": "/usr/bin/notify", "event": { "type": "clear" } }
            ]),
        );
    }

    /// `useDispatch` starts the subprocess itself, and every call's props reach it.
    #[test]
    fn use_dispatch_registers_the_subprocess_with_merged_props() {
        let module_calls = eval(
            r#"export default function render() {
useDispatch("/usr/bin/mod", { a: 1 });
useDispatch("/usr/bin/mod", { b: 2 });
return <span class="text-white">hi</span>;
}"#,
        )
        .module_calls;
        assert_eq!(
            module_calls,
            vec![(
                "/usr/bin/mod".to_string(),
                serde_json::json!({ "a": 1, "b": 2 })
            )]
        );
    }

    /// The deprecated names are the new ones, not copies of them.
    #[test]
    fn use_events_is_use_dispatch_under_its_old_name() {
        let result = eval(
            r#"export default function render() {
return <span class="text-white">{String(useEvents === useDispatch)}</span>;
}"#,
        )
        .layout;
        assert_eq!(result["children"][0], "true");
    }

    const SERVICE_LAYOUT: &str = r#"export default function render() {
const [state, dispatch] = useService("/usr/bin/svc", { limit: 5 });
return <div class="flex" on_click={[dispatch.next({ step: 2 })]}>{state === null ? "null" : state.name}</div>;
}"#;

    fn eval_service(line: Option<&str>) -> EvalOutput {
        let mut streams = std::collections::HashMap::new();
        if let Some(line) = line {
            streams.insert(("/usr/bin/svc".to_string(), None), line.to_string());
        }
        JsxEvaluator::new(SERVICE_LAYOUT, serde_json::Value::Null, None)
            .unwrap()
            .eval(&streams)
            .unwrap()
    }

    /// A Service has said nothing yet: its state is `null`, as `useJSONStream`'s is.
    #[test]
    fn use_service_state_is_null_before_the_first_line() {
        assert_eq!(eval_service(None).layout["children"][0], "null");
    }

    #[test]
    fn use_service_state_is_the_parsed_latest_line() {
        let layout = eval_service(Some(r#"{"name":"hello"}"#)).layout;
        assert_eq!(layout["children"][0], "hello");
    }

    #[test]
    fn use_service_dispatch_builds_intents_for_its_bin() {
        assert_eq!(
            eval_service(None).layout["on_click"],
            serde_json::json!([
                { "channel": "/usr/bin/svc", "event": { "type": "next", "step": 2 } }
            ]),
        );
    }

    #[test]
    fn use_service_registers_the_subprocess_with_its_props() {
        assert_eq!(
            eval_service(None).module_calls,
            vec![(
                "/usr/bin/svc".to_string(),
                serde_json::json!({ "limit": 5 })
            )]
        );
    }

    /// However a layout reaches a bin — `useService` twice, or beside
    /// `useJSONStream` and `useDispatch` — it is one registration, and every read
    /// is of the bin with no script, which `apply_eval_result` (`src/app.rs`) folds into it.
    #[test]
    fn a_service_reached_several_ways_is_still_one_subprocess() {
        let out = eval(
            r#"export default function render() {
useService("/usr/bin/svc", { a: 1 });
useService("/usr/bin/svc", { b: 2 });
useJSONStream("/usr/bin/svc");
useDispatch("/usr/bin/svc");
return <span class="text-white">hi</span>;
}"#,
        );
        assert_eq!(
            out.module_calls,
            vec![(
                "/usr/bin/svc".to_string(),
                serde_json::json!({ "a": 1, "b": 2 })
            )]
        );
        assert!(
            out.stream_calls
                .iter()
                .all(|call| call == &("/usr/bin/svc".to_string(), None)),
            "got: {:?}",
            out.stream_calls
        );
    }

    /// `<Module>` is `useService` as a render prop: same state, same dispatch.
    #[test]
    fn module_render_prop_gets_what_use_service_returns() {
        let mut streams = std::collections::HashMap::new();
        streams.insert(
            ("/usr/bin/svc".to_string(), None),
            r#"{"name":"hello"}"#.to_string(),
        );
        let layout = JsxEvaluator::new(
            r#"export default function render() { return <Module bin="/usr/bin/svc">{(data, events) => <div class="flex" on_click={[events.next()]}>{data.name}</div>}</Module>; }"#,
            serde_json::Value::Null,
            None,
        )
        .unwrap()
        .eval(&streams)
        .unwrap()
        .layout;
        assert_eq!(layout["children"][0], "hello");
        assert_eq!(
            layout["on_click"],
            serde_json::json!([{ "channel": "/usr/bin/svc", "event": { "type": "next" } }]),
        );
    }

    #[test]
    fn use_json_stream_parses_latest_json_output() {
        let mut streams = std::collections::HashMap::new();
        streams.insert(
            ("/usr/bin/test".to_string(), None),
            r#"{"name":"hello"}"#.to_string(),
        );
        let result = JsxEvaluator::new(
            r#"export default function render() { return <span class="text-white">{useJSONStream("/usr/bin/test").name}</span>; }"#,
            serde_json::Value::Null,
            None,
        ).unwrap().eval(&streams).unwrap().layout;
        assert_eq!(result["children"][0], "hello");
    }

    #[test]
    fn module_component_records_module_call() {
        let module_calls = eval(
            r#"export default function render() { return <Module bin="/usr/bin/test-module">{(data, events) => <span class="text-white">hi</span>}</Module>; }"#,
        ).module_calls;
        assert!(module_calls
            .iter()
            .any(|(bin, _)| bin == "/usr/bin/test-module"));
    }

    const ONE_FIGURE: &str = r#"export default function render() {
return <div><D3Figure key="cpu/1" width={248} height={100} draw={(svg, d3) => {}} /></div>;
}"#;

    #[test]
    fn a_d3_figure_renders_as_an_image_naming_its_picture() {
        let layout = eval(ONE_FIGURE).layout;
        let img = &layout["children"][0];
        assert_eq!(img["type"], "img", "got: {layout}");
        // tauler-core's `D3Figure` writes this prefix by hand (it cannot depend
        // on tauler): this is what holds it to the one the renderer binds.
        assert_eq!(
            img["src"],
            format!(
                "{}cpu/1@248x100",
                crate::render::picture::PICTURE_SRC_PREFIX
            ),
            "got: {layout}"
        );
        assert_eq!(img["style"]["width"], 248, "got: {layout}");
        assert_eq!(img["style"]["height"], 100, "got: {layout}");
    }

    #[test]
    fn a_d3_figure_records_a_draw_request_with_its_key_and_size() {
        assert_eq!(
            eval(ONE_FIGURE).figure_calls,
            vec![FigureCall {
                key: "cpu/1@248x100".into(),
                width: 248.0,
                height: 100.0
            }]
        );
    }

    #[test]
    fn two_d3_figures_record_two_requests_in_document_order() {
        let calls = eval(
            r#"export default function render() {
return <div>
  <D3Figure key="a" width={10} height={20} draw={() => {}} />
  <D3Figure key="b" width={30} height={40} draw={() => {}} />
</div>;
}"#,
        )
        .figure_calls;
        assert_eq!(
            calls,
            vec![
                FigureCall {
                    key: "a@10x20".into(),
                    width: 10.0,
                    height: 20.0
                },
                FigureCall {
                    key: "b@30x40".into(),
                    width: 30.0,
                    height: 40.0
                },
            ]
        );
    }

    #[test]
    fn draw_requests_do_not_carry_over_between_evals() {
        let evaluator = JsxEvaluator::new(ONE_FIGURE, serde_json::Value::Null, None).unwrap();
        let streams = std::collections::HashMap::new();
        evaluator.eval(&streams).unwrap();
        let second = evaluator.eval(&streams).unwrap().figure_calls;
        assert_eq!(
            second,
            vec![FigureCall {
                key: "cpu/1@248x100".into(),
                width: 248.0,
                height: 100.0
            }],
            "the second eval must hold only its own request"
        );
    }

    /// A D3Figure element built once, when the module loads, and returned by
    /// every render: its `registerFigure` call happens before any eval.
    const MODULE_SCOPE_FIGURE: &str = r#"const chart = <D3Figure key="k" width={10} height={20}
  draw={(svg) => svg.append("circle").attr("r", 3)} />;
export default function render() {
return <div>{chart}</div>;
}"#;

    #[test]
    fn a_d3_figure_built_at_module_scope_is_requested_by_every_eval() {
        let evaluator =
            JsxEvaluator::new(MODULE_SCOPE_FIGURE, serde_json::Value::Null, None).unwrap();
        let streams = std::collections::HashMap::new();
        let expected = vec![FigureCall {
            key: "k@10x20".into(),
            width: 10.0,
            height: 20.0,
        }];
        assert_eq!(
            evaluator.eval(&streams).unwrap().figure_calls,
            expected,
            "the figure is in the tree, so the first eval must request it"
        );
        assert_eq!(
            evaluator.eval(&streams).unwrap().figure_calls,
            expected,
            "and so must every later one"
        );
    }

    #[test]
    fn figure_calls_follow_document_order_in_the_tree() {
        let calls = eval(
            r#"const late = <D3Figure key="b" width={30} height={40} draw={() => {}} />;
export default function render() {
const early = <D3Figure key="a" width={10} height={20} draw={() => {}} />;
return <div><div>{late}</div>{early}</div>;
}"#,
        )
        .figure_calls;
        let keys: Vec<&str> = calls.iter().map(|c| c.key.as_str()).collect();
        assert_eq!(
            keys,
            ["b@30x40", "a@10x20"],
            "document order, not the order the elements were built in"
        );
    }

    #[test]
    fn a_d3_figure_without_a_key_fails_the_eval_with_an_error_that_mentions_key() {
        let err = JsxEvaluator::new(
            r#"export default function render() {
return <div><D3Figure width={248} height={100} draw={() => {}} /></div>;
}"#,
            serde_json::Value::Null,
            None,
        )
        .unwrap()
        .eval(&std::collections::HashMap::new())
        .err()
        .expect("a D3Figure without a key must fail the eval");
        assert!(
            err.to_string().contains("key"),
            "the error must name the missing `key`, got: {err}"
        );
    }

    /// The eval error of a layout with one `<D3Figure>` carrying exactly `props`.
    fn d3_figure_with_props_eval_error(props: &str) -> String {
        JsxEvaluator::new(
            &format!(
                r#"export default function render() {{
return <div><D3Figure {props} draw={{() => {{}}}} /></div>;
}}"#
            ),
            serde_json::Value::Null,
            None,
        )
        .unwrap()
        .eval(&std::collections::HashMap::new())
        .err()
        .unwrap_or_else(|| panic!("a D3Figure with {props} must fail the eval"))
        .to_string()
    }

    /// The eval error of a layout with one `<D3Figure key="k">` carrying `props`.
    fn d3_figure_eval_error(props: &str) -> String {
        d3_figure_with_props_eval_error(&format!(r#"key="k" {props}"#))
    }

    /// The eval error of a layout with one `<D3Figure>` keyed by `key_expr`.
    fn d3_figure_key_eval_error(key_expr: &str) -> String {
        d3_figure_with_props_eval_error(&format!("key={{{key_expr}}} width={{1}} height={{2}}"))
    }

    #[test]
    fn a_d3_figure_without_a_width_fails_the_eval_with_an_error_that_mentions_width() {
        let err = d3_figure_eval_error("height={100}");
        assert!(
            err.contains("<D3Figure> needs a numeric width"),
            "the error must name the missing `width`, got: {err}"
        );
    }

    #[test]
    fn a_d3_figure_with_a_string_height_fails_the_eval_with_an_error_that_mentions_height() {
        let err = d3_figure_eval_error(r#"width={248} height="100""#);
        assert!(
            err.contains("<D3Figure> needs a numeric width and height"),
            "numbers only, so a string height must be refused by name, got: {err}"
        );
    }

    #[test]
    fn a_d3_figure_does_not_run_its_draw_function_in_the_render_runtime() {
        let evaluator = JsxEvaluator::new(
            r#"export default function render() {
globalThis.__drew = false;
return <div><D3Figure key="k" width={1} height={1}
  draw={() => { globalThis.__drew = true; throw new Error("draw ran"); }} /></div>;
}"#,
            serde_json::Value::Null,
            None,
        )
        .unwrap();
        evaluator
            .eval(&std::collections::HashMap::new())
            .expect("draw must not run, so its throw must not fail the eval");
        let drew = evaluator
            .context
            .with(|ctx| ctx.eval::<bool, _>("globalThis.__drew").unwrap());
        assert!(!drew, "draw ran in the render runtime");
    }

    #[test]
    fn a_numeric_key_is_accepted_and_stringified() {
        let out = eval(
            r#"export default function render() {
return <div><D3Figure key={42} width={1} height={2} draw={() => {}} /></div>;
}"#,
        );
        assert_eq!(
            out.layout["children"][0]["src"],
            format!("{}42@1x2", crate::render::picture::PICTURE_SRC_PREFIX)
        );
        assert_eq!(
            out.figure_calls,
            vec![FigureCall {
                key: "42@1x2".into(),
                width: 1.0,
                height: 2.0
            }]
        );
    }

    #[test]
    fn an_eval_with_a_figure_records_every_stream() {
        let evaluator = JsxEvaluator::new(
            r#"export default function render() {
return <div><D3Figure key={useStringStream("/bin/sh", "a")} width={1} height={1} draw={() => {}} /></div>;
}"#,
            serde_json::Value::Null,
            None,
        )
        .unwrap();
        let a = ("/bin/sh".to_string(), Some("a".to_string()));
        let b = ("/bin/sh".to_string(), Some("b".to_string()));
        let streams = HashMap::from([(a, "1".to_string()), (b, "unread".to_string())]);

        let out = evaluator.eval(&streams).unwrap();

        assert_eq!(out.inputs.stream_values, streams);
    }

    #[test]
    fn an_eval_with_no_figure_keeps_no_stream_values() {
        let evaluator = JsxEvaluator::new(
            r#"export default function render() {
return <span>{useStringStream("/bin/sh", "a")}</span>;
}"#,
            serde_json::Value::Null,
            None,
        )
        .unwrap();
        let streams = HashMap::from([(
            ("/bin/sh".to_string(), Some("a".to_string())),
            "1".to_string(),
        )]);

        let out = evaluator.eval(&streams).unwrap();

        assert!(out.inputs.stream_values.is_empty());
    }

    #[test]
    fn object_keys_name_their_pictures_by_a_hash_of_their_json() {
        // `JsxEvaluator::new` is the render runtime, with effects denied: the
        // hash helper must be there too, not only behind `Effects::Allowed`.
        let out = eval(
            r#"export default function render() {
return <div>
  <D3Figure key={{a: 1}} width={10} height={20} draw={() => {}} />
  <D3Figure key={{a: 2}} width={10} height={20} draw={() => {}} />
</div>;
}"#,
        );
        // The first 16 hex chars of sha256('{"a":1}') and sha256('{"a":2}').
        let names = ["json-015abd7f5cc57a2d@10x20", "json-7e8059f495589fcd@10x20"];
        let keys: Vec<&str> = out.figure_calls.iter().map(|c| c.key.as_str()).collect();
        assert_eq!(keys, names, "got: {}", out.layout);
        for (i, name) in names.iter().enumerate() {
            assert_eq!(
                out.layout["children"][i]["src"],
                format!("{}{name}", crate::render::picture::PICTURE_SRC_PREFIX),
                "got: {}",
                out.layout
            );
        }
    }

    #[test]
    fn a_boolean_key_keeps_its_string_name() {
        let out = eval(
            r#"export default function render() {
return <div><D3Figure key={true} width={1} height={2} draw={() => {}} /></div>;
}"#,
        );
        assert_eq!(
            out.figure_calls,
            vec![FigureCall {
                key: "true@1x2".into(),
                width: 1.0,
                height: 2.0
            }]
        );
    }

    #[rstest::rstest]
    #[case::nan("NaN", "NaN@1x2")]
    #[case::infinity("Infinity", "Infinity@1x2")]
    #[case::negative_infinity("-Infinity", "-Infinity@1x2")]
    fn a_non_finite_number_key_keeps_its_string_name(#[case] key: &str, #[case] name: &str) {
        // A primitive key is never JSON-stringified, so nothing is lost: it is
        // named `String(key)` as before, e.g. `total / count` with count 0.
        let out = eval(&format!(
            r#"export default function render() {{
return <div><D3Figure key={{{key}}} width={{1}} height={{2}} draw={{() => {{}}}} /></div>;
}}"#
        ));
        let keys: Vec<&str> = out.figure_calls.iter().map(|c| c.key.as_str()).collect();
        assert_eq!(keys, [name], "got: {}", out.layout);
    }

    #[rstest::rstest]
    #[case::function("{f: () => 1}", "key.f")]
    #[case::undefined("{u: undefined}", "key.u")]
    #[case::symbol("{s: Symbol()}", "key.s")]
    #[case::nan("{n: NaN}", "key.n")]
    #[case::infinity("{i: Infinity}", "key.i")]
    #[case::negative_infinity("{i: -Infinity}", "key.i")]
    #[case::bigint("{b: 1n}", "key.b")]
    #[case::nested_date("{rows: [{t: 1}, {t: new Date(0)}]}", "key.rows[1].t")]
    #[case::array_hole("[1,,2]", "key[1]")]
    #[case::map("{m: new Map()}", "key.m")]
    #[case::set("{s: new Set()}", "key.s")]
    #[case::class_instance("{c: new (class C {})()}", "key.c")]
    #[case::cycle("(() => { const o = {}; o.self = o; return o; })()", "key.self")]
    #[case::non_identifier_name(r#"{"a-b": [0, NaN]}"#, r#"key["a-b"][1]"#)]
    #[case::whole_key_function("() => 1", "(at key)")]
    #[case::whole_key_symbol("Symbol()", "(at key)")]
    #[case::whole_key_bigint("1n", "(at key)")]
    fn a_key_holding_a_lossy_value_fails_naming_its_path(#[case] key: &str, #[case] path: &str) {
        let err = d3_figure_key_eval_error(key);
        assert!(
            err.contains("<D3Figure>") && err.contains(path),
            "a key JSON would drop or mangle must be refused at {path}, got: {err}"
        );
    }

    #[test]
    fn a_null_prototype_object_key_is_accepted() {
        let out = eval(
            r#"export default function render() {
return <div><D3Figure key={Object.assign(Object.create(null), {a: 1})} width={10} height={20} draw={() => {}} /></div>;
}"#,
        );
        let keys: Vec<&str> = out.figure_calls.iter().map(|c| c.key.as_str()).collect();
        assert_eq!(keys, ["json-015abd7f5cc57a2d@10x20"], "got: {}", out.layout);
    }

    #[test]
    fn a_key_reaching_one_object_twice_without_a_cycle_is_accepted() {
        // Shared but acyclic, like `{ samples, latest: samples.at(-1) }`: only
        // a true cycle is lossy, so a seen-set check would wrongly refuse this.
        let out = eval(
            r#"export default function render() {
const x = {a: 1};
return <div><D3Figure key={{p: x, q: [x]}} width={10} height={20} draw={() => {}} /></div>;
}"#,
        );
        // The first 16 hex chars of sha256('{"p":{"a":1},"q":[{"a":1}]}').
        let keys: Vec<&str> = out.figure_calls.iter().map(|c| c.key.as_str()).collect();
        assert_eq!(keys, ["json-f2f7ebe41634361f@10x20"], "got: {}", out.layout);
    }

    #[test]
    fn jsx_evaluator_reflects_updated_stream_values_on_second_call() {
        let evaluator = JsxEvaluator::new(
            r#"export default function render() { return <span class="text-white">{useStringStream("/bin/bash", "echo hi")}</span>; }"#,
            serde_json::Value::Null,
            None,
        ).unwrap();

        let mut streams1 = std::collections::HashMap::new();
        streams1.insert(
            ("/bin/bash".to_string(), Some("echo hi".to_string())),
            "first".to_string(),
        );
        let result1 = evaluator.eval(&streams1).unwrap().layout;
        assert_eq!(result1["children"][0], "first");

        let mut streams2 = std::collections::HashMap::new();
        streams2.insert(
            ("/bin/bash".to_string(), Some("echo hi".to_string())),
            "second".to_string(),
        );
        let result2 = evaluator.eval(&streams2).unwrap().layout;
        assert_eq!(result2["children"][0], "second");
    }

    /// Regression: stream keys `(bin, None)` and `(bin, Some(""))` must be distinct.
    #[test]
    fn stream_key_none_and_some_empty_are_not_interchangeable() {
        let key_for_none: (String, Option<String>) = ("/usr/bin/foo".to_string(), None);
        let key_for_empty: (String, Option<String>) =
            ("/usr/bin/foo".to_string(), Some("".to_string()));
        let mut map: std::collections::HashMap<(String, Option<String>), &str> =
            std::collections::HashMap::new();
        map.insert(key_for_none, "value_for_none");
        map.insert(key_for_empty, "value_for_empty");
        assert_eq!(
            map.len(),
            2,
            "(bin, None) and (bin, Some(\"\")) must be distinct map keys"
        );
    }

    #[test]
    fn jsx_evaluator_supports_export_default_render_function() {
        let streams = std::collections::HashMap::new();
        let result = JsxEvaluator::new(
            r#"export default function render() { return <span class="text-white">hello</span>; }"#,
            serde_json::Value::Null,
            None,
        )
        .unwrap()
        .eval(&streams)
        .unwrap()
        .layout;
        assert_eq!(
            result["type"], "span",
            "expected type=span, got: {:?}",
            result
        );
        assert_eq!(
            result["children"][0], "hello",
            "expected the text child 'hello', got: {:?}",
            result
        );
    }

    #[test]
    fn jsx_evaluator_resolves_sibling_import_from_disk() {
        let tmp_dir =
            std::env::temp_dir().join(format!("tauler_sibling_import_{}", std::process::id()));
        std::fs::create_dir_all(&tmp_dir).expect("failed to create temp dir");
        std::fs::write(
            tmp_dir.join("Foo.jsx"),
            "export default function Foo() { return 42; }",
        )
        .expect("failed to write Foo.jsx");

        let layout_source = r#"import Foo from './Foo.jsx';
export default function render() { return <span class="text-white">{String(Foo())}</span>; }"#;

        let streams = std::collections::HashMap::new();
        let result = JsxEvaluator::new(
            layout_source,
            serde_json::Value::Null,
            Some(&tmp_dir.join("layout.jsx")),
        )
        .unwrap()
        .eval(&streams)
        .unwrap()
        .layout;

        assert_eq!(
            result["children"][0], "42",
            "expected the text child '42' from imported Foo, got: {:?}",
            result
        );
        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    /// A Package context over `lockfile_path` and `cache_root`, with a fetch manager
    /// whose reload signals nobody reads.
    fn package_context(lockfile_path: PathBuf, cache_root: PathBuf) -> crate::pkg::PackageContext {
        let (reload_tx, _reload_rx) = std::sync::mpsc::channel();
        crate::pkg::PackageContext {
            fetch_manager: Arc::new(crate::pkg::fetch_manager::FetchManager::new(reload_tx)),
            lockfile_path,
            // Offline over an empty directory: these tests import no d3.
            d3: Arc::new(crate::d3_figure::fetch::Fetcher::offline(
                cache_root.join("d3"),
            )),
            cache_root,
        }
    }

    /// A Package whose `index.jsx` default-exports `Widget`, returning 42.
    const WIDGET_PACKAGE: &[(&str, &str, bool)] = &[(
        "index.jsx",
        "export default function Widget() { return 42; }",
        false,
    )];

    /// The full path, end to end, for an already-pinned-and-cached Package: the
    /// Lockfile names a commit, that commit's checkout is already on disk (placed
    /// here exactly the way a real fetch would — via `pkg::git::fetch_and_place`
    /// against a local fixture repo, no network needed), and
    /// `JsxEvaluator::new_with_packages` resolves, loads and evaluates it like any
    /// other import. This is the resolver's warm path: Lockfile lookup → cache
    /// path → nested `ConfinedFsResolver` extension fallback → the existing
    /// `SchemaAwareLoader`/`ConfinedFsLoader` load-and-transform → QuickJS module
    /// linking, exercised for real, not mocked at any layer.
    #[test]
    fn jsx_evaluator_resolves_a_warm_git_package_end_to_end() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (layout_dir, pkg_ctx, _) = warm_package(tmp.path(), WIDGET_PACKAGE);

        let layout_source = r#"import Widget from "@gh/fixture/repo";
export default function render() { return <span class="text-white">{String(Widget())}</span>; }"#;

        let result = JsxEvaluator::new_with_packages(
            layout_source,
            serde_json::Value::Null,
            Some(&layout_dir.join("layout.jsx")),
            pkg_ctx,
            Default::default(),
        )
        .unwrap_or_else(|e| panic!("expected the warm Package to resolve, got: {e}"))
        .eval(&std::collections::HashMap::new())
        .unwrap()
        .layout;

        assert_eq!(
            result["children"][0], "42",
            "expected the text child '42' from the imported Package's Widget(), got: {:?}",
            result
        );
    }

    /// A Package whose `index.jsx` itself imports a sibling file
    /// (`./helper.jsx`) — found by review to be broken: `GitPackageResolver`
    /// only special-cased the top-level `@gh/owner/repo` specifier, so a
    /// relative import *inside* the Package's own source fell through to the
    /// layout's `ConfinedFsResolver`, confined to the layout's own directory,
    /// and was rejected as escaping it. Any Package with more than one file
    /// was broken. `package_root_for` fixes this by recognizing that `base`
    /// lives inside a Package's checkout and re-scoping confinement to that
    /// checkout's own root.
    #[test]
    fn jsx_evaluator_resolves_a_multi_file_git_package_end_to_end() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (layout_dir, pkg_ctx, _) = warm_package(
            tmp.path(),
            &[
                (
                    "helper.jsx",
                    "export function helperValue() { return 99; }",
                    false,
                ),
                (
                    "index.jsx",
                    "import { helperValue } from './helper.jsx';\n\
                     export default function Widget() { return helperValue(); }",
                    false,
                ),
            ],
        );

        let layout_source = r#"import Widget from "@gh/fixture/repo";
export default function render() { return <span class="text-white">{String(Widget())}</span>; }"#;

        let result = JsxEvaluator::new_with_packages(
            layout_source,
            serde_json::Value::Null,
            Some(&layout_dir.join("layout.jsx")),
            pkg_ctx,
            Default::default(),
        )
        .unwrap_or_else(|e| {
            panic!("expected the multi-file Package's internal import to resolve, got: {e}")
        })
        .eval(&std::collections::HashMap::new())
        .unwrap()
        .layout;

        assert_eq!(
            result["children"][0], "99",
            "expected the text child '99' from helper.jsx via index.jsx, got: {:?}",
            result
        );
    }

    /// A Package with no Lockfile entry and nothing cached yet (the ordinary
    /// startup/reload case for a brand-new import, ADR 0041's "Discovery" and
    /// "Unified boot/reload policy" sections) fails resolution gracefully — an
    /// `Err`, never a panic — the same as any other unresolvable import already
    /// does. This is what lets `initial_load`/`handle_layout_reload` fall into
    /// their existing non-fatal "log and don't touch live state" branch instead of
    /// needing a new one.
    #[test]
    fn jsx_evaluator_fails_gracefully_on_a_cold_git_package() {
        let dir = std::env::temp_dir().join(format!("tauler_git_pkg_cold_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create dir");
        let lockfile_path = dir.join("tauler-pkg.lock");
        let cache_root = dir.join("cache");

        let pkg_ctx = package_context(lockfile_path, cache_root);

        let layout_source = r#"import Widget from "@gh/fixture/repo";
export default function render() { return <span class="text-white">{String(Widget())}</span>; }"#;

        let result = JsxEvaluator::new_with_packages(
            layout_source,
            serde_json::Value::Null,
            Some(&dir.join("layout.jsx")),
            pkg_ctx,
            Default::default(),
        );

        assert!(
            result.is_err(),
            "a cold Package must fail resolution, not panic or hang"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The same `@gh/owner/repo` specifier from a plain `JsxEvaluator::new` (no
    /// `PackageContext` at all) fails exactly like any other unrecognized bare
    /// specifier — every one of the ~35 existing call sites across this codebase
    /// that never opted into package support keeps behaving exactly as it did
    /// before this feature existed.
    #[test]
    fn a_git_package_specifier_without_a_package_context_fails_like_any_unknown_import() {
        let result = JsxEvaluator::new(
            r#"import Widget from "@gh/fixture/repo";
export default function render() { return <root>{String(Widget())}</root>; }"#,
            serde_json::Value::Null,
            None,
        );
        assert!(result.is_err());
    }

    /// The req #7 escape hatch, exercised end to end: a Development-mode entry
    /// (`development: true`, no pin enforced) resolves through the
    /// `development-<hash>` cache path, not a commit sha, and evaluates exactly
    /// like a pinned Package once its checkout is warm.
    #[test]
    fn jsx_evaluator_resolves_a_warm_development_mode_package_end_to_end() {
        let dir =
            std::env::temp_dir().join(format!("tauler_git_pkg_dev_mode_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create dir");
        let (remote, sha) = crate::pkg::git::fixture_repo(&dir, WIDGET_PACKAGE);

        let layout_dir = dir.join("layout");
        std::fs::create_dir_all(&layout_dir).expect("create layout dir");
        let lockfile_path = layout_dir.join("tauler-pkg.lock");
        let cache_root = dir.join("cache");

        let mut lockfile = crate::pkg::lockfile::Lockfile::default();
        lockfile.set(
            "fixture",
            "repo",
            crate::pkg::lockfile::PackageEntry {
                commit: sha.clone(),
                development: true,
            },
        );
        lockfile
            .save_to_path(&lockfile_path)
            .expect("saving the lockfile should succeed");

        // Placed at the *development* ref, not the commit sha — proving the
        // resolver actually reads `entry.development` rather than always using
        // the pinned commit.
        let dev_ref = crate::pkg::cache::development_ref(&lockfile_path)
            .expect("development_ref should succeed");
        let final_path =
            crate::pkg::cache::pinned_cache_path(&cache_root, "fixture", "repo", &dev_ref);
        crate::pkg::git::fetch_and_place(&remote.to_string_lossy(), &sha, &cache_root, &final_path)
            .expect("pre-warming the cache should succeed");

        let pkg_ctx = package_context(lockfile_path, cache_root);

        let layout_source = r#"import Widget from "@gh/fixture/repo";
export default function render() { return <span class="text-white">{String(Widget())}</span>; }"#;

        let result = JsxEvaluator::new_with_packages(
            layout_source,
            serde_json::Value::Null,
            Some(&layout_dir.join("layout.jsx")),
            pkg_ctx,
            Default::default(),
        )
        .unwrap_or_else(|e| panic!("expected the Development-mode Package to resolve, got: {e}"))
        .eval(&std::collections::HashMap::new())
        .unwrap()
        .layout;

        assert_eq!(
            result["children"][0], "42",
            "expected the text child '42' from the Development-mode Package's Widget(), got: {:?}",
            result
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn loaded_paths_includes_imported_sibling() {
        let tmp_dir =
            std::env::temp_dir().join(format!("tauler_loaded_paths_{}", std::process::id()));
        std::fs::create_dir_all(&tmp_dir).expect("failed to create temp dir");
        std::fs::write(
            tmp_dir.join("Comp.jsx"),
            "export default function Comp() { return 1; }",
        )
        .expect("failed to write Comp.jsx");

        let layout_source = r#"import Comp from './Comp.jsx';
export default function render() { return <span class="text-white">{String(Comp())}</span>; }"#;

        let evaluator = JsxEvaluator::new(
            layout_source,
            serde_json::Value::Null,
            Some(&tmp_dir.join("layout.jsx")),
        )
        .expect("JsxEvaluator::new failed");

        let canonical_comp = tmp_dir
            .join("Comp.jsx")
            .canonicalize()
            .expect("canonicalize failed");

        let paths = evaluator.loaded_paths();
        let _ = std::fs::remove_dir_all(&tmp_dir);

        assert!(
            paths.contains(&canonical_comp),
            "loaded_paths() must include the canonicalized path of Comp.jsx; got: {:?}",
            paths
        );
    }

    #[test]
    fn loaded_paths_is_empty_when_no_imports() {
        let evaluator = JsxEvaluator::new(
            r#"export default function render() { return <span class="text-white">hi</span>; }"#,
            serde_json::Value::Null,
            None,
        )
        .expect("JsxEvaluator::new failed");

        let paths = evaluator.loaded_paths();
        assert!(
            paths.is_empty(),
            "loaded_paths() must be empty when there are no imports; got: {:?}",
            paths
        );
    }

    #[test]
    fn jsx_null_and_false_children_are_filtered_from_container() {
        let result = eval(
            r#"export default function render() {
const show = false;
return <div class="flex">
  <span class="text-white">visible</span>
  {show && <span class="text-white">hidden</span>}
  {null}
</div>;
}"#,
        )
        .layout;
        let children = result["children"].as_array().unwrap();
        assert_eq!(children.len(), 1, "expected 1 child, got: {:?}", children);
        assert_eq!(children[0]["children"][0], "visible");
    }

    /// Bonus: JSX fragment shorthand (`<>...</>`) now actually works and flattens its
    /// children into the parent's `children` array with no wrapper — tauler could never
    /// do this before, since the old JS runtime's `_jsx` never defined `_jsxFrag`.
    #[test]
    fn jsx_fragment_shorthand_flattens_into_parent_children() {
        let result = eval(
            r#"export default function render() {
return <div class="flex">
  <>
    <span class="a">{"first"}</span>
    <span class="b">{"second"}</span>
  </>
</div>;
}"#,
        )
        .layout;
        let children = result["children"].as_array().unwrap();
        assert_eq!(
            children.len(),
            2,
            "expected 2 children, got: {:?}",
            children
        );
        assert_eq!(children[0]["children"][0], "first");
        assert_eq!(children[1]["children"][0], "second");
    }

    /// The geometry these two exercise — a misplaced or repeated `<Workspaces>`
    /// still producing panels, just from the last one declared — is covered in
    /// `tests/workspaces_shim_test.rs`. What only a unit test *here* can check is
    /// the warning itself: `tracing_test`'s auto env-filter scopes to the test's
    /// own crate name, so from an external integration test a warning `tauler::jsx`
    /// logs is filtered out before it ever reaches the capture buffer. From inside
    /// this crate, the filter matches.
    fn eval_with_screen(source: &str) -> EvalOutput {
        crate::init_global_ctx(crate::config::FontConfig::default());
        let ctx = serde_json::json!({"screen_width": 1920, "screen_height": 1080});
        JsxEvaluator::new(source, ctx, None)
            .unwrap()
            .eval(&std::collections::HashMap::new())
            .unwrap()
    }

    const WORKSPACES_LAYOUT: &str = r#"export default function render() {
      return <root>
        <I3Layout>
          <Panel id="sidebar" anchor="left" size={300}>
            <div class="side" />
          </Panel>
          <Workspaces>
            {(Contents) => <Contents style={{width: 1620, height: 1080}} />}
          </Workspaces>
        </I3Layout>
      </root>;
    }"#;

    #[test]
    #[tracing_test::traced_test]
    fn a_workspaces_that_is_not_last_warns() {
        let layout = WORKSPACES_LAYOUT.replace(
            r#"      </Workspaces>
        </I3Layout>"#,
            r#"      </Workspaces>
          <Panel id="topbar" anchor="top" size={20}>
            <div class="top" />
          </Panel>
        </I3Layout>"#,
        );
        eval_with_screen(&layout);
        assert!(
            logs_contain("<Workspaces> may only be used once"),
            "a <Workspaces> that isn't last is reported, not silently accepted"
        );
    }

    /// `warn` has no caller yet (see its doc comment) — this proves the JS-to-
    /// `tracing::warn!` crossing itself works, independent of whichever global ends up
    /// calling it first.
    #[test]
    #[tracing_test::traced_test]
    fn warn_reaches_a_tracing_warning() {
        eval_with_screen(
            r#"export default function render() {
              warn("a distinctive warning from JS");
              return <root />;
            }"#,
        );
        assert!(
            logs_contain("a distinctive warning from JS"),
            "warn() must actually reach a tracing::warn! log, not disappear"
        );
    }

    #[test]
    #[tracing_test::traced_test]
    fn a_repeated_workspaces_warns() {
        let layout = WORKSPACES_LAYOUT.replace(
            r#"      </Workspaces>
        </I3Layout>"#,
            r#"      </Workspaces>
          <Workspaces>
            {(Contents) => <Contents style={{width: 1620, height: 1080}} />}
          </Workspaces>
        </I3Layout>"#,
        );
        eval_with_screen(&layout);
        assert!(
            logs_contain("<Workspaces> may only be used once"),
            "a repeated <Workspaces> is reported, not silently accepted"
        );
    }

    #[test]
    #[tracing_test::traced_test]
    fn a_single_workspaces_last_does_not_warn() {
        eval_with_screen(WORKSPACES_LAYOUT);
        assert!(
            !logs_contain("<Workspaces> may only be used once"),
            "used correctly, there is nothing to warn about"
        );
    }

    /// Issue #525 bug #4: a sidebar wider than the screen (as a squished
    /// panel from a bad DPR reading, or just a misconfigured layout, could
    /// produce) drives `freeW` negative. Uncaught, that throws crossing the
    /// `u32` FFI boundary into Rust and drops the *entire* tick's panel/gaps
    /// update — not just the offending one. `eval_with_screen` unwraps the
    /// eval result, so a regression here fails loudly rather than silently.
    #[test]
    fn a_gap_wider_than_the_screen_does_not_drop_the_whole_tick() {
        let layout = WORKSPACES_LAYOUT.replace("size={300}", "size={2000}");
        eval_with_screen(&layout);
    }

    /// Issue #570: a layout file that throws at module load must surface the thrown
    /// message. rquickjs's `Error::Exception` is contentless ("Exception generated by
    /// QuickJS"); the text is only readable while the `Ctx` is alive, so it has to be
    /// captured at the closure boundary.
    #[test]
    fn a_layout_that_throws_while_loading_reports_the_thrown_message() {
        let err = JsxEvaluator::new(
            r#"throw new Error("boom while loading");
            export default function render() { return <root/>; }"#,
            serde_json::Value::Null,
            None,
        )
        .err()
        .expect("a module that throws while loading must fail to load");
        assert!(
            err.to_string().contains("boom while loading"),
            "Display must carry the thrown message, got: {err}"
        );
        assert!(
            format!("{err:?}").contains("boom while loading"),
            "Debug must carry the thrown message, got: {err:?}"
        );
    }

    /// Issue #570, the render path: a `render()` that throws must report what it threw.
    #[test]
    fn a_render_that_throws_reports_the_thrown_message() {
        let evaluator = JsxEvaluator::new(
            r#"export default function render() { throw new Error("boom while rendering"); }"#,
            serde_json::Value::Null,
            None,
        )
        .unwrap();
        let err = evaluator
            .eval(&std::collections::HashMap::new())
            .err()
            .expect("a render that throws must fail");
        assert!(
            err.to_string().contains("boom while rendering"),
            "Display must carry the thrown message, got: {err}"
        );
        assert!(
            format!("{err:?}").contains("boom while rendering"),
            "Debug must carry the thrown message, got: {err:?}"
        );
    }

    /// Issue #570, the reconciler path: `eval_units` crosses the same closure
    /// boundary and must report the thrown message too.
    #[test]
    fn a_reconciler_render_that_throws_reports_the_thrown_message() {
        let evaluator = JsxEvaluator::new_reconciler(
            r#"export default function render() { throw new Error("boom while rendering"); }"#,
            serde_json::Value::Null,
            None,
            Default::default(),
        )
        .unwrap();
        let err = evaluator
            .eval_units(&std::collections::HashMap::new())
            .expect_err("a reconciler render that throws must fail");
        assert!(
            err.to_string().contains("boom while rendering"),
            "Display must carry the thrown message, got: {err}"
        );
        assert!(
            format!("{err:?}").contains("boom while rendering"),
            "Debug must carry the thrown message, got: {err:?}"
        );
    }

    /// Issue #570 as a person actually hits it: a typo in a layout file is a
    /// ReferenceError, not an explicit throw, and the error must name the identifier.
    #[test]
    fn a_typo_in_a_layout_file_names_the_undefined_identifier() {
        let evaluator = JsxEvaluator::new(
            r#"export default function render() { return <root>{noSuchThing}</root>; }"#,
            serde_json::Value::Null,
            None,
        )
        .unwrap();
        let err = evaluator
            .eval(&std::collections::HashMap::new())
            .err()
            .expect("an undefined identifier must fail the render");
        assert!(
            err.to_string().contains("noSuchThing"),
            "the error must name the undefined identifier, got: {err}"
        );
    }

    /// Writes `source` as `<dir>/layout.op.mdx` and evaluates it with that file as
    /// the layout path, returning the rendered tree.
    fn eval_layout_file(dir: &Path, source: &str) -> serde_json::Value {
        let layout_path = dir.join("layout.op.mdx");
        std::fs::write(&layout_path, source).expect("write layout file");
        JsxEvaluator::new(source, serde_json::Value::Null, Some(&layout_path))
            .unwrap_or_else(|e| panic!("expected the layout to evaluate, got: {e}"))
            .eval(&std::collections::HashMap::new())
            .unwrap()
            .layout
    }

    /// A layout that renders an imported `meta` object's three fields as text children.
    const RENDER_IMPORTED_META: &str = r#"import { meta } from './Foo.jsx';
export default function render() { return <div><span>{String(meta.dirname)}</span><span>{String(meta.filename)}</span><span>{String(meta.url)}</span></div>; }"#;

    const EXPORT_META: &str = "export const meta = { dirname: import.meta.dirname, filename: import.meta.filename, url: import.meta.url };";

    fn text_child(tree: &serde_json::Value, i: usize) -> String {
        tree["children"][i]["children"][0]
            .as_str()
            .unwrap_or_else(|| panic!("no text child {i} in {tree:?}"))
            .to_string()
    }

    #[test]
    fn a_relative_import_sees_its_own_import_meta() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join("Foo.jsx"), EXPORT_META).expect("write Foo.jsx");

        let tree = eval_layout_file(tmp.path(), RENDER_IMPORTED_META);

        let dir = tmp.path().canonicalize().unwrap();
        let filename = dir.join("Foo.jsx").to_string_lossy().to_string();
        assert_eq!(text_child(&tree, 0), dir.to_string_lossy());
        assert_eq!(text_child(&tree, 1), filename);
        assert_eq!(text_child(&tree, 2), format!("file://{filename}"));
    }

    /// An import of an import reports its own directory, not its importer's.
    #[test]
    fn a_nested_import_sees_its_own_import_meta() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(tmp.path().join("a/b")).expect("create a/b");
        std::fs::write(tmp.path().join("a/b/B.jsx"), EXPORT_META).expect("write B.jsx");
        std::fs::write(
            tmp.path().join("a/A.jsx"),
            "export { meta } from './b/B.jsx';",
        )
        .expect("write A.jsx");

        let tree = eval_layout_file(
            tmp.path(),
            &RENDER_IMPORTED_META.replace("./Foo.jsx", "./a/A.jsx"),
        );

        let dir = tmp.path().canonicalize().unwrap().join("a/b");
        assert_eq!(text_child(&tree, 0), dir.to_string_lossy());
        assert_eq!(text_child(&tree, 1), dir.join("B.jsx").to_string_lossy());
    }

    #[test]
    fn the_layout_file_sees_its_own_import_meta() {
        let tmp = tempfile::tempdir().expect("tempdir");

        let tree = eval_layout_file(
            tmp.path(),
            r#"export default function render() { return <div><span>{String(import.meta.dirname)}</span><span>{String(import.meta.filename)}</span><span>{String(import.meta.url)}</span></div>; }"#,
        );

        let dir = tmp.path().canonicalize().unwrap();
        let filename = dir.join("layout.op.mdx").to_string_lossy().to_string();
        assert_eq!(text_child(&tree, 0), dir.to_string_lossy());
        assert_eq!(text_child(&tree, 1), filename);
        assert_eq!(text_child(&tree, 2), format!("file://{filename}"));
    }

    /// A layout reached through a symlinked directory (a common dotfiles setup) reports
    /// the same resolved directory its imports do, since the resolver resolves theirs.
    #[test]
    fn the_layout_file_and_its_imports_agree_through_a_symlink() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let real = tmp.path().join("real");
        std::fs::create_dir_all(&real).expect("create real dir");
        std::fs::write(real.join("Foo.jsx"), EXPORT_META).expect("write Foo.jsx");
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&real, &link).expect("symlink");

        let tree = eval_layout_file(
            &link,
            r#"import { meta } from './Foo.jsx';
export default function render() { return <div><span>{String(import.meta.dirname)}</span><span>{String(meta.dirname)}</span></div>; }"#,
        );

        assert_eq!(text_child(&tree, 0), text_child(&tree, 1));
    }

    #[test]
    fn import_meta_url_is_percent_encoded() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let spaced = tmp.path().join("with space");
        std::fs::create_dir_all(&spaced).expect("create spaced dir");
        std::fs::write(spaced.join("Foo.jsx"), EXPORT_META).expect("write Foo.jsx");

        let tree = eval_layout_file(&spaced, RENDER_IMPORTED_META);

        assert!(text_child(&tree, 0).ends_with("/with space"), "{tree:?}");
        let url = text_child(&tree, 2);
        assert!(url.starts_with("file:///"), "{url}");
        assert!(url.contains("/with%20space/Foo.jsx"), "{url}");
    }

    #[test]
    fn import_meta_is_undefined_without_a_layout_path() {
        let tree = JsxEvaluator::new(
            r#"export default function render() { return <div><span>{String(import.meta.dirname)}</span><span>{String(import.meta.filename)}</span><span>{String(import.meta.url)}</span></div>; }"#,
            serde_json::Value::Null,
            None,
        )
        .unwrap()
        .eval(&std::collections::HashMap::new())
        .unwrap()
        .layout;
        for i in 0..3 {
            assert_eq!(text_child(&tree, i), "undefined");
        }
    }

    /// Commits `files` (path, contents, executable) to a fresh local repo, warms the
    /// cache with that commit and writes a Lockfile pinning it as `@gh/fixture/repo`.
    /// Returns the layout directory, the Package context and the checkout path.
    fn warm_package(
        dir: &Path,
        files: &[(&str, &str, bool)],
    ) -> (PathBuf, crate::pkg::PackageContext, PathBuf) {
        let (remote, sha) = crate::pkg::git::fixture_repo(dir, files);

        let layout_dir = dir.join("layout");
        std::fs::create_dir_all(&layout_dir).expect("create layout dir");
        let lockfile_path = layout_dir.join("tauler-pkg.lock");
        let cache_root = dir.join("cache");
        let final_path = crate::pkg::cache::pinned_cache_path(&cache_root, "fixture", "repo", &sha);
        crate::pkg::git::fetch_and_place(&remote.to_string_lossy(), &sha, &cache_root, &final_path)
            .expect("pre-warming the cache should succeed");
        let mut lockfile = crate::pkg::lockfile::Lockfile::default();
        lockfile.set(
            "fixture",
            "repo",
            crate::pkg::lockfile::PackageEntry {
                commit: sha,
                development: false,
            },
        );
        lockfile
            .save_to_path(&lockfile_path)
            .expect("saving the lockfile should succeed");
        let pkg_ctx = package_context(lockfile_path, cache_root);
        (layout_dir, pkg_ctx, final_path)
    }

    /// `import.meta.dirname` inside a Package points into the pinned checkout, so a
    /// Package can name a program it ships (`${import.meta.dirname}/../bin/x`).
    #[test]
    fn a_package_module_sees_its_checkout_in_import_meta() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (layout_dir, pkg_ctx, final_path) = warm_package(
            tmp.path(),
            &[
                (
                    "data/Volume.jsx",
                    "export const volumeDir = import.meta.dirname;",
                    false,
                ),
                (
                    "index.jsx",
                    "export { volumeDir } from './data/Volume.jsx';\n\
                     export const indexDir = import.meta.dirname;",
                    false,
                ),
            ],
        );

        let layout_source = r#"import { indexDir, volumeDir } from "@gh/fixture/repo";
export default function render() { return <div><span>{String(indexDir)}</span><span>{String(volumeDir)}</span></div>; }"#;
        let layout_path = layout_dir.join("layout.op.mdx");
        std::fs::write(&layout_path, layout_source).expect("write layout file");

        let tree = JsxEvaluator::new_with_packages(
            layout_source,
            serde_json::Value::Null,
            Some(&layout_path),
            pkg_ctx,
            Default::default(),
        )
        .unwrap_or_else(|e| panic!("expected the warm Package to resolve, got: {e}"))
        .eval(&std::collections::HashMap::new())
        .unwrap()
        .layout;

        let checkout = final_path.canonicalize().unwrap();
        assert_eq!(text_child(&tree, 0), checkout.to_string_lossy());
        assert_eq!(
            text_child(&tree, 1),
            checkout.join("data").to_string_lossy()
        );
    }

    /// The point of the feature: a Package's component names the program the same
    /// Package ships, and that path spawns it straight from the pinned checkout.
    #[test]
    fn a_package_component_spawns_the_program_it_ships() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (layout_dir, pkg_ctx, _) = warm_package(
            tmp.path(),
            &[
                (
                    "bin/tauler-volume",
                    "#!/bin/sh\necho '{\"percent\":42}'\n",
                    true,
                ),
                (
                    "data/Volume.jsx",
                    "export function Volume() {\n\
                       const v = useJSONStream(`${import.meta.dirname}/../bin/tauler-volume`);\n\
                       return <span>{String(v?.percent)}</span>;\n\
                     }",
                    false,
                ),
                (
                    "index.jsx",
                    "export { Volume } from './data/Volume.jsx';",
                    false,
                ),
            ],
        );

        let layout_source = r#"import { Volume } from "@gh/fixture/repo";
export default function render() { return <Volume />; }"#;
        let layout_path = layout_dir.join("layout.op.mdx");
        std::fs::write(&layout_path, layout_source).expect("write layout file");

        let output = JsxEvaluator::new_with_packages(
            layout_source,
            serde_json::Value::Null,
            Some(&layout_path),
            pkg_ctx,
            Default::default(),
        )
        .unwrap_or_else(|e| panic!("expected the warm Package to resolve, got: {e}"))
        .eval(&std::collections::HashMap::new())
        .unwrap();

        let [(bin, None)] = output.stream_calls.as_slice() else {
            panic!("expected one stream call, got: {:?}", output.stream_calls);
        };
        let module = crate::data::spawn_module(bin, None);
        let line = module
            .rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("the Package's program should print a line");
        assert_eq!(line, r#"{"percent":42}"#);
    }

    use crate::d3_figure::test_support::{
        d3_only_pkg_ctx, fake_d3_assets, fake_d3_fetcher, write_fake_files,
    };

    const D3_LAYOUT: &str = r#"import { scaleLinear } from "d3";
export default function render() { return <span class="text-white">{String(scaleLinear(2))}</span>; }"#;

    #[test]
    fn a_layout_importing_a_name_from_d3_renders_with_the_fetched_d3() {
        let dir = tempfile::tempdir().expect("tempdir");
        let d3_dir = dir.path().join("d3");
        write_fake_files(&d3_dir);
        let pkg_ctx = d3_only_pkg_ctx(dir.path(), fake_d3_fetcher(&d3_dir));

        let result = JsxEvaluator::new_with_packages(
            D3_LAYOUT,
            serde_json::Value::Null,
            Some(dir.path()),
            pkg_ctx,
            Default::default(),
        )
        .unwrap_or_else(|e| panic!("expected \"d3\" to resolve, got: {e}"))
        .eval(&std::collections::HashMap::new())
        .unwrap()
        .layout;

        assert_eq!(result["children"][0], "4", "got: {result:?}");
    }

    #[test]
    fn importing_d3_before_its_files_are_fetched_fails_like_a_cold_package() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pkg_ctx = d3_only_pkg_ctx(dir.path(), fake_d3_fetcher(&dir.path().join("empty")));

        let result = JsxEvaluator::new_with_packages(
            D3_LAYOUT,
            serde_json::Value::Null,
            Some(dir.path()),
            pkg_ctx,
            Default::default(),
        );

        assert!(result.is_err(), "a cold d3 must fail resolution");
    }

    /// The "Discovery lives in the resolver" half: resolving a cold `"d3"` is what
    /// starts the Fetcher's download (whose completion wakes the reload).
    #[test]
    fn importing_d3_before_its_files_are_fetched_starts_the_d3_download() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (requested_tx, requested_rx) = std::sync::mpsc::channel::<String>();
        let requested_tx = Mutex::new(requested_tx);
        let (reload_tx, _reload_rx) = std::sync::mpsc::channel();
        let fetcher = crate::d3_figure::fetch::Fetcher::build(
            dir.path().join("d3"),
            fake_d3_assets(),
            Some((
                reload_tx,
                Arc::new(move |url: &str| {
                    let _ = requested_tx.lock().unwrap().send(url.to_string());
                    Err("offline in tests".to_string())
                }),
            )),
        );
        let pkg_ctx = d3_only_pkg_ctx(dir.path(), fetcher);

        let result = JsxEvaluator::new_with_packages(
            D3_LAYOUT,
            serde_json::Value::Null,
            Some(dir.path()),
            pkg_ctx,
            Default::default(),
        );

        assert!(result.is_err(), "a cold d3 must fail resolution");
        requested_rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("resolving a cold \"d3\" should start the d3 download");
    }

    #[test]
    fn importing_d3_without_a_package_context_fails_like_any_unknown_import() {
        let result = JsxEvaluator::new(D3_LAYOUT, serde_json::Value::Null, None);

        assert!(result.is_err());
    }

    // ---- The draw runtime (`JsxEvaluator::new_draw` / `draw_figure`) ----

    /// A limit no test draw comes near, except the one that loops forever.
    const GENEROUS_LIMIT: std::time::Duration = std::time::Duration::from_secs(5);

    /// A layout rendering one D3Figure with key `k`, 10×20, whose `draw` body is
    /// `draw` (an arrow function taking `(svg, d3)`).
    fn one_figure_with_draw(draw: &str) -> String {
        format!(
            r#"export default function render() {{
return <div><D3Figure key="k" width={{10}} height={{20}} draw={{{draw}}} /></div>;
}}"#
        )
    }

    /// A draw evaluator over `source` with the fake d3/linkedom files in place,
    /// already evaluated once so its figures are registered. The tempdir is
    /// returned so it outlives the evaluator.
    fn ready_draw_evaluator(source: &str) -> (JsxEvaluator, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("tempdir");
        let d3_dir = dir.path().join("d3");
        write_fake_files(&d3_dir);
        let pkg_ctx = d3_only_pkg_ctx(dir.path(), fake_d3_fetcher(&d3_dir));
        let evaluator = JsxEvaluator::new_draw(
            source,
            serde_json::Value::Null,
            Some(dir.path()),
            Some(pkg_ctx),
            Default::default(),
        )
        .unwrap_or_else(|e| panic!("the draw runtime should start, got: {e}"));
        evaluator
            .eval(&std::collections::HashMap::new())
            .unwrap_or_else(|e| panic!("the draw runtime should evaluate the layout, got: {e}"));
        (evaluator, dir)
    }

    #[test]
    fn the_render_and_draw_runtimes_name_the_same_figures_for_the_same_inputs() {
        let source = r#"export default function render() {
return <div><D3Figure key={"k/" + useStringStream("/bin/sh", "v")} width={10} height={20} draw={() => {}} /></div>;
}"#;
        let streams = HashMap::from([(
            ("/bin/sh".to_string(), Some("v".to_string())),
            "7".to_string(),
        )]);
        let render = JsxEvaluator::new(source, serde_json::Value::Null, None).unwrap();
        let out = render.eval(&streams).unwrap();
        let keys = |calls: &[FigureCall]| calls.iter().map(|c| c.key.clone()).collect::<Vec<_>>();
        assert_eq!(keys(&out.figure_calls), ["k/7@10x20"]);
        let draw = JsxEvaluator::new_draw(
            source,
            serde_json::Value::Null,
            None,
            None,
            Default::default(),
        )
        .unwrap();

        let first = draw.eval(&out.inputs.stream_values).unwrap();
        let second = draw.eval(&out.inputs.stream_values).unwrap();

        assert_eq!(first.figure_calls, out.figure_calls);
        assert_eq!(second.figure_calls, out.figure_calls);
    }

    #[test]
    fn a_stream_read_only_inside_draw_reaches_the_draw_runtime() {
        let source = r#"export default function render() {
return <div><D3Figure key={"k" + useStringStream("/bin/sh", "a")} width={10} height={20}
  draw={(svg) => svg.append("text").attr("data-b", useStringStream("/bin/sh", "b"))} /></div>;
}"#;
        let streams = HashMap::from([
            (
                ("/bin/sh".to_string(), Some("a".to_string())),
                "1".to_string(),
            ),
            (
                ("/bin/sh".to_string(), Some("b".to_string())),
                "B-VALUE".to_string(),
            ),
        ]);
        let out = JsxEvaluator::new(source, serde_json::Value::Null, None)
            .unwrap()
            .eval(&streams)
            .unwrap();
        let (draw, _dir) = ready_draw_evaluator(source);
        draw.eval(&out.inputs.stream_values).unwrap();

        let svg = draw
            .draw_figure("k1@10x20", 10.0, 20.0, GENEROUS_LIMIT)
            .expect("the figure should draw");

        assert!(svg.contains(r#"data-b="B-VALUE""#), "got: {svg}");
    }

    #[test]
    fn the_draw_runtime_draws_a_figures_svg_with_its_draw_function() {
        let (evaluator, _dir) = ready_draw_evaluator(&one_figure_with_draw(
            r#"(svg) => svg.append("circle").attr("r", 3)"#,
        ));

        let svg = evaluator
            .draw_figure("k@10x20", 10.0, 20.0, GENEROUS_LIMIT)
            .expect("the figure should draw");

        assert!(svg.contains("<svg"), "got: {svg}");
        assert!(svg.contains(r#"width="10""#), "got: {svg}");
        assert!(svg.contains(r#"height="20""#), "got: {svg}");
        assert!(svg.contains(r#"<circle r="3""#), "got: {svg}");
    }

    #[test]
    fn draw_receives_its_pictures_name() {
        let (evaluator, _dir) = ready_draw_evaluator(&one_figure_with_draw(
            r#"(svg, d3, { name }) => svg.attr("data-name", name)"#,
        ));

        let svg = evaluator
            .draw_figure("k@10x20", 10.0, 20.0, GENEROUS_LIMIT)
            .expect("the figure should draw");

        assert!(svg.contains(r#"data-name="k@10x20""#), "got: {svg}");
    }

    #[test]
    fn an_object_key_draws_under_the_name_the_render_runtime_gave_it() {
        let source = r#"export default function render() {
return <div><D3Figure key={{rows: [1, 2]}} width={10} height={20}
  draw={(svg) => svg.append("circle").attr("r", 3)} /></div>;
}"#;
        let name = eval(source).figure_calls[0].key.clone();
        let (evaluator, _dir) = ready_draw_evaluator(source);

        let svg = evaluator
            .draw_figure(&name, 10.0, 20.0, GENEROUS_LIMIT)
            .unwrap_or_else(|e| panic!("{name} should draw in the draw runtime, got: {e}"));

        assert!(svg.contains(r#"<circle r="3""#), "got: {svg}");
    }

    #[test]
    fn the_draw_runtime_draws_a_d3_figure_built_at_module_scope() {
        let (evaluator, _dir) = ready_draw_evaluator(MODULE_SCOPE_FIGURE);
        // A second eval too: the registration from module load must survive it.
        evaluator
            .eval(&std::collections::HashMap::new())
            .expect("the layout should evaluate again");

        let svg = evaluator
            .draw_figure("k@10x20", 10.0, 20.0, GENEROUS_LIMIT)
            .expect("a figure built at module scope is in the tree, so it should draw");

        assert!(svg.contains(r#"<circle r="3""#), "got: {svg}");
    }

    #[test]
    fn a_module_scope_figure_that_leaves_the_tree_and_comes_back_draws_again() {
        let (evaluator, _dir) = ready_draw_evaluator(
            r#"const chart = <D3Figure key="k" width={10} height={20}
  draw={(svg) => svg.append("circle").attr("r", 3)} />;
let n = 0;
export default function render() {
n += 1;
return <div>{n === 2 ? null : chart}</div>;
}"#,
        );
        let streams = std::collections::HashMap::new();
        evaluator.eval(&streams).expect("second eval: hidden");
        assert!(
            matches!(
                evaluator.draw_figure("k@10x20", 10.0, 20.0, GENEROUS_LIMIT),
                Err(DrawError::UnknownKey(_))
            ),
            "precondition: a hidden figure is not in the latest eval"
        );
        evaluator.eval(&streams).expect("third eval: shown again");

        let svg = evaluator
            .draw_figure("k@10x20", 10.0, 20.0, GENEROUS_LIMIT)
            .expect("the element was built once, at load, and is back in the tree");

        assert!(svg.contains(r#"<circle r="3""#), "got: {svg}");
    }

    #[test]
    fn a_key_the_latest_eval_no_longer_renders_is_unknown_key() {
        let (evaluator, _dir) = ready_draw_evaluator(
            r#"let n = 0;
export default function render() {
n += 1;
return <div><D3Figure key={"k" + n} width={10} height={20} draw={() => {}} /></div>;
}"#,
        );
        evaluator
            .eval(&std::collections::HashMap::new())
            .expect("the layout should evaluate again");

        let result = evaluator.draw_figure("k1@10x20", 10.0, 20.0, GENEROUS_LIMIT);

        assert!(
            matches!(&result, Err(DrawError::UnknownKey(_))),
            "k1 left the tree, so it is not drawn; got: {result:?}"
        );
        assert!(evaluator
            .draw_figure("k2@10x20", 10.0, 20.0, GENEROUS_LIMIT)
            .is_ok());
    }

    #[test]
    fn drawing_a_key_no_d3_figure_has_is_unknown_key() {
        let (evaluator, _dir) = ready_draw_evaluator(&one_figure_with_draw("() => {}"));

        let result = evaluator.draw_figure("no-such-key", 10.0, 20.0, GENEROUS_LIMIT);

        assert!(
            matches!(&result, Err(DrawError::UnknownKey(_))),
            "got: {result:?}"
        );
    }

    #[test]
    fn a_draw_function_that_throws_is_threw_with_its_message() {
        let (evaluator, _dir) = ready_draw_evaluator(&one_figure_with_draw(
            r#"() => { throw new Error("bad draw"); }"#,
        ));

        let result = evaluator.draw_figure("k@10x20", 10.0, 20.0, GENEROUS_LIMIT);

        match result {
            Err(DrawError::Threw(message)) => {
                assert!(message.contains("bad draw"), "got: {message}")
            }
            other => panic!("expected Threw, got: {other:?}"),
        }
    }

    #[test]
    fn a_draw_function_that_loops_forever_times_out_and_the_runtime_still_draws_after() {
        let (evaluator, _dir) = ready_draw_evaluator(
            r#"export default function render() {
return <div>
  <D3Figure key="loop" width={10} height={20} draw={() => { for (;;) {} }} />
  <D3Figure key="ok" width={10} height={20} draw={(svg) => svg.append("circle").attr("r", 3)} />
</div>;
}"#,
        );

        let started = std::time::Instant::now();
        let result = evaluator.draw_figure(
            "loop@10x20",
            10.0,
            20.0,
            std::time::Duration::from_millis(200),
        );
        let elapsed = started.elapsed();

        assert_eq!(result, Err(DrawError::TimedOut));
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "a 200 ms limit took {elapsed:?}"
        );
        let svg = evaluator
            .draw_figure("ok@10x20", 10.0, 20.0, GENEROUS_LIMIT)
            .expect("the runtime should keep drawing after an interrupted draw");
        assert!(svg.contains(r#"<circle r="3""#), "got: {svg}");
    }

    #[test]
    fn the_draw_runtime_has_timers_that_never_fire() {
        let (evaluator, _dir) = ready_draw_evaluator(&one_figure_with_draw(
            r#"(svg) => {
  setTimeout(() => { throw 1; }, 0);
  const interval = setInterval(() => { throw 2; }, 0);
  clearInterval(interval);
  const frame = requestAnimationFrame(() => { throw 3; });
  cancelAnimationFrame(frame);
  clearTimeout(0);
  svg.append("circle").attr("r", 3);
}"#,
        ));

        let svg = evaluator
            .draw_figure("k@10x20", 10.0, 20.0, GENEROUS_LIMIT)
            .expect("a draw that schedules timers should still draw");

        assert!(svg.contains(r#"<circle r="3""#), "got: {svg}");
    }

    /// SPEC: no `fetch`, filesystem or shell builtins in the draw runtime.
    #[test]
    fn the_draw_runtime_has_no_shell_filesystem_or_fetch() {
        let (evaluator, _dir) = ready_draw_evaluator(&one_figure_with_draw(
            r#"(svg) => svg.attr("data-effects", [typeof sh, typeof read, typeof ls, typeof fetch].join())"#,
        ));

        let svg = evaluator
            .draw_figure("k@10x20", 10.0, 20.0, GENEROUS_LIMIT)
            .expect("the figure should draw");

        assert!(
            svg.contains(r#"data-effects="undefined,undefined,undefined,undefined""#),
            "got: {svg}"
        );
    }

    #[test]
    fn drawing_before_d3_and_linkedom_are_downloaded_is_not_ready() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pkg_ctx = d3_only_pkg_ctx(dir.path(), fake_d3_fetcher(&dir.path().join("empty")));
        let evaluator = JsxEvaluator::new_draw(
            &one_figure_with_draw("() => {}"),
            serde_json::Value::Null,
            Some(dir.path()),
            Some(pkg_ctx),
            Default::default(),
        )
        .unwrap_or_else(|e| panic!("the draw runtime should start without d3, got: {e}"));
        evaluator
            .eval(&std::collections::HashMap::new())
            .expect("the layout should evaluate");

        let result = evaluator.draw_figure("k@10x20", 10.0, 20.0, GENEROUS_LIMIT);

        assert_eq!(result, Err(DrawError::NotReady));
    }

    /// The real files, through the real Fetcher: d3's own axis must come out of
    /// linkedom's serializer.
    #[test]
    #[ignore = "downloads d3 and linkedom"]
    fn the_draw_runtime_draws_a_real_d3_axis() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (reload_tx, _reload_rx) = std::sync::mpsc::channel();
        let fetcher = crate::d3_figure::fetch::Fetcher::new(dir.path().join("d3"), reload_tx);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while fetcher.sources().is_none() {
            assert!(
                std::time::Instant::now() < deadline,
                "d3 and linkedom did not download within 60 s"
            );
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        let pkg_ctx = d3_only_pkg_ctx(dir.path(), fetcher);
        let evaluator = JsxEvaluator::new_draw(
            &one_figure_with_draw(
                r#"(svg, d3) => svg.append("g").call(d3.axisBottom(d3.scaleLinear().domain([0, 1]).range([0, 100])))"#,
            ),
            serde_json::Value::Null,
            Some(dir.path()),
            Some(pkg_ctx),
            Default::default(),
        )
        .unwrap_or_else(|e| panic!("the draw runtime should start, got: {e}"));
        evaluator
            .eval(&std::collections::HashMap::new())
            .expect("the layout should evaluate");

        let svg = evaluator
            .draw_figure("k@10x20", 10.0, 20.0, GENEROUS_LIMIT)
            .expect("the axis should draw");

        assert!(svg.contains(r#"class="tick""#), "got: {svg}");
    }

    /// A layout file measuring `call` (a JS expression), in each of the three
    /// runtimes that evaluate it: render, reconciler, Draw.
    fn measured_in_every_runtime(call: &str) -> [serde_json::Value; 3] {
        crate::init_global_ctx(crate::config::FontConfig::default());
        let source = format!(
            "export default function render() {{ return <div>{{JSON.stringify({call})}}</div>; }}"
        );
        let empty = std::collections::HashMap::new();
        let text = |evaluator: JsxEvaluator| -> serde_json::Value {
            let out = evaluator.eval(&empty).unwrap().layout;
            serde_json::from_str(out["children"][0].as_str().unwrap()).unwrap()
        };
        [
            text(JsxEvaluator::new(&source, serde_json::Value::Null, None).unwrap()),
            text(
                JsxEvaluator::new_reconciler(
                    &source,
                    serde_json::Value::Null,
                    None,
                    Default::default(),
                )
                .unwrap(),
            ),
            text(
                JsxEvaluator::new_draw(
                    &source,
                    serde_json::Value::Null,
                    None,
                    None,
                    Default::default(),
                )
                .unwrap(),
            ),
        ]
    }

    /// A chart compiles in the render runtime or the Draw runtime depending on
    /// where it is built; it must lay out the same in both, so every runtime
    /// measures, outside the effects gate, and gets the same answer.
    #[test]
    fn measure_text_gives_one_answer_in_every_runtime() {
        let [render, reconciler, draw] = measured_in_every_runtime(
            r#"measureText("Hello", { family: "sans-serif", size: 11 })"#,
        );

        assert!(render["width"].as_f64().unwrap() > 0.0, "got: {render}");
        assert_eq!(render, reconciler);
        assert_eq!(render, draw);
    }

    /// What the layout file sees is what the bar's fonts measure, to 1/64 px.
    #[test]
    fn measure_text_is_the_width_the_bars_fonts_give() {
        let [render, ..] = measured_in_every_runtime(
            r#"measureText("Requests per second", { family: "sans-serif", size: 11 })"#,
        );
        let expected = crate::render::with_global_ctx(|ctx| {
            crate::render::measure::measure_text(
                ctx,
                "Requests per second",
                &crate::render::measure::TextFont {
                    family: "sans-serif".into(),
                    size: 11.0,
                    weight: 400.0,
                    style: "normal".into(),
                },
            )
            .unwrap()
        });

        let width = render["width"].as_f64().unwrap();
        assert!(
            (width - expected.width).abs() < 1.0 / 64.0,
            "{render} vs {expected:?}"
        );
        assert_eq!(
            width * 64.0,
            (width * 64.0).round(),
            "rounded to 1/64 px: {width}"
        );
        assert!(render["ascent"].as_f64().unwrap() > 0.0, "got: {render}");
        assert!(render["descent"].as_f64().unwrap() > 0.0, "got: {render}");
    }

    /// A family the host lacks is a configuration problem, not a bug in the
    /// layout: it measures as the fallback drawing uses, and the bar stays up.
    #[test]
    fn measure_text_with_an_unknown_family_measures_the_fallback() {
        let [render, ..] = measured_in_every_runtime(
            r#"measureText("Hello", { family: "No Such Family", size: 11 })"#,
        );
        assert!(render["width"].as_f64().unwrap() > 0.0, "got: {render}");
    }

    /// The programmer's mistakes throw a `TypeError` that names the function.
    #[rstest::rstest]
    #[case::empty_family(r#"measureText("Hello", { family: "", size: 11 })"#)]
    #[case::blank_family_list(r#"measureText("Hello", { family: " , ", size: 11 })"#)]
    #[case::no_family(r#"measureText("Hello", { size: 11 })"#)]
    #[case::no_options(r#"measureText("Hello")"#)]
    #[case::zero_size(r#"measureText("Hello", { family: "sans-serif", size: 0 })"#)]
    #[case::string_size(r#"measureText("Hello", { family: "sans-serif", size: "11" })"#)]
    #[case::infinite_size(r#"measureText("Hello", { family: "sans-serif", size: Infinity })"#)]
    #[case::number_text(r#"measureText(42, { family: "sans-serif", size: 11 })"#)]
    fn measure_text_rejects_a_programmers_mistake_with_a_type_error(#[case] call: &str) {
        let [render, ..] = measured_in_every_runtime(&format!(
            r#"(() => {{ try {{ {call}; return "no error"; }} catch (e) {{ return [e.name, e.message]; }} }})()"#
        ));
        assert_eq!(render[0], "TypeError", "got: {render}");
        assert!(
            render[1].as_str().unwrap().contains("measureText"),
            "got: {render}"
        );
    }

    /// A family that is not a CSS family list at all is a typo in the layout, not a
    /// font the host lacks: it throws rather than measure as nothing.
    #[test]
    fn measure_text_rejects_a_family_that_does_not_parse_naming_it() {
        let [render, reconciler, draw] = measured_in_every_runtime(
            r#"(() => { try { measureText("Hello", { family: "a;b", size: 11 }); return "no error"; } catch (e) { return [e.name, e.message]; } })()"#,
        );
        assert_eq!(render[0], "TypeError", "got: {render}");
        let message = render[1].as_str().unwrap();
        assert!(message.contains("measureText"), "got: {message}");
        assert!(message.contains(r#""a;b""#), "got: {message}");
        assert_eq!(render, reconciler);
        assert_eq!(render, draw);
    }

    /// One line, as a canvas measures: a line break or a tab counts as a space.
    #[test]
    fn measure_text_measures_a_line_break_as_a_space() {
        let [render, ..] = measured_in_every_runtime(
            r#"[measureText("a\nb\tc", { family: "sans-serif", size: 11 }), measureText("a b c", { family: "sans-serif", size: 11 })]"#,
        );
        assert_eq!(render[0], render[1]);
    }

    // ---- `store(...)` over `crate::state` (issue #271) ----
    //
    // The compile pass names each Store (`layout.jsx#<name>`: these layouts have no
    // path).

    /// One Store, one element carrying a handler per kind of write, and the Store's
    /// contents as the element's text so an eval shows what the render read.
    const STORE_LAYOUT: &str = r#"
import { store } from 'tauler'; const desk = store({ count: 0, items: [], note: { text: "a" } });
export default () => (
  <div
    on_click={() => { desk.count += 1; return { channel: "c", event: desk.count }; }}
    on_push={() => { desk.items.push("x"); }}
    on_same={() => { desk.count = desk.count; desk.items.length; }}
    on_throw={() => { desk.count = 99; desk.items.push("lost"); throw new Error("boom"); }}
    on_replace={() => { const old = desk.items; desk.items = ["new"]; old.push("stale"); }}
  >{JSON.stringify({ count: desk.count, items: desk.items })}</div>
);
"#;

    fn store_evaluator(source: &str) -> JsxEvaluator {
        JsxEvaluator::new(source, serde_json::Value::Null, None).unwrap()
    }

    fn eval_again(e: &JsxEvaluator) -> EvalOutput {
        e.eval(&std::collections::HashMap::new()).unwrap()
    }

    /// Evaluate, then call the handler the root element carries under `prop`.
    fn fire(e: &JsxEvaluator, prop: &str) -> HandlerOutcome {
        let out = eval_again(e);
        let id = out.layout[prop]["$handler"]
            .as_i64()
            .unwrap_or_else(|| panic!("no handler under {prop}: {}", out.layout));
        e.invoke_handler_outcome(id, &serde_json::Value::Null)
    }

    /// What the render read from the Store, parsed back out of the element's text.
    fn rendered_store(e: &JsxEvaluator) -> serde_json::Value {
        let layout = eval_again(e).layout.to_string();
        let start = layout.find(r#"{\"count\""#).expect("the store as text");
        let end = start + layout[start..].find("}").expect("its end") + 1;
        serde_json::from_str(&layout[start..end].replace(r#"\""#, "\"")).expect("JSON")
    }

    fn desk(e: &JsxEvaluator) -> serde_json::Value {
        serde_json::Value::Object(
            e.state_handle()
                .lock()
                .unwrap()
                .snapshot("layout.jsx#desk")
                .expect("declared"),
        )
    }

    #[test]
    fn store_a_handlers_top_level_write_survives_the_next_eval() {
        let e = store_evaluator(STORE_LAYOUT);
        let outcome = fire(&e, "on_click");
        assert!(outcome.state_changed);
        assert_eq!(
            outcome.intents,
            Some(serde_json::json!([{ "channel": "c", "event": 1 }]))
        );
        assert_eq!(desk(&e)["count"], 1);
        assert_eq!(rendered_store(&e)["count"], 1);
        fire(&e, "on_click");
        assert_eq!(rendered_store(&e)["count"], 2);
    }

    #[test]
    fn store_a_handlers_nested_push_survives_the_next_eval() {
        let e = store_evaluator(STORE_LAYOUT);
        assert!(fire(&e, "on_push").state_changed);
        assert_eq!(desk(&e)["items"], serde_json::json!(["x"]));
        assert_eq!(rendered_store(&e)["items"], serde_json::json!(["x"]));
        assert!(fire(&e, "on_push").state_changed);
        assert_eq!(rendered_store(&e)["items"], serde_json::json!(["x", "x"]));
    }

    #[test]
    fn store_a_handler_that_writes_what_is_there_reports_no_change() {
        let e = store_evaluator(STORE_LAYOUT);
        assert!(!fire(&e, "on_same").state_changed);
    }

    #[test]
    fn store_a_handler_that_throws_after_a_write_leaves_state_unchanged() {
        let e = store_evaluator(STORE_LAYOUT);
        fire(&e, "on_push");
        let before = desk(&e);
        let outcome = fire(&e, "on_throw");
        assert_eq!(outcome, HandlerOutcome::default());
        assert_eq!(desk(&e), before);
        // The runtime's own copy of `items` held the half-made push; it must not be
        // what the next render reads.
        assert_eq!(
            rendered_store(&e),
            serde_json::json!({ "count": 0, "items": ["x"] })
        );
    }

    /// A nested value that a top-level write replaced is no longer the state, so a
    /// later change to it in place is not written back over its replacement.
    #[test]
    fn store_a_replaced_nested_value_is_not_written_back() {
        let e = store_evaluator(STORE_LAYOUT);
        assert!(fire(&e, "on_replace").state_changed);
        assert_eq!(desk(&e)["items"], serde_json::json!(["new"]));
    }

    #[test]
    fn store_a_render_time_write_survives_the_next_eval() {
        let e = store_evaluator(
            r#"
import { store } from 'tauler'; const seen = store({ renders: 0, log: [] });
export default () => {
  seen.renders += 1;
  seen.log.push(seen.renders);
  return <div>{String(seen.renders)}</div>;
};
"#,
        );
        eval_again(&e);
        eval_again(&e);
        let out = eval_again(&e);
        assert!(out.layout.to_string().contains("\"3\""), "{}", out.layout);
        let state = e.state_handle();
        let state = state.lock().unwrap();
        assert_eq!(state.get("layout.jsx#seen", "renders").unwrap().0, 3);
        assert_eq!(
            state.get("layout.jsx#seen", "log").unwrap().0,
            &serde_json::json!([1, 2, 3])
        );
    }

    #[test]
    fn store_a_render_that_throws_leaves_state_unchanged() {
        let e = store_evaluator(
            r#"
import { store } from 'tauler'; const s = store({ n: 0, list: [] });
export default () => {
  s.n += 1;
  s.list.push(s.n);
  if (s.n === 2) throw new Error("second render fails");
  return <div />;
};
"#,
        );
        eval_again(&e);
        assert!(e.eval(&std::collections::HashMap::new()).is_err());
        let state = e.state_handle();
        let state = state.lock().unwrap();
        assert_eq!(state.get("layout.jsx#s", "n").unwrap().0, 1);
        assert_eq!(
            state.get("layout.jsx#s", "list").unwrap().0,
            &serde_json::json!([1])
        );
    }

    /// A read is answered from the runtime's own mirror while the store's clock stands,
    /// so a write from anywhere else has to move it.
    #[test]
    fn store_a_write_from_outside_the_runtime_is_read_by_the_next_eval() {
        let e = store_evaluator(
            r#"
import { store } from 'tauler'; const s = store({ n: 1, gone: true });
export default () => <div>{"n=" + s.n + " gone=" + s.gone + " keys=" + Object.keys(s)}</div>;
"#,
        );
        assert!(eval_again(&e)
            .layout
            .to_string()
            .contains("n=1 gone=true keys=gone,n"));
        {
            let state = e.state_handle();
            let mut state = state.lock().unwrap();
            state.set("layout.jsx#s", "n", serde_json::json!(2));
            state.remove("layout.jsx#s", "gone");
        }
        let out = eval_again(&e).layout.to_string();
        assert!(out.contains("n=2 gone=undefined keys=n"), "{out}");
    }

    /// `key in store` says whether the key exists — before and after the key was read,
    /// for a nested value and for `null`, and after a delete.
    #[test]
    fn store_in_operator_says_whether_a_key_exists() {
        let e = store_evaluator(
            r#"
import { store } from 'tauler'; const s = store({ n: 1, list: [1], nothing: null, gone: true });
export default () => {
  const before = ["n", "list", "nothing", "missing"].map((k) => k in s);
  s.n; s.list; s.nothing; s.missing;
  const after = ["n", "list", "nothing", "missing"].map((k) => k in s);
  delete s.gone;
  return <div>{before.join(",") + ";" + after.join(",") + ";" + ("gone" in s)}</div>;
};
"#,
        );
        let out = eval_again(&e).layout.to_string();
        assert!(
            out.contains("true,true,true,false;true,true,true,false;false"),
            "{out}"
        );
    }

    /// The same nested JS value is handed out again while its version stands, and a
    /// write from outside this runtime replaces it.
    #[test]
    fn store_a_nested_value_is_reused_until_its_version_moves() {
        let e = store_evaluator(
            r#"
import { store } from 'tauler'; const s = store({ list: [1], other: 0 });
let last;
export default () => {
  const same = last === s.list;
  last = s.list;
  return <div>{same ? "same" : "fresh"}{JSON.stringify(s.list)}{Object.keys(s).join(",")}</div>;
};
"#,
        );
        assert!(eval_again(&e).layout.to_string().contains("fresh"));
        assert!(eval_again(&e).layout.to_string().contains("same"));
        // Another key's write moves no version of `list`.
        e.state_handle()
            .lock()
            .unwrap()
            .set("layout.jsx#s", "other", serde_json::json!(1));
        assert!(eval_again(&e).layout.to_string().contains("same"));
        e.state_handle()
            .lock()
            .unwrap()
            .set("layout.jsx#s", "list", serde_json::json!([1, 2]));
        let out = eval_again(&e).layout.to_string();
        assert!(out.contains("fresh") && out.contains("[1,2]"), "{out}");
        assert!(out.contains("list,other"), "{out}");
    }

    // ---- Stores: the compile pass and the owner above the evaluator (issue #271) ----

    /// Writes `files` (relative path, contents) into a fresh directory and returns it
    /// with the path of `layout.jsx` inside it.
    fn store_fixture(files: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        for (name, contents) in files {
            let path = dir.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).expect("create dir");
            std::fs::write(path, contents).expect("write fixture");
        }
        let layout = dir.path().join("layout.jsx");
        (dir, layout)
    }

    fn load_fixture(files: &[(&str, &str)]) -> Result<JsxEvaluator, JsError> {
        let (_dir, layout) = store_fixture(files);
        let source = std::fs::read_to_string(&layout).expect("layout.jsx is in the fixture");
        JsxEvaluator::new(&source, serde_json::Value::Null, Some(&layout))
    }

    #[test]
    fn store_in_the_layout_file_is_named_by_file_and_variable() {
        let evaluator = load_fixture(&[(
            "layout.jsx",
            r#"import { store } from 'tauler'; const history = store({ entries: [] });
export const desk = store({ open: false });
export default function render() { return <div>{String(desk.open)}{history.entries.length}</div>; }"#,
        )])
        .expect("layout loads");
        assert_eq!(
            evaluator.state_identities(),
            ["layout.jsx#history", "layout.jsx#desk"]
        );
    }

    /// With no layout path there is no file name; the module is `layout.jsx`.
    #[test]
    fn store_in_a_layout_without_a_path_is_named_layout_jsx() {
        let evaluator = JsxEvaluator::new(
            "import { store } from 'tauler'; const a = store({ n: 1 });\nexport default function render() { return <div>{a.n}</div>; }",
            serde_json::Value::Null,
            None,
        )
        .expect("layout loads");
        assert_eq!(evaluator.state_identities(), ["layout.jsx#a"]);
    }

    #[test]
    fn store_in_an_import_is_named_by_its_path_from_the_layout() {
        let evaluator = load_fixture(&[
            (
                "components/Desk.jsx",
                "import { store } from 'tauler'; export const desk = store({ open: false });\nconst seen = store({ n: 0 });\nexport const n = () => seen.n;",
            ),
            (
                "layout.jsx",
                r#"import { desk } from "./components/Desk";
export default function render() { return <div>{String(desk.open)}</div>; }"#,
            ),
        ])
        .expect("layout loads");
        assert_eq!(
            evaluator.state_identities(),
            ["components/Desk.jsx#desk", "components/Desk.jsx#seen"]
        );
    }

    /// A re-export is not a second declaration: one identity, the defining file's, and
    /// both import paths reach the same object.
    #[test]
    fn store_re_exported_from_a_second_file_keeps_the_defining_files_identity() {
        let evaluator = load_fixture(&[
            ("a.jsx", "import { store } from 'tauler'; export const desk = store({ open: false });"),
            ("b.jsx", "export { desk } from './a';\nexport { desk as renamed } from './a';"),
            (
                "layout.jsx",
                r#"import { desk as direct } from "./a";
import { desk, renamed } from "./b";
export default function render() { return <div>{String(direct === desk && desk === renamed)}</div>; }"#,
            ),
        ])
        .expect("layout loads");
        assert_eq!(evaluator.state_identities(), ["a.jsx#desk"]);
        let layout = evaluator.eval(&HashMap::new()).unwrap().layout;
        assert!(layout.to_string().contains("true"), "got: {layout}");
    }

    #[test]
    fn store_that_is_not_the_whole_value_of_a_const_fails_the_load_with_file_and_line() {
        let in_import = load_fixture(&[
            (
                "components/Bad.jsx",
                "import { store } from 'tauler'; export function Bad() {\n  return <div>{store({ n: 0 }).n}</div>;\n}",
            ),
            (
                "layout.jsx",
                "import { Bad } from './components/Bad';\nexport default function render() { return <Bad />; }",
            ),
        ])
        .err()
        .expect("the load must fail")
        .to_string();
        assert!(
            in_import
                .contains("components/Bad.jsx:2: store() must be the whole value of a `const`"),
            "got: {in_import}"
        );

        let in_layout = load_fixture(&[(
            "layout.jsx",
            "import { store } from 'tauler'; export default function render() {\n\n  let s = store({});\n  return <div />;\n}",
        )])
        .err()
        .expect("the load must fail")
        .to_string();
        assert!(
            in_layout.contains("layout.jsx:3: store()"),
            "got: {in_layout}"
        );
    }

    /// `.ts` and `.tsx` imports: an `as` cast inside the default, one around the whole
    /// call, a type argument and a type annotation all leave the call nameable.
    #[test]
    fn store_in_typescript_files_is_named() {
        let evaluator = load_fixture(&[
            (
                "state.ts",
                "type Desk = { open: boolean; tags: string[] };\n\
                 import { store } from 'tauler'; export const desk = store({ open: false, tags: [] as string[] });\n\
                 export const cast = store({ n: 0 }) as { n: number };\n\
                 export const typed = store<Desk>({ open: true, tags: [] } as Desk);\n\
                 export const annotated: Desk = store({ open: true, tags: [] });",
            ),
            (
                "Panel.tsx",
                "import { desk } from './state';\n\
                 import { store } from 'tauler'; const panel = store({ width: 0 as number });\n\
                 export const Panel = (props: { label: string }) => <div>{props.label}{String(desk.open)}{panel.width}</div>;",
            ),
            (
                "layout.jsx",
                "import { Panel } from './Panel';\nexport default function render() { return <Panel label=\"x\" />; }",
            ),
        ])
        .expect("layout loads");
        assert_eq!(
            evaluator.state_identities(),
            [
                "state.ts#desk",
                "state.ts#cast",
                "state.ts#typed",
                "state.ts#annotated",
                "Panel.tsx#panel"
            ]
        );
        evaluator.eval(&HashMap::new()).expect("renders");
    }

    /// What happens to a file with its own `store`: nothing. Its calls resolve to its
    /// own function, so the pass neither names them nor refuses the one inside a
    /// function, and tauler's `store` is never called — no identity is declared.
    #[test]
    fn a_file_with_its_own_store_function_is_left_alone() {
        let evaluator = load_fixture(&[
            (
                "own.jsx",
                "function store(value) { return { mine: value }; }\n\
                 export const top = store(1);\n\
                 export function nested() { return store(2); }",
            ),
            (
                "imported.jsx",
                "import { store } from './helper';\nexport const viaImport = store(3);",
            ),
            ("helper.jsx", "export const store = (value) => ({ helper: value });"),
            (
                "layout.jsx",
                r#"import { top, nested } from "./own";
import { viaImport } from "./imported";
export default function render() { return <div>{JSON.stringify([top, nested(), viaImport])}</div>; }"#,
            ),
        ])
        .expect("a file's own `store` must not fail the load");
        assert!(evaluator.state_identities().is_empty());
        let layout = evaluator.eval(&HashMap::new()).unwrap().layout.to_string();
        assert!(
            layout.contains(r#"[{\"mine\":1},{\"mine\":2},{\"helper\":3}]"#),
            "got: {layout}"
        );
    }

    /// The other half of the above: tauler's `store` reached through an alias is a call
    /// the pass cannot see, and the runtime refuses it when it runs.
    #[test]
    fn store_through_an_alias_throws_at_load() {
        let err = load_fixture(&[(
            "layout.jsx",
            "import { store } from 'tauler';\nconst make = store;\nconst a = make({});\nexport default function render() { return <div />; }",
        )])
        .err()
        .expect("the load must fail")
        .to_string();
        assert!(
            err.contains("store() must be the whole value of a const"),
            "got: {err}"
        );
    }

    /// No layout file can choose an identity. Whatever way `store` is reached and
    /// whatever is passed after the default, the host asks for the token the compile
    /// pass was given, and a layout file cannot read it.
    #[test]
    fn store_called_with_a_hand_written_identity_is_refused() {
        for body in [
            // Through an alias, which the pass does not follow.
            "import { store } from 'tauler';\nconst make = store;\nconst a = make({}, 'mine#a');",
            // With a guess at the third argument.
            "import { store } from 'tauler';\nconst make = store;\nconst a = make({}, 'mine#a', 'token');",
            // Through the namespace, which is not the binding the pass looks for.
            "import * as t from 'tauler';\nconst a = t.store({}, 'mine#a');",
            // The function behind the module, by its name on `globalThis`.
            "const a = globalThis.__tauler_store({}, 'mine#a');",
            // The identity of a Store that exists, to reach it without importing it.
            "import { store } from 'tauler';\nconst real = store({ n: 1 });\nconst make = store;\nconst a = make({}, 'layout.jsx#real');",
        ] {
            let state = SharedState::default();
            let err = JsxEvaluator::new_with_state(
                &format!("{body}\nexport default () => <div />;"),
                serde_json::Value::Null,
                None,
                Arc::clone(&state),
            )
            .err()
            .unwrap_or_else(|| panic!("the load must fail: {body}"))
            .to_string();
            assert!(
                err.contains("store() takes exactly one argument"),
                "{body}\ngot: {err}"
            );
            assert_eq!(state.lock().unwrap().snapshot("mine#a"), None, "{body}");
        }
    }

    /// `globals` is gone (ADR 0051): a layout file that still names it does not load,
    /// and the error says what replaced it. A file's own `globals` is nobody's business.
    #[test]
    fn a_layout_that_names_globals_fails_to_load_and_points_at_store() {
        let (_dir, layout) = store_fixture(&[
            (
                "components/Old.jsx",
                "export const Old = () => {\n  globals.n = 1;\n  return <div />;\n};",
            ),
            (
                "layout.jsx",
                "import { Old } from './components/Old';\nexport default () => <Old />;",
            ),
        ]);
        let source = std::fs::read_to_string(&layout).unwrap();
        for (source, path, at) in [
            (
                "export default () => <div>{globals.count ?? 0}</div>;",
                None,
                "layout.jsx:1: ",
            ),
            (
                source.as_str(),
                Some(layout.as_path()),
                "components/Old.jsx:2: ",
            ),
        ] {
            let err = JsxEvaluator::new(source, serde_json::Value::Null, path)
                .err()
                .expect("the load must fail")
                .to_string();
            assert!(
                err.contains(at) && err.contains("`globals` was removed") && err.contains("store"),
                "got: {err}"
            );
        }

        JsxEvaluator::new(
            "const globals = { n: 1 };\nexport default () => <div>{globals.n}</div>;",
            serde_json::Value::Null,
            None,
        )
        .expect("a file's own `globals` loads");
    }

    /// `store` is not a global: only the import reaches it.
    #[test]
    fn store_is_not_on_global_this() {
        let e = store_evaluator(
            "export default () => <div>{typeof globalThis.store + ',' + typeof store}</div>;",
        );
        let layout = eval_again(&e).layout.to_string();
        assert!(layout.contains("undefined,undefined"), "{layout}");
    }

    #[test]
    fn store_called_without_the_import_fails_the_load_and_says_what_to_import() {
        let err = JsxEvaluator::new(
            "const desk = store({ open: false });\nexport default () => <div />;",
            serde_json::Value::Null,
            None,
        )
        .err()
        .expect("the load must fail")
        .to_string();
        assert!(
            err.contains("layout.jsx:1: store is not a global")
                && err.contains(r#"import { store } from "tauler""#),
            "got: {err}"
        );
    }

    #[test]
    fn store_imported_under_another_name_is_named() {
        let e = store_evaluator(
            "import { store as s } from 'tauler';\nconst desk = s({ open: true });\nexport default () => <div>{String(desk.open)}</div>;",
        );
        assert_eq!(e.state_identities(), ["layout.jsx#desk"]);
        assert!(eval_again(&e).layout.to_string().contains("true"));
    }

    /// The import resolves in the reconciler and draw runtimes too, with a filesystem
    /// (a layout path) and without one.
    #[test]
    fn store_import_resolves_in_every_runtime() {
        let source = "import { store } from 'tauler';\nconst desk = store({ open: true });\nexport default () => <div>{String(desk.open)}</div>;";
        let (_dir, layout) = store_fixture(&[("layout.jsx", source)]);
        for path in [None, Some(layout.as_path())] {
            let state = SharedState::default();
            JsxEvaluator::new_with_state(source, serde_json::Value::Null, path, Arc::clone(&state))
                .expect("render runtime");
            JsxEvaluator::new_reconciler(source, serde_json::Value::Null, path, Arc::clone(&state))
                .expect("reconciler runtime");
            JsxEvaluator::new_draw(source, serde_json::Value::Null, path, None, state)
                .expect("draw runtime");
        }
    }

    /// A Package's Store is named by its specifier and its path in the Package, with no
    /// commit in it, so it cannot collide with the importer's `layout.op.mdx#desk`.
    #[test]
    fn store_in_a_package_is_named_by_its_specifier_without_the_ref() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (layout_dir, pkg_ctx, _) = warm_package(
            tmp.path(),
            &[
                ("state/desk.jsx", "import { store } from 'tauler'; export const desk = store({ open: false });", false),
                (
                    "index.jsx",
                    "import { store } from 'tauler'; export { desk } from './state/desk.jsx';\nconst hidden = store({ n: 0 });\nexport const n = () => hidden.n;",
                    false,
                ),
            ],
        );
        let layout_source = r#"import { desk as theirs } from "@gh/fixture/repo";
import { store } from 'tauler'; const desk = store({ open: true });
export default function render() { return <div>{String(theirs.open)}{String(desk.open)}</div>; }"#;
        let layout_path = layout_dir.join("layout.op.mdx");
        std::fs::write(&layout_path, layout_source).expect("write layout file");
        let evaluator = JsxEvaluator::new_with_packages(
            layout_source,
            serde_json::Value::Null,
            Some(&layout_path),
            pkg_ctx,
            Default::default(),
        )
        .expect("layout loads");
        assert_eq!(
            evaluator.state_identities(),
            [
                "@gh/fixture/repo/state/desk.jsx#desk",
                "@gh/fixture/repo/index.jsx#hidden",
                "layout.op.mdx#desk"
            ]
        );
    }

    /// The central claim of the reload risk: two evaluators built from the same source
    /// around one owner are one state. The second is what a reload builds.
    #[test]
    fn a_store_keeps_its_value_across_a_rebuilt_evaluator() {
        let source = r#"import { store } from 'tauler'; const counter = store({ n: 0 });
export default function render() { counter.n += 1; return <div>{"n=" + counter.n}</div>; }"#;
        let state = SharedState::default();

        let first =
            JsxEvaluator::new_with_state(source, serde_json::Value::Null, None, Arc::clone(&state))
                .unwrap();
        first.eval(&HashMap::new()).unwrap();
        let layout = first.eval(&HashMap::new()).unwrap().layout.to_string();
        assert!(layout.contains("n=2"), "got: {layout}");
        drop(first);
        assert_eq!(
            state
                .lock()
                .unwrap()
                .get("layout.jsx#counter", "n")
                .unwrap()
                .0,
            2
        );

        let second =
            JsxEvaluator::new_with_state(source, serde_json::Value::Null, None, state).unwrap();
        let layout = second.eval(&HashMap::new()).unwrap().layout.to_string();
        assert!(
            layout.contains("n=3"),
            "the rebuilt evaluator starts from 2; got: {layout}"
        );

        // Without a shared owner the same rebuild starts over.
        let fresh = JsxEvaluator::new(source, serde_json::Value::Null, None).unwrap();
        let layout = fresh.eval(&HashMap::new()).unwrap().layout.to_string();
        assert!(layout.contains("n=1"), "got: {layout}");
    }

    /// Not an assertion: prints what the pass adds to one file's load, for the DECISION
    /// at the layout's `transform_source` call. Run with `--nocapture`.
    #[test]
    fn store_pass_cost() {
        let component = "export function Row({ label, value }) {\n  return <div class=\"flex gap-2\"><span>{label}</span><span>{value}</span></div>;\n}\n";
        let plain: String = (0..50)
            .map(|i| component.replace("Row", &format!("Row{i}")))
            .collect();
        let with_store = format!("import {{ store }} from 'tauler'; export const desk = store({{ open: false }});\n{plain}");
        let median = |f: &dyn Fn()| {
            let mut samples: Vec<std::time::Duration> = (0..200)
                .map(|_| {
                    let t = std::time::Instant::now();
                    f();
                    t.elapsed()
                })
                .collect();
            samples.sort();
            samples[samples.len() / 2]
        };
        let skip = median(&|| {
            tauler_core::store_pass::rewrite(&plain, "a.jsx", "a.jsx", "t").unwrap();
        });
        let pass = median(&|| {
            tauler_core::store_pass::rewrite(&with_store, "a.jsx", "a.jsx", "t").unwrap();
        });
        let transform = median(&|| {
            optative_script::jsx::transform_source(&with_store, "a.jsx");
        });
        eprintln!(
            "store_pass_cost: {} bytes; no Store in file: {skip:?}; with a Store: {pass:?}; transform_source alone: {transform:?}",
            with_store.len()
        );
    }

    /// Every string leaf of a layout tree, in document order.
    fn layout_texts(node: &serde_json::Value, out: &mut Vec<String>) {
        match node {
            serde_json::Value::String(text) => out.push(text.clone()),
            serde_json::Value::Array(items) => {
                items.iter().for_each(|i| layout_texts(i, out));
            }
            serde_json::Value::Object(map) => {
                if let Some(children) = map.get("children") {
                    layout_texts(children, out);
                }
            }
            _ => {}
        }
    }

    // ---- The compile pass, the Proxy and the owner together (issue #271) ----

    /// A Store the compile pass names in an import, written by a handler into the
    /// `StateStore`, and found again by a second evaluator built on the same owner from
    /// the same source, which is what a reload builds.
    #[test]
    fn store_written_by_a_handler_survives_a_rebuilt_evaluator() {
        let (_dir, layout) = store_fixture(&[
            (
                "state/desk.jsx",
                "import { store } from 'tauler'; export const desk = store({ open: false });",
            ),
            (
                "layout.jsx",
                r#"import { desk } from "./state/desk";
export default function render() {
    return <div on_click={() => { desk.open = !desk.open; }}>
        <span>{`open=${desk.open}`}</span>
    </div>;
}"#,
            ),
        ]);
        let source = std::fs::read_to_string(&layout).expect("layout.jsx is in the fixture");
        let build = |state: SharedState| {
            JsxEvaluator::new_with_state(&source, serde_json::Value::Null, Some(&layout), state)
                .unwrap_or_else(|e| panic!("expected the layout to load, got: {e}"))
        };
        let texts = |evaluator: &JsxEvaluator| {
            let mut texts = Vec::new();
            layout_texts(&eval_again(evaluator).layout, &mut texts);
            texts
        };
        let state = SharedState::default();

        let first = build(Arc::clone(&state));
        // No identity was written by hand: the pass named the Store after its file.
        assert_eq!(first.state_identities(), ["state/desk.jsx#desk"]);
        let before = eval_again(&first).layout;
        let mut shown = Vec::new();
        layout_texts(&before, &mut shown);
        assert_eq!(shown, ["open=false"]);

        let handler = |node: &serde_json::Value| {
            node["on_click"]["$handler"]
                .as_i64()
                .unwrap_or_else(|| panic!("no handler on {node}"))
        };
        let store_write = first.invoke_handler_outcome(handler(&before), &serde_json::Value::Null);
        assert!(store_write.state_changed, "the Store's handler");
        assert_eq!(
            state
                .lock()
                .unwrap()
                .get("state/desk.jsx#desk", "open")
                .unwrap()
                .0,
            true
        );
        assert_eq!(texts(&first), ["open=true"]);
        drop(first);

        let second = build(state);
        assert_eq!(
            texts(&second),
            ["open=true"],
            "the rebuilt evaluator starts from what the first one left"
        );
    }

    // ---- The reconciler and draw runtimes read the app's store (issue #271, P3) ----

    /// A render runtime whose only handler flips `desk.open`, on `state`.
    const DESK_RENDER: &str = r#"
import { store } from 'tauler'; const desk = store({ open: false, items: [] });
export default () => <div on_click={() => { desk.open = !desk.open; }}>{`open=${desk.open}`}</div>;
"#;

    /// Build [`DESK_RENDER`] on `state` and run its handler once: `desk.open` is true.
    fn desk_opened_by_a_handler(state: &SharedState) -> JsxEvaluator {
        let render = JsxEvaluator::new_with_state(
            DESK_RENDER,
            serde_json::Value::Null,
            None,
            Arc::clone(state),
        )
        .unwrap();
        let shown = eval_again(&render).layout;
        let id = shown["on_click"]["$handler"].as_i64().expect("a handler");
        let outcome = render.invoke_handler_outcome(id, &serde_json::Value::Null);
        assert!(outcome.state_changed, "precondition: the handler wrote");
        render
    }

    fn desk_snapshot(
        state: &SharedState,
        render: &JsxEvaluator,
    ) -> serde_json::Map<String, serde_json::Value> {
        let identities = render.state_identities();
        assert_eq!(identities.len(), 1, "one Store: {identities:?}");
        state
            .lock()
            .unwrap()
            .snapshot(&identities[0])
            .expect("the Store is declared")
    }

    fn reconciler_on(state: &SharedState, source: &str) -> JsxEvaluator {
        JsxEvaluator::new_reconciler(source, serde_json::Value::Null, None, Arc::clone(state))
            .unwrap_or_else(|e| panic!("expected the reconciler runtime to load, got: {e}"))
    }

    const DESK_UNIT: &str = r#"
const Light = unit({
  key: (i) => i.entity,
  value: (i) => i.state,
  reconciler: optativeSet({ observe: () => [] }),
});
"#;

    #[test]
    fn store_written_by_a_handler_is_read_by_the_reconciler_runtime() {
        let state = SharedState::default();
        let source = format!(
            r#"import {{ store }} from 'tauler'; const desk = store({{ open: false, items: [] }});
{DESK_UNIT}
export default () => <root>
  <Light entity="light.desk" state={{desk.open ? "on" : "off"}} />
  <div on_click={{() => {{ desk.open = !desk.open; }}}} />
</root>;"#
        );
        let render = JsxEvaluator::new_with_state(
            &source,
            serde_json::Value::Null,
            None,
            Arc::clone(&state),
        )
        .unwrap();
        let reconciler = reconciler_on(&state, &source);
        let declared = |evaluator: &JsxEvaluator| {
            let batches = evaluator.eval_units(&HashMap::new()).unwrap();
            batches[0].items[0]["state"].clone()
        };
        assert_eq!(declared(&reconciler), "off");

        let shown = eval_again(&render).layout;
        let id = shown["children"][0]["on_click"]["$handler"]
            .as_i64()
            .unwrap_or_else(|| panic!("no handler in {shown}"));
        render.invoke_handler_outcome(id, &serde_json::Value::Null);

        assert_eq!(
            declared(&reconciler),
            "on",
            "the next Unit evaluation reads what the handler wrote"
        );
    }

    /// Every kind of write a render can make, and the Store as JSON after it, starting
    /// from what [`desk_opened_by_a_handler`] leaves: `{"items":[],"open":true}`.
    const RENDER_TIME_WRITES: [(&str, &str); 6] = [
        ("desk.open = false;", r#"{"items":[],"open":false}"#),
        ("delete desk.open;", r#"{"items":[]}"#),
        ("desk.items.push(1);", r#"{"items":[1],"open":true}"#),
        ("desk.added = 1;", r#"{"added":1,"items":[],"open":true}"#),
        (
            "desk.items = [2]; desk.items.push(3);",
            r#"{"items":[2,3],"open":true}"#,
        ),
        ("desk.open = 1; desk.open = 2;", r#"{"items":[],"open":2}"#),
    ];

    /// A layout whose render reads `desk`, makes `write`, reads it again, and shows
    /// both readings: as a Unit's declared state and as text.
    fn render_that_writes(write: &str) -> String {
        format!(
            "import {{ store }} from 'tauler'; const desk = store({{ open: false, items: [] }});\n\
             {DESK_UNIT}\n\
             export default () => {{\n\
               const before = JSON.stringify(desk);\n\
               {write}\n\
               const shown = before + ' -> ' + JSON.stringify(desk);\n\
               return <root><Light entity=\"light.desk\" state={{shown}} /><div>{{shown}}</div></root>;\n\
             }};"
        )
    }

    /// P2 in a runtime that only reads: the render's own write is what the rest of that
    /// render reads, the store never gets it, and the next evaluation starts from the
    /// store again.
    #[test]
    fn store_render_time_write_in_the_reconciler_runtime_is_kept_for_that_evaluation_only() {
        let state = SharedState::default();
        let render = desk_opened_by_a_handler(&state);
        let saved = desk_snapshot(&state, &render);

        for (write, after) in RENDER_TIME_WRITES {
            let reconciler = reconciler_on(&state, &render_that_writes(write));
            for evaluation in 1..=2 {
                let batches = reconciler
                    .eval_units(&HashMap::new())
                    .unwrap_or_else(|e| panic!("`{write}` threw: {e}"));
                assert_eq!(
                    batches[0].items[0]["state"],
                    format!(r#"{{"items":[],"open":true}} -> {after}"#),
                    "`{write}`, evaluation {evaluation}"
                );
                assert_eq!(desk_snapshot(&state, &render), saved, "after `{write}`");
            }
        }
    }

    #[test]
    fn store_render_time_write_in_the_draw_runtime_is_kept_for_that_evaluation_only() {
        let state = SharedState::default();
        let render = desk_opened_by_a_handler(&state);
        let saved = desk_snapshot(&state, &render);

        for (write, after) in RENDER_TIME_WRITES {
            let draw = JsxEvaluator::new_draw(
                &render_that_writes(write),
                serde_json::Value::Null,
                None,
                None,
                Arc::clone(&state),
            )
            .unwrap();
            for evaluation in 1..=2 {
                let shown = draw
                    .eval(&HashMap::new())
                    .unwrap_or_else(|e| panic!("`{write}` threw: {e}"))
                    .layout;
                let mut texts = Vec::new();
                layout_texts(&shown, &mut texts);
                assert_eq!(
                    texts,
                    [format!(r#"{{"items":[],"open":true}} -> {after}"#)],
                    "`{write}`, evaluation {evaluation}"
                );
                assert_eq!(desk_snapshot(&state, &render), saved, "after `{write}`");
            }
        }
    }

    /// A render that throws after a write leaves nothing behind in a runtime that only
    /// reads either.
    #[test]
    fn store_render_time_write_in_the_reconciler_runtime_is_dropped_when_the_render_throws() {
        let state = SharedState::default();
        let render = desk_opened_by_a_handler(&state);
        let source = format!(
            "import {{ store }} from 'tauler'; const desk = store({{ open: false, items: [] }});\n\
             {DESK_UNIT}\n\
             let renders = 0;\n\
             export default () => {{\n\
               const before = JSON.stringify(desk);\n\
               if (renders++ === 0) {{ desk.open = false; desk.items.push(1); throw new Error('boom'); }}\n\
               return <root><Light entity=\"light.desk\" state={{before}} /></root>;\n\
             }};"
        );
        let reconciler = reconciler_on(&state, &source);
        reconciler
            .eval_units(&HashMap::new())
            .expect_err("the first render throws");
        let batches = reconciler.eval_units(&HashMap::new()).unwrap();
        assert_eq!(batches[0].items[0]["state"], r#"{"items":[],"open":true}"#);
        drop(render);
    }

    /// Module scope is the layout's own evaluation too: a write there does not stop
    /// the reconciler runtime from loading, and does not reach the store.
    #[test]
    fn store_module_scope_write_in_the_reconciler_runtime_loads_and_stores_nothing() {
        let state = SharedState::default();
        let reconciler = reconciler_on(
            &state,
            "import { store } from 'tauler'; const desk = store({ n: 0 });\ndesk.n = 5;\n\
             export default () => <root />;",
        );
        reconciler.eval_units(&HashMap::new()).unwrap();
        let identities = reconciler.state_identities();
        assert_eq!(state.lock().unwrap().snapshot(&identities[0]), None);
    }

    #[test]
    fn store_write_from_a_unit_hook_changes_nothing() {
        let state = SharedState::default();
        let render = desk_opened_by_a_handler(&state);
        let saved = desk_snapshot(&state, &render);
        let source = r#"import { store } from 'tauler'; const desk = store({ open: false, items: [] });
const Light = unit({
  key: (i) => i.entity,
  value: (i) => i.state,
  reconciler: optativeSet({ observe: () => [] }),
  enter: (items) => { desk.open = false; },
});
export default () => <root><Light entity="light.desk" state="on" /></root>;"#;
        let reconciler = reconciler_on(&state, source);
        let batches = reconciler.eval_units(&HashMap::new()).unwrap();
        let items = serde_json::Value::Array(batches[0].items.clone());

        let ran = reconciler.dispatch_unit_hook(batches[0].unit_index, "enter", "enterOne", &items);

        assert_eq!(ran, 0, "the hook threw");
        assert_eq!(desk_snapshot(&state, &render), saved);
    }

    #[test]
    fn store_declared_in_the_reconciler_runtime_stores_no_default() {
        // Nothing declared it yet: the reconciler's declaration leaves the store empty.
        let empty = SharedState::default();
        let other_default = "import { store } from 'tauler'; const desk = store({ open: false, items: [], extra: 1 });\n\
                             export default () => <root />;";
        let reconciler = reconciler_on(&empty, other_default);
        reconciler.eval_units(&HashMap::new()).unwrap();
        let identities = reconciler.state_identities();
        assert_eq!(empty.lock().unwrap().snapshot(&identities[0]), None);

        // Declared by the render runtime: a default with a new key fills nothing.
        let state = SharedState::default();
        let render = desk_opened_by_a_handler(&state);
        let saved = desk_snapshot(&state, &render);
        reconciler_on(&state, other_default)
            .eval_units(&HashMap::new())
            .unwrap();
        assert_eq!(desk_snapshot(&state, &render), saved);
    }

    #[test]
    fn store_written_by_a_handler_is_read_by_the_draw_runtime_which_cannot_write_it() {
        let state = SharedState::default();
        let render = desk_opened_by_a_handler(&state);
        let saved = desk_snapshot(&state, &render);
        let draw_on = |source: &str| {
            JsxEvaluator::new_draw(
                source,
                serde_json::Value::Null,
                None,
                None,
                Arc::clone(&state),
            )
            .unwrap()
        };

        let shown = draw_on(DESK_RENDER).eval(&HashMap::new()).unwrap().layout;
        let mut texts = Vec::new();
        layout_texts(&shown, &mut texts);
        assert_eq!(texts, ["open=true"]);

        // `draw` code runs outside the render, so its write is refused.
        let draw = draw_on(
            "import { store } from 'tauler'; const desk = store({ open: false, items: [] });\n\
             export default () => <div><D3Figure key=\"k\" width={10} height={10} draw={() => { desk.open = false; }} /></div>;",
        );
        draw.eval(&HashMap::new()).unwrap();
        let error = draw.context.with(|ctx| {
            ctx.eval::<(), _>("__tauler_figures.values().next().value.draw()")
                .catch(&ctx)
                .expect_err("a write from draw code must throw")
                .to_string()
        });
        assert!(
            error.contains("a Store cannot be written from the draw runtime"),
            "threw: {error}"
        );
        assert_eq!(desk_snapshot(&state, &render), saved);
    }

    // ---- Local Stores (`tauler-core/src/local_store.rs`: paths P1 to P6) ----
    //
    // Every test evaluates one evaluator several times: an identity is only worth
    // anything across Ticks.

    /// `label=n`, counting clicks in a local Store.
    const LOCAL_COUNTER: (&str, &str) = (
        "components/Counter.jsx",
        r#"import { store } from "tauler";
export function Counter({ label }) {
    const s = store({ n: 0 });
    return <span on_click={() => { s.n += 1; }}>{`${label ?? "-"}=${s.n}`}</span>;
}
export function Outer({ children }) { return <div>{children}</div>; }
"#,
    );

    /// A function that declares a local Store, in a plain `.js` file: `name=n`, where
    /// `n` counts the renders this value has lived through.
    const LOCAL_TICKS: (&str, &str) = (
        "lib/ticks.js",
        r#"import { store } from "tauler";
export function ticks(name) {
    const s = store({ n: 0 });
    s.n += 1;
    return `${name}=${s.n}`;
}
"#,
    );

    const LOCAL_IMPORTS: &str = "import { Counter, Outer } from './components/Counter';\nimport { ticks } from './lib/ticks.js';\n";

    struct LocalLayout {
        _dir: tempfile::TempDir,
        path: PathBuf,
        source: String,
        state: SharedState,
        evaluator: JsxEvaluator,
    }

    impl LocalLayout {
        /// `layout` (after [`LOCAL_IMPORTS`]) beside [`LOCAL_COUNTER`], [`LOCAL_TICKS`]
        /// and `files`.
        fn try_new(layout: &str, files: &[(&str, &str)]) -> Result<Self, JsError> {
            let source = format!("{LOCAL_IMPORTS}{layout}");
            let mut all = vec![LOCAL_COUNTER, LOCAL_TICKS, ("layout.jsx", source.as_str())];
            all.extend_from_slice(files);
            let (dir, path) = store_fixture(&all);
            let state = SharedState::default();
            let evaluator = JsxEvaluator::new_with_state(
                &source,
                serde_json::Value::Null,
                Some(&path),
                Arc::clone(&state),
            )?;
            Ok(Self {
                _dir: dir,
                path,
                source,
                state,
                evaluator,
            })
        }

        fn new(layout: &str) -> Self {
            Self::try_new(layout, &[])
                .unwrap_or_else(|e| panic!("expected the layout to load, got: {e}"))
        }

        /// What a reload builds: a new evaluator for `layout` on the same store.
        fn rebuilt(&mut self, layout: &str) {
            self.source = format!("{LOCAL_IMPORTS}{layout}");
            self.evaluator = JsxEvaluator::new_with_state(
                &self.source,
                serde_json::Value::Null,
                Some(&self.path),
                Arc::clone(&self.state),
            )
            .unwrap_or_else(|e| panic!("expected the layout to load, got: {e}"));
        }

        fn eval(&self, streams: &[(&str, &str)]) -> Result<serde_json::Value, String> {
            let streams = streams
                .iter()
                .map(|(bin, value)| ((bin.to_string(), None), value.to_string()))
                .collect();
            self.evaluator
                .eval(&streams)
                .map(|out| out.layout)
                .map_err(|e| e.to_string())
        }

        /// One Tick: the texts it rendered, or the error it threw.
        fn tick(&self, streams: &[(&str, &str)]) -> Result<Vec<String>, String> {
            let mut texts = Vec::new();
            layout_texts(&self.eval(streams)?, &mut texts);
            Ok(texts)
        }

        /// Evaluate and run the `nth` `on_click` handler of the tree, in document order.
        fn click(&self, streams: &[(&str, &str)], nth: usize) -> HandlerOutcome {
            let ids = self.handlers(streams);
            self.evaluator
                .invoke_handler_outcome(ids[nth], &serde_json::Value::Null)
        }

        /// Evaluate, and the ids of the tree's `on_click` handlers in document order.
        fn handlers(&self, streams: &[(&str, &str)]) -> Vec<i64> {
            fn handlers(node: &serde_json::Value, out: &mut Vec<i64>) {
                match node {
                    serde_json::Value::Array(items) => {
                        items.iter().for_each(|i| handlers(i, out));
                    }
                    serde_json::Value::Object(map) => {
                        if let Some(id) = map.get("on_click").and_then(|h| h["$handler"].as_i64()) {
                            out.push(id);
                        }
                        if let Some(children) = map.get("children") {
                            handlers(children, out);
                        }
                    }
                    _ => {}
                }
            }
            let mut ids = Vec::new();
            handlers(&self.eval(streams).expect("eval"), &mut ids);
            ids
        }

        /// The local identities the last render declared, without their prefix.
        fn identities(&self) -> Vec<String> {
            let mut identities: Vec<String> = self
                .evaluator
                .state_identities()
                .into_iter()
                .filter_map(|id| Some(id.strip_prefix(crate::state::LOCAL_IDENTITY_PREFIX)?.into()))
                .collect();
            identities.sort();
            identities
        }

        fn stored(&self) -> usize {
            self.state.lock().unwrap().len()
        }
    }

    /// P1 and P4: two instances told apart by `key`, a handler's write, one Tick.
    #[test]
    fn local_store_two_keyed_instances_keep_their_own_values() {
        let layout = LocalLayout::new(
            r#"export default function render() {
                return <div><Counter key="left" label="l" /><Counter key="right" label="r" /></div>;
            }"#,
        );
        assert_eq!(layout.tick(&[]).unwrap(), ["l=0", "r=0"]);
        assert_eq!(
            layout.identities(),
            [
                "components/Counter.jsx#Counter[left] > components/Counter.jsx#Counter.s",
                "components/Counter.jsx#Counter[right] > components/Counter.jsx#Counter.s",
            ]
        );
        let outcome = layout.click(&[], 1);
        assert!(outcome.state_changed, "a handler's write causes a Tick");
        assert_eq!(layout.tick(&[]).unwrap(), ["l=0", "r=1"]);
        layout.click(&[], 0);
        layout.click(&[], 1);
        assert_eq!(layout.tick(&[]).unwrap(), ["l=1", "r=2"]);
        assert_eq!(layout.tick(&[]).unwrap(), ["l=1", "r=2"], "unchanged stays");
        assert_eq!(layout.stored(), 2);
    }

    /// P2: a function from a plain `.js` file declares the Store; it is called from two
    /// components and twice in one, and each call has its own value under the name of
    /// its `const`. Swapping the two calls (a rebuilt evaluator, as an edit gives)
    /// moves no value.
    #[test]
    fn local_store_a_function_called_twice_in_one_component_is_told_apart_by_its_consts() {
        let body = |first: &str, second: &str| {
            format!(
                r#"function Menu({{ skip }}) {{
                    const {first} = ticks("{first}");
                    if (skip) return <div>{{{first}}}</div>;
                    const {second} = ticks("{second}");
                    return <div>{{menu}}{{help}}</div>;
                }}
                function Other() {{ const menu = ticks("other"); return <div>{{menu}}</div>; }}
                export default function render() {{
                    return <div><Menu skip={{useStringStream("skip") === "1"}} /><Other /></div>;
                }}"#
            )
        };
        let mut layout = LocalLayout::new(&body("menu", "help"));
        assert_eq!(layout.tick(&[]).unwrap(), ["menu=1", "help=1", "other=1"]);
        assert_eq!(
            layout.identities(),
            [
                "layout.jsx#Menu > help > lib/ticks.js#ticks.s",
                "layout.jsx#Menu > menu > lib/ticks.js#ticks.s",
                "layout.jsx#Other > menu > lib/ticks.js#ticks.s",
            ]
        );
        assert_eq!(layout.tick(&[]).unwrap(), ["menu=2", "help=2", "other=2"]);

        // The lines swap: with a counter of calls `help` would read `menu`'s value.
        layout.rebuilt(&body("help", "menu"));
        assert_eq!(layout.tick(&[]).unwrap(), ["menu=3", "help=3", "other=3"]);
        // The first call alone: nothing shifts into its place.
        assert_eq!(
            layout.tick(&[("skip", "1")]).unwrap(),
            ["help=4", "other=4"]
        );
    }

    const LOCAL_ROWS: &str = r#"export default function render() {
        const order = useStringStream("order").split(",").filter(Boolean);
        return <div>{order.map((name) => <Counter key={name} label={name} />)}</div>;
    }"#;

    /// P3: rows are components with `key`; a reordered list and a row appearing in
    /// front move no value.
    #[test]
    fn local_store_rows_of_a_list_keep_their_values_by_key() {
        let layout = LocalLayout::new(LOCAL_ROWS);
        let abc = [("order", "a,b,c")];
        assert_eq!(layout.tick(&abc).unwrap(), ["a=0", "b=0", "c=0"]);
        layout.click(&abc, 1);
        layout.click(&abc, 2);
        layout.click(&abc, 2);
        assert_eq!(layout.tick(&abc).unwrap(), ["a=0", "b=1", "c=2"]);
        assert_eq!(
            layout.tick(&[("order", "c,a,b")]).unwrap(),
            ["c=2", "a=0", "b=1"]
        );
        assert_eq!(
            layout.tick(&[("order", "new,c,a,b")]).unwrap(),
            ["new=0", "c=2", "a=0", "b=1"]
        );
    }

    /// Dropping: a local Store the render did not declare is dropped at once, so a row
    /// that comes back starts from its default and nothing is left behind.
    #[test]
    fn local_store_a_row_that_leaves_the_list_is_dropped_and_starts_over() {
        let layout = LocalLayout::new(LOCAL_ROWS);
        let ab = [("order", "a,b")];
        layout.tick(&ab).unwrap();
        layout.click(&ab, 0);
        layout.click(&ab, 1);
        assert_eq!(layout.tick(&ab).unwrap(), ["a=1", "b=1"]);
        assert_eq!(layout.stored(), 2);

        assert_eq!(layout.tick(&[("order", "b")]).unwrap(), ["b=1"]);
        assert_eq!(layout.stored(), 1, "a's Store is gone from the store");
        assert_eq!(layout.tick(&ab).unwrap(), ["a=0", "b=1"]);
    }

    /// A handler a press captured outlives its render. When a later render dropped the
    /// local Store it writes, the write is ignored: no entry comes back, the handler's
    /// other writes are kept, and a write that only reached the dropped Store causes no
    /// Tick. A held local Store and a shared Store are written as before.
    #[test]
    fn local_store_a_handler_held_past_its_render_writes_nothing_to_a_dropped_store() {
        let layout = LocalLayout::new(
            r#"import { store } from "tauler";
            const total = store({ n: 0 });
            function Row({ name, both }) {
                const s = store({ n: 0 });
                const press = () => {
                    s.n = (s.n ?? 0) + 1;
                    s.tags = [name];
                    delete s.gone;
                    if (both) total.n += 1;
                };
                return <span on_click={press}>{`${name}=${s.n}`}</span>;
            }
            export default function render() {
                const order = useStringStream("order").split(",").filter(Boolean);
                const both = useStringStream("both") === "1";
                return <div>{order.map((name) => <Row key={name} name={name} both={both} />)}<span>{`total=${total.n}`}</span></div>;
            }"#,
        );
        let ab = [("order", "a,b")];
        let b = [("order", "b")];
        let row_a = "local://layout.jsx#Row[a] > layout.jsx#Row.s";

        // The press is on row `a`; the next render has no row `a`.
        let ids = layout.handlers(&ab);
        layout.evaluator.capture_handler(ids[0]);
        assert_eq!(layout.tick(&b).unwrap(), ["b=0", "total=0"]);
        assert_eq!(layout.stored(), 2, "row b's and `total`");
        let outcome = layout
            .evaluator
            .invoke_captured_handler_outcome(&serde_json::Value::Null);
        assert!(!outcome.state_changed, "nothing changed, so no Tick");
        assert_eq!(layout.stored(), 2, "no entry without an owner");
        assert!(layout.state.lock().unwrap().snapshot(row_a).is_none());
        assert_eq!(layout.tick(&b).unwrap(), ["b=0", "total=0"]);
        assert_eq!(layout.tick(&ab).unwrap(), ["a=0", "b=0", "total=0"]);

        // The same with a write to a shared Store beside it: that write is kept.
        let both = [("order", "a,b"), ("both", "1")];
        let ids = layout.handlers(&both);
        layout.evaluator.capture_handler(ids[0]);
        layout.tick(&b).unwrap();
        let outcome = layout
            .evaluator
            .invoke_captured_handler_outcome(&serde_json::Value::Null);
        assert!(outcome.state_changed, "`total` changed");
        assert!(layout.state.lock().unwrap().snapshot(row_a).is_none());
        assert_eq!(layout.tick(&b).unwrap(), ["b=0", "total=1"]);

        // A held local Store is written as before, by a captured handler too.
        let ids = layout.handlers(&ab);
        layout.evaluator.capture_handler(ids[1]);
        assert!(layout.click(&ab, 0).state_changed);
        assert!(
            layout
                .evaluator
                .invoke_captured_handler_outcome(&serde_json::Value::Null)
                .state_changed
        );
        assert_eq!(layout.tick(&ab).unwrap(), ["a=1", "b=1", "total=1"]);
        assert_eq!(
            layout.state.lock().unwrap().snapshot(row_a).unwrap()["tags"],
            serde_json::json!(["a"])
        );
    }

    /// Nothing grows per Tick: neither the store nor the runtime's list of identities,
    /// however many keys come and go.
    #[test]
    fn local_store_keys_that_come_and_go_leave_nothing_behind() {
        let layout = LocalLayout::new(LOCAL_ROWS);
        for i in 0..50 {
            let order = format!("k{i},k{}", i + 1);
            assert_eq!(layout.tick(&[("order", &order)]).unwrap().len(), 2);
            assert_eq!(layout.stored(), 2, "tick {i}");
            assert_eq!(layout.evaluator.state_identities().len(), 2, "tick {i}");
        }
        layout.tick(&[("order", "")]).unwrap();
        assert_eq!(layout.stored(), 0);
        assert!(layout.evaluator.state_identities().is_empty());
    }

    fn assert_declared_twice(err: &str, identity: &str) {
        assert!(
            err.contains(&format!(
                "the local Store `{identity}` is declared twice in one render"
            )),
            "{err}"
        );
        assert!(
            err.contains("Make the repeated part a component with its own `key`"),
            "{err}"
        );
    }

    /// Rule 2, one check for every way of reaching one declaration twice.
    #[test]
    fn local_store_declared_twice_in_one_render_throws_and_says_what_to_do() {
        for (body, identity) in [
            // Two siblings with no `key`.
            (
                "return <div><Counter /><Counter /></div>;",
                "components/Counter.jsx#Counter > components/Counter.jsx#Counter.s",
            ),
            // Two siblings with one `key`.
            (
                r#"return <div><Counter key="x" /><Counter key="x" /></div>;"#,
                "components/Counter.jsx#Counter[x] > components/Counter.jsx#Counter.s",
            ),
            // A `.map()` with no `key`.
            (
                r#"return <div>{["a", "b"].map((name) => <Counter label={name} />)}</div>;"#,
                "components/Counter.jsx#Counter > components/Counter.jsx#Counter.s",
            ),
            // The body of a loop: one `const`, reached twice.
            (
                r#"const out = []; for (const name of ["a", "b"]) { const row = ticks(name); out.push(row); } return <div>{out}</div>;"#,
                "row > lib/ticks.js#ticks.s",
            ),
            // A function called twice with no `const` of its own.
            (
                r#"return <div>{ticks("a")}{ticks("b")}</div>;"#,
                "lib/ticks.js#ticks.s",
            ),
            // A component called twice as a plain function.
            (
                "return <div>{Counter({})}{Counter({})}</div>;",
                "components/Counter.jsx#Counter.s",
            ),
        ] {
            let layout =
                LocalLayout::new(&format!("export default function render() {{ {body} }}"));
            let err = layout.tick(&[]).unwrap_err();
            assert_declared_twice(&err, identity);
            assert_eq!(
                layout.stored(),
                0,
                "a render that threw declared nothing: {body}"
            );
        }
    }

    /// The Store is two links down; the identity in the message shows which link
    /// repeats.
    #[test]
    fn local_store_beneath_a_repeated_component_names_the_whole_chain() {
        let layout = LocalLayout::new(
            r#"function Row() { return <div><Counter /></div>; }
            function Board() { return <div><Row /><Row /></div>; }
            export default function render() { return <Board />; }"#,
        );
        assert_declared_twice(
            &layout.tick(&[]).unwrap_err(),
            "layout.jsx#Board > layout.jsx#Row > components/Counter.jsx#Counter > components/Counter.jsx#Counter.s",
        );
    }

    /// The rule is about Stores, not about repetition: nothing here declares one twice.
    #[test]
    fn local_store_repeated_components_with_no_store_beneath_do_not_throw() {
        let layout = LocalLayout::new(
            r#"function Plain({ label }) { return <span>{label}</span>; }
            export default function render() {
                return <div>
                    <Plain label="1" /><Plain label="2" />
                    {["x", "y"].map((name) => <Plain label={name} />)}
                    <Outer><Counter key="l" label="l" /></Outer>
                    <Outer><Counter key="r" label="r" /></Outer>
                </div>;
            }"#,
        );
        assert_eq!(
            layout.tick(&[]).unwrap(),
            ["1", "2", "x", "y", "l=0", "r=0"]
        );
    }

    /// A Tick that threw leaves the stack empty and the store as it was: the next one
    /// renders, and what the throwing render wrote is rolled back.
    #[test]
    fn local_store_a_render_that_throws_rolls_back_and_drops_nothing() {
        let layout = LocalLayout::new(
            r#"function Shown() { const text = ticks("t"); return <span>{text}</span>; }
            export default function render() {
                const mode = useStringStream("mode");
                if (mode === "twice") return <div><Shown /><Shown /></div>;
                if (mode === "throw") return <div><Shown />{(() => { throw new Error("boom"); })()}</div>;
                if (mode === "hidden") return <div><Counter /></div>;
                return <div><Shown /><Counter /></div>;
            }"#,
        );
        assert_eq!(layout.tick(&[]).unwrap(), ["t=1", "-=0"]);
        layout.click(&[], 0);
        assert_eq!(layout.tick(&[]).unwrap(), ["t=3", "-=1"]);

        // Both wrote `n` before they threw; neither declared the Counter's Store.
        assert!(layout.tick(&[("mode", "twice")]).is_err());
        let thrown = layout.tick(&[("mode", "throw")]).unwrap_err();
        assert!(thrown.starts_with("Error: boom"), "{thrown}");
        assert_eq!(layout.stored(), 2, "a render that threw dropped nothing");
        assert_eq!(layout.tick(&[]).unwrap(), ["t=4", "-=1"]);

        // A render that returned without `Shown` drops its Store, and only that one.
        assert_eq!(layout.tick(&[("mode", "hidden")]).unwrap(), ["-=1"]);
        assert_eq!(layout.stored(), 1);
        assert_eq!(layout.tick(&[]).unwrap(), ["t=1", "-=1"]);
    }

    /// Rule 1 allows a condition around a declaration: it is the same Store whenever it
    /// is reached, and dropped while it is not.
    #[test]
    fn local_store_declared_under_a_condition_is_one_store_while_it_is_reached() {
        let layout = LocalLayout::new(
            r#"import { store } from "tauler";
            function Panel({ open }) {
                const always = store({ n: 0 });
                always.n += 1;
                if (!open) return <span>{`closed ${always.n}`}</span>;
                const detail = store({ n: 0 });
                detail.n += 1;
                return <span>{`open ${always.n} ${detail.n}`}</span>;
            }
            export default function render() {
                return <Panel open={useStringStream("open") === "1"} />;
            }"#,
        );
        let open = [("open", "1")];
        assert_eq!(layout.tick(&open).unwrap(), ["open 1 1"]);
        assert_eq!(layout.tick(&open).unwrap(), ["open 2 2"]);
        assert_eq!(layout.tick(&[]).unwrap(), ["closed 3"]);
        assert_eq!(layout.tick(&open).unwrap(), ["open 4 1"]);
    }

    /// A component inside itself and a function that calls itself: each level is one
    /// more link, so each has its own value.
    #[test]
    fn local_store_recursion_gives_each_level_its_own_store() {
        let layout = LocalLayout::new(
            r#"import { store } from "tauler";
            function Tree({ depth }) {
                const s = store({ n: depth * 10 });
                return <div><span>{`tree${depth}=${s.n}`}</span>{depth > 0 && <Tree depth={depth - 1} />}</div>;
            }
            function nest(depth) {
                const s = store({ n: depth * 100 });
                if (depth === 0) return [`nest0=${s.n}`];
                const inner = nest(depth - 1);
                return [`nest${depth}=${s.n}`, ...inner];
            }
            function Nested() { const all = nest(2); return <div>{all}</div>; }
            export default function render() { return <div><Tree depth={2} /><Nested /></div>; }"#,
        );
        assert_eq!(
            layout.tick(&[]).unwrap(),
            [
                "tree2=20",
                "tree1=10",
                "tree0=0",
                "nest2=200",
                "nest1=100",
                "nest0=0"
            ]
        );
        assert_eq!(
            layout.identities(),
            [
                "layout.jsx#Nested > all > inner > inner > layout.jsx#nest.s",
                "layout.jsx#Nested > all > inner > layout.jsx#nest.s",
                "layout.jsx#Nested > all > layout.jsx#nest.s",
                "layout.jsx#Tree > layout.jsx#Tree > layout.jsx#Tree > layout.jsx#Tree.s",
                "layout.jsx#Tree > layout.jsx#Tree > layout.jsx#Tree.s",
                "layout.jsx#Tree > layout.jsx#Tree.s",
            ]
        );
    }

    /// The chain is the authors, not the parents in the output tree: a child written in
    /// `render` is `render`'s, whatever it is nested in, and a component passed as
    /// `children` ran before the component that receives it.
    #[test]
    fn local_store_the_chain_is_the_authors_not_the_parents() {
        let layout = LocalLayout::new(
            r#"function Row() { return <Outer><Counter label="in-row" /></Outer>; }
            function Card() { return <Outer><Outer><Counter label="card" /></Outer></Outer>; }
            export default function render() {
                return <div>
                    <Outer><Counter label="child" /></Outer>
                    <Row key="a" /><Row key="b" />
                    <Card />
                </div>;
            }"#,
        );
        assert_eq!(
            layout.tick(&[]).unwrap(),
            ["child=0", "in-row=0", "in-row=0", "card=0"]
        );
        let counter = "components/Counter.jsx#Counter > components/Counter.jsx#Counter.s";
        assert_eq!(
            layout.identities(),
            [
                counter.to_string(),
                format!("layout.jsx#Card > {counter}"),
                format!("layout.jsx#Row[a] > {counter}"),
                format!("layout.jsx#Row[b] > {counter}"),
            ]
        );
        layout.click(&[], 2);
        assert_eq!(
            layout.tick(&[]).unwrap(),
            ["child=0", "in-row=0", "in-row=1", "card=0"]
        );
    }

    /// A keyed sibling appearing in front leaves the other alone, and a prop change
    /// keeps the value: neither is part of an identity.
    #[test]
    fn local_store_a_sibling_in_front_and_a_prop_change_move_no_value() {
        let layout = LocalLayout::new(
            r#"export default function render() {
                const show = useStringStream("show") === "1";
                return <div>{show && <Counter key="a" label="a" />}<Counter key="b" label={useStringStream("label")} /></div>;
            }"#,
        );
        let hidden = [("label", "one")];
        layout.tick(&hidden).unwrap();
        layout.click(&hidden, 0);
        assert_eq!(layout.tick(&hidden).unwrap(), ["one=1"]);
        assert_eq!(
            layout.tick(&[("show", "1"), ("label", "two")]).unwrap(),
            ["a=0", "two=1"]
        );
    }

    /// `key` is already a prop of `<D3Figure>`, where it may be an object: only a
    /// string or a number is part of a link, and it is encoded so that no key can
    /// spell another link.
    #[test]
    fn local_store_only_a_string_or_a_number_key_is_part_of_a_link() {
        let layout = LocalLayout::new(
            r#"function A() { return <Counter key={{ a: 1 }} />; }
            function B() { return <Counter key={7} />; }
            function C() { return <Counter key="x] > y" />; }
            export default function render() { return <div><A /><B /><C /></div>; }"#,
        );
        layout.tick(&[]).unwrap();
        let counter = "components/Counter.jsx#Counter.s";
        assert_eq!(
            layout.identities(),
            [
                format!("layout.jsx#A > components/Counter.jsx#Counter > {counter}"),
                format!("layout.jsx#B > components/Counter.jsx#Counter[7] > {counter}"),
                format!(
                    "layout.jsx#C > components/Counter.jsx#Counter[x%5D%20%3E%20y] > {counter}"
                ),
            ]
        );

        // Half of a surrogate pair cannot be encoded: the error names the declaration.
        let layout = LocalLayout::new(
            r#"export default function render() { return <Counter key={"\ud800"} />; }"#,
        );
        let err = layout.tick(&[]).unwrap_err();
        assert!(
            err.contains(&format!(
                "`{counter}` is declared beneath a component whose `key` is not valid text"
            )),
            "{err}"
        );
    }

    /// An element built at the top level of a module has no author and no render: the
    /// load fails, so it cannot be placed in two components and share one Store. The
    /// same when a function called there builds it, declared before or after the call.
    #[test]
    fn local_store_beneath_an_element_built_at_the_top_level_of_a_module_fails_the_load() {
        for built in [
            "const built = <Counter />;",
            "function build() { return <Counter />; }\nconst built = build();",
            "const built = build();\nfunction build() { return <Late />; }\nfunction Late() { return <Counter />; }",
        ] {
            let err = LocalLayout::try_new(
                &format!(
                    "{built}\nexport default function render() {{ return <div>{{built}}</div>; }}"
                ),
                &[],
            )
            .err()
            .expect("the load must fail")
            .to_string();
            assert!(
                err.contains("can only be declared while a component renders"),
                "{built}: {err}"
            );
        }
    }

    /// The accepted limit (a): a render prop runs when the receiver calls it, so the
    /// receiver is the author; the built-in `<Module>` is one such receiver and has a
    /// name. Called twice, the receiver declares one identity twice.
    #[test]
    fn local_store_a_render_prop_gets_the_chain_of_the_component_that_calls_it() {
        let layout = LocalLayout::new(
            r#"function Recv({ render }) { return <div>{render("r")}</div>; }
            function Twice({ render }) { return <div>{render("1")}{render("2")}</div>; }
            export default function render() {
                const keyed = useStringStream("keyed") === "1";
                return <div>
                    <Recv render={(label) => <Counter label={label} />} />
                    <Module bin="some-bin">{() => <Counter label="m" />}</Module>
                    <Twice render={(n) => keyed ? <Counter key={n} label={n} /> : <Counter label="once" />} />
                </div>;
            }"#,
        );
        let counter = "components/Counter.jsx#Counter.s";
        assert_declared_twice(
            &layout.tick(&[]).unwrap_err(),
            &format!("layout.jsx#Twice > components/Counter.jsx#Counter > {counter}"),
        );
        assert_eq!(
            layout.tick(&[("keyed", "1")]).unwrap(),
            ["r=0", "m=0", "1=0", "2=0"]
        );
        assert_eq!(
            layout.identities(),
            [
                format!("layout.jsx#Recv > components/Counter.jsx#Counter > {counter}"),
                format!("layout.jsx#Twice > components/Counter.jsx#Counter[1] > {counter}"),
                format!("layout.jsx#Twice > components/Counter.jsx#Counter[2] > {counter}"),
                format!("tauler#Module > components/Counter.jsx#Counter > {counter}"),
            ]
        );
    }

    /// What a function's `.name` alone could not tell apart: two components of one name
    /// from two modules, side by side and as alternatives.
    #[test]
    fn local_store_components_of_one_name_from_two_modules_do_not_share() {
        let toggle = |module: &str| {
            format!(
                r#"import {{ store }} from "tauler";
export function Toggle() {{
    const s = store({{ first: "{module}", n: 0 }});
    s.n += 1;
    return <span>{{`{module}:${{s.first}}:${{s.n}}`}}</span>;
}}"#
            )
        };
        let layout = LocalLayout::try_new(
            r#"import { Toggle as ToggleA } from "./a.jsx";
            import { Toggle as ToggleB } from "./b.jsx";
            export default function render() {
                const which = useStringStream("which");
                if (which === "both") return <div><ToggleA /><ToggleB /></div>;
                return which === "a" ? <ToggleA /> : <ToggleB />;
            }"#,
            &[("a.jsx", &toggle("a")), ("b.jsx", &toggle("b"))],
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            layout.tick(&[("which", "both")]).unwrap(),
            ["a:a:1", "b:b:1"]
        );
        assert_eq!(layout.tick(&[("which", "a")]).unwrap(), ["a:a:2"]);
        // `b` was dropped by the Tick that did not show it, and never read `a`'s.
        assert_eq!(layout.tick(&[("which", "b")]).unwrap(), ["b:b:1"]);
    }

    /// How a component gets its name: every way of binding a function at the top level
    /// of a file, in `.jsx`, `.tsx` and `.ts`, and a default export with no name.
    #[test]
    fn local_store_every_function_bound_at_the_top_level_of_a_file_is_a_named_component() {
        let layout = LocalLayout::try_new(
            r#"import Anonymous from "./anonymous.jsx";
            import ArrowDefault from "./arrow.tsx";
            import { Typed } from "./typed.tsx";
            import { Made } from "./made.tsx";
            import { store } from "tauler";
            const Arrow = () => { const s = store({ v: "arrow" }); return <span>{s.v}</span>; };
            export const Expression = function () { const s = store({ v: "expression" }); return <span>{s.v}</span>; };
            export default function render() {
                return <div><Arrow /><Expression /><Anonymous /><ArrowDefault /><Typed label="typed" /><Made /></div>;
            }"#,
            &[
                (
                    "anonymous.jsx",
                    "import { store } from 'tauler';\nexport default function () { const s = store({ v: 'anonymous' }); return <span>{s.v}</span>; }",
                ),
                (
                    "arrow.tsx",
                    "import { store } from 'tauler';\nexport default () => { const s = store({ v: 'default arrow' }); return <span>{s.v}</span>; };",
                ),
                (
                    "typed.tsx",
                    "import { store } from 'tauler';\ntype Props = { label: string };\nexport const Typed = ({ label }: Props) => { const s = store({ v: label }) as { v: string }; return <span>{s.v}</span>; };",
                ),
                (
                    "made.tsx",
                    "import { make } from './make.ts';\nexport function Made() { const made = make<string>('made'); return <span>{made}</span>; }",
                ),
                (
                    "make.ts",
                    "import { store } from 'tauler';\nexport function make<T>(v: T): T { const s = store({ v }); return s.v as T; }",
                ),
            ],
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            layout.tick(&[]).unwrap(),
            [
                "arrow",
                "expression",
                "anonymous",
                "default arrow",
                "typed",
                "made"
            ]
        );
        assert_eq!(
            layout.identities(),
            [
                "anonymous.jsx#default > anonymous.jsx#default.s",
                "arrow.tsx#default > arrow.tsx#default.s",
                "layout.jsx#Arrow > layout.jsx#Arrow.s",
                "layout.jsx#Expression > layout.jsx#Expression.s",
                "made.tsx#Made > made > make.ts#make.s",
                "typed.tsx#Typed > typed.tsx#Typed.s",
            ]
        );
    }

    /// A component the compile pass cannot name (defined inside a function, or made by
    /// a call) is loud when a Store is declared beneath it, never a silent share, and
    /// costs nothing otherwise.
    #[test]
    fn local_store_beneath_a_component_with_no_name_throws_and_says_where_to_move_it() {
        for (body, name) in [
            (
                "function Host() { const Inline = () => <Counter />; return <Inline />; }",
                "Inline",
            ),
            (
                "const wrap = (C) => (props) => <C {...props} />; const Wrapped = wrap(Counter); function Host() { return <Wrapped />; }",
                "(a function with no name)",
            ),
        ] {
            let layout = LocalLayout::new(&format!(
                "{body}\nexport default function render() {{ return <Host />; }}"
            ));
            let err = layout.tick(&[]).unwrap_err();
            assert!(
                err.contains(&format!("is declared beneath the component `{name}`, which tauler cannot name")),
                "{err}"
            );
            assert!(err.contains("at the top level of its file"), "{err}");
        }
        let layout = LocalLayout::new(
            r#"function Host() { const Inline = ({ label }) => <span>{label}</span>; return <div><Inline label="a" /><Inline label="b" /></div>; }
            export default function render() { return <Host />; }"#,
        );
        assert_eq!(layout.tick(&[]).unwrap(), ["a", "b"]);
    }

    /// A function that declares a Store, called by a handler: no component is
    /// rendering, so there is no instance for the Store to belong to.
    #[test]
    fn local_store_declared_from_a_handler_throws_and_changes_nothing() {
        let layout = LocalLayout::new(
            r#"export default function render() {
                return <div on_click={() => { const late = ticks("late"); }}>{String(1)}</div>;
            }"#,
        );
        let outcome = layout.click(&[], 0);
        assert!(!outcome.state_changed);
        assert_eq!(layout.stored(), 0);
        assert_eq!(layout.tick(&[]).unwrap(), ["1"]);
    }

    /// P5: a reload builds a second evaluator on the same store. A local Store keeps
    /// its value, and a key the edit added to its default is filled in.
    #[test]
    fn local_store_a_key_added_to_the_default_is_filled_in_by_a_reload() {
        let source = |default: &str| {
            format!(
                r#"import {{ store }} from "tauler";
                function Count() {{ const s = store({default}); s.n += 1; return <div>{{`${{s.n}} ${{s.added}}`}}</div>; }}
                export default function render() {{ return <Count />; }}"#
            )
        };
        let mut layout = LocalLayout::new(&source("{ n: 0 }"));
        assert_eq!(layout.tick(&[]).unwrap(), ["1 undefined"]);
        layout.rebuilt(&source("{ n: 0, added: 'later' }"));
        assert_eq!(layout.tick(&[]).unwrap(), ["2 later"]);
    }

    /// The call wrapper changes nothing a layout can see: arguments are evaluated once,
    /// in the author's order, before the callee runs; spread and defaults work; `this`
    /// is `undefined` as in any module; a promise comes back as a promise; and a name
    /// left by a call that threw is gone when the error is caught.
    #[test]
    fn local_store_a_named_call_behaves_as_the_call_the_author_wrote() {
        let layout = LocalLayout::new(
            r#"const log = [];
            const note = (text) => { log.push(text); return text; };
            function join(a, b = "default", ...rest) { log.push("join"); return [a, b, ...rest, String(this)].join(","); }
            function fail() { throw new Error("thrown inside"); }
            async function later() { return 1; }
            function Probe() {
                log.length = 0;
                const joined = join(note("a"), note("b"), ...[note("c"), note("d")]);
                const short = join("only");
                let caught = "";
                try { const never = fail(); } catch (e) { caught = e.message; }
                const promise = later();
                const after = ticks("after");
                return <div>{joined}{short}{caught}{String(promise instanceof Promise)}{log.join(" ")}{after}</div>;
            }
            export default function render() { return <Probe />; }"#,
        );
        assert_eq!(
            layout.tick(&[]).unwrap(),
            [
                "a,b,c,d,undefined",
                "only,default,undefined",
                "thrown inside",
                "true",
                "a b c d join join",
                "after=1",
            ]
        );
        assert_eq!(
            layout.identities(),
            ["layout.jsx#Probe > after > lib/ticks.js#ticks.s"],
            "`never` is not left on the stack by the call that threw"
        );
    }

    // ---- A plain `.js` import ----

    /// The refusals for `globals` and for a `store` that was not imported are not made
    /// in a plain `.js` import (`tauler-core/src/store_pass.rs` tests the rule).
    #[test]
    fn local_store_a_plain_js_import_that_names_globals_and_calls_a_global_store_loads() {
        let vendored = (
            "lib/vendored.js",
            "const g = typeof globals !== 'undefined' ? globals : { n: 5 };\n\
             export const n = typeof store === 'function' ? store(g).n : g.n;\n",
        );
        let layout = LocalLayout::try_new(
            "import { n } from './lib/vendored.js';\n\
             export default function render() { return <div>{String(n)}</div>; }",
            &[vendored],
        )
        .unwrap_or_else(|e| panic!("expected the layout to load, got: {e}"));
        assert_eq!(layout.tick(&[]).unwrap(), ["5"]);
    }

    /// A default export written `export default function () {...}` is a declaration:
    /// JavaScript makes it before any module code runs, so a module that imports it in
    /// a cycle may call it while it loads. The compile pass keeps that, and the
    /// function's `.name`.
    #[test]
    fn an_unnamed_default_function_is_callable_from_an_import_cycle_while_loading() {
        let first = (
            "lib/first.js",
            "import { early } from './second.js';
             export default function () { return 'hoisted'; }
             export { early };
",
        );
        let second = (
            "lib/second.js",
            "import first from './first.js';
             export const early = first();
",
        );
        let layout = LocalLayout::try_new(
            "import first, { early } from './lib/first.js';
             export default function render() { return <div>{`${early}/${first.name}`}</div>; }",
            &[first, second],
        )
        .unwrap_or_else(|e| panic!("expected the layout to load, got: {e}"));
        assert_eq!(layout.tick(&[]).unwrap(), ["hoisted/default"]);
    }

    // ---- A call whose value is destructured ----

    /// A function that declares a Store two calls down, and gives back an object.
    const LOCAL_BOTH: &str =
        "function both(name) { const text = ticks(name); return { text, list: [text] }; }\n";

    fn assert_destructured(err: &str, place: &str) {
        assert!(
            err.contains(&format!(
                "store(): `lib/ticks.js#ticks.s` is declared beneath a call whose value is \
                 destructured, at `{place}`"
            )),
            "{err}"
        );
        assert!(
            err.contains("`const menu = toggle(); const { open, flip } = menu;`"),
            "{err}"
        );
    }

    /// A destructured call has no one name to be found by, so a Store declared beneath
    /// it throws and says what to write; written that way, it works.
    #[test]
    fn local_store_beneath_a_destructured_call_throws_and_says_to_assign_it_first() {
        for (line, declaration) in [
            (5, "const { text } = both(\"a\");"),
            (5, "const [text] = both(\"a\").list;"),
            (5, "let { text } = both(\"a\");"),
            (5, "const { text } =\n both(\"a\");"),
        ] {
            // `.list` after the call is not a call that is the whole value.
            let throws = !declaration.contains(".list");
            let layout = LocalLayout::new(&format!(
                "{LOCAL_BOTH}function Menu() {{\n {declaration}\n return <div>{{text}}</div>;\n}}\n\
                 export default function render() {{ return <Menu />; }}"
            ));
            if !throws {
                // No name and no marker: one such call works, as any call with no `const`.
                assert_eq!(layout.tick(&[]).unwrap(), ["a=1"], "{declaration}");
                continue;
            }
            let err = layout.tick(&[]).unwrap_err();
            assert_destructured(&err, &format!("layout.jsx:{line}"));
            assert_eq!(layout.stored(), 0, "{declaration}");
        }

        let layout = LocalLayout::new(&format!(
            "{LOCAL_BOTH}function Menu() {{
                const menu = both(\"a\");
                const {{ text }} = menu;
                return <div>{{text}}</div>;
            }}
            export default function render() {{ return <Menu />; }}"
        ));
        assert_eq!(layout.tick(&[]).unwrap(), ["a=1"]);
        assert_eq!(layout.tick(&[]).unwrap(), ["a=2"]);
        assert_eq!(
            layout.identities(),
            ["layout.jsx#Menu > menu > text > lib/ticks.js#ticks.s"]
        );
    }

    /// A destructured call of a function that declares no Store is the call the author
    /// wrote: the same value, arguments once and in order, and nothing left behind by
    /// one that threw.
    #[test]
    fn local_store_a_destructured_call_of_a_function_with_no_store_works_as_written() {
        let layout = LocalLayout::new(
            r#"const log = [];
            const note = (text) => { log.push(text); return text; };
            function split(a, b = "default", ...rest) { log.push("split"); return { first: a, others: [b, ...rest], self: String(this) }; }
            function fail() { throw new Error("thrown inside"); }
            const { first: top } = split("top");
            function Probe() {
                log.length = 0;
                const { first, others, self } = split(note("a"), note("b"), ...[note("c")]);
                let [one, two = "two"] = split("only").others;
                const [b, c] = (() => others)();
                let caught = "";
                try { const { never } = fail(); } catch (e) { caught = e.message; }
                const after = ticks("after");
                return <div>{top}{first}{others.join("+")}{self}{one}{two}{b}{c}{caught}{log.join(" ")}{after}</div>;
            }
            export default function render() { return <Probe />; }"#,
        );
        assert_eq!(
            layout.tick(&[]).unwrap(),
            [
                "top",
                "a",
                "b+c",
                "undefined",
                "default",
                "two",
                "b",
                "c",
                "thrown inside",
                "a b c split split",
                "after=1",
            ]
        );
        assert_eq!(
            layout.identities(),
            ["layout.jsx#Probe > after > lib/ticks.js#ticks.s"],
            "the destructured call that threw left nothing that stops a later declaration"
        );
    }

    /// The same call at the top level of a module would declare a shared Store with no
    /// name: it fails the load with the same message.
    #[test]
    fn local_store_beneath_a_destructured_call_at_the_top_level_fails_the_load() {
        let err = LocalLayout::try_new(
            &format!(
                "{LOCAL_BOTH}const {{ text }} = both(\"menu\");\n\
                 export default function render() {{ return <div>{{text}}</div>; }}"
            ),
            &[],
        )
        .err()
        .expect("the load must fail")
        .to_string();
        assert_destructured(&err, "layout.jsx:4");
    }

    // ---- A function that declares a Store, called at the top level of a module ----

    const LOCAL_FACTORY: &str = r#"const menu = ticks("menu");
        export const help = ticks("help");
        export default function render() { return <div>{menu}{help}</div>; }"#;

    /// No component is running, so the identity is only names and the value is shared:
    /// two `const`s, two shared Stores, declared once by the load and kept like any
    /// shared Store, through renders that do not declare them and through a reload.
    #[test]
    fn local_store_declared_through_a_top_level_const_is_a_shared_store() {
        let mut layout = LocalLayout::new(LOCAL_FACTORY);
        let declared = [
            "local://layout.jsx#menu > lib/ticks.js#ticks.s",
            "local://layout.jsx#help > lib/ticks.js#ticks.s",
        ];
        assert_eq!(layout.evaluator.state_identities(), declared);
        assert_eq!(layout.tick(&[]).unwrap(), ["menu=1", "help=1"]);
        assert_eq!(layout.tick(&[]).unwrap(), ["menu=1", "help=1"]);
        assert_eq!(layout.stored(), 2, "a render drops nothing a load declared");
        assert_eq!(layout.evaluator.state_identities(), declared);

        // A reload: the load declares them again and finds the saved values.
        layout.rebuilt(LOCAL_FACTORY);
        let kept = layout.evaluator.state_identities();
        layout.state.lock().unwrap().retain_identities(&kept);
        assert_eq!(layout.tick(&[]).unwrap(), ["menu=2", "help=2"]);
        assert_eq!(layout.stored(), 2);
    }

    /// One identity twice at the top level of a module: the function behind one
    /// `const` reaches the declaration twice. And a call with no `const` there has no
    /// name to be shared under.
    #[test]
    fn local_store_declared_twice_at_the_top_level_of_a_module_fails_the_load() {
        let err = LocalLayout::try_new(
            r#"function pair() { return [ticks("a"), ticks("b")]; }
            const both = pair();
            export default function render() { return <div>{both}</div>; }"#,
            &[],
        )
        .err()
        .expect("the load must fail")
        .to_string();
        assert!(
            err.contains(
                "the shared Store `layout.jsx#both > lib/ticks.js#ticks.s` is declared twice while its file loads"
            ),
            "{err}"
        );

        let err = LocalLayout::try_new(
            r#"ticks("no const");
            export default function render() { return <div />; }"#,
            &[],
        )
        .err()
        .expect("the load must fail")
        .to_string();
        assert!(
            err.contains("can only be declared while a component renders"),
            "{err}"
        );
        assert!(err.contains("assign the call"), "{err}");
    }

    // ---- The reconciler and draw runtimes (P6) ----

    const LOCAL_RUNTIMES: &str = r#"import { store } from "tauler";
const Light = unit({
  key: (i) => i.entity,
  value: (i) => i.state,
  reconciler: optativeSet({ observe: () => [] }),
});
function nest(depth) {
    const s = store({ on: false });
    if (depth === 0) return s;
    const inner = nest(depth - 1);
    return inner;
}
function Switch({ name }) {
    const s = store({ on: false });
    const deep = nest(1);
    return <root>
        <Light entity={name} state={s.on ? "on" : "off"} />
        <div on_click={() => { s.on = !s.on; }}>{`${name}=${s.on}`}</div>
    </root>;
}
export default function render() {
    const names = useStringStream("names").split(",").filter(Boolean);
    return <root>{names.map((name) => <Switch key={name} name={name} />)}</root>;
}"#;

    /// One layout, three runtimes: each builds the same identities; the reconciler
    /// runtime reads what a handler wrote; and a runtime that only reads never drops
    /// and never stores a default, whatever its own render declares.
    #[test]
    fn local_store_identities_are_the_same_in_every_runtime_and_only_the_render_runtime_drops() {
        let state = SharedState::default();
        let both: HashMap<_, _> = HashMap::from([(("names".to_string(), None), "a,b".to_string())]);
        let only_a: HashMap<_, _> = HashMap::from([(("names".to_string(), None), "a".to_string())]);
        let render = JsxEvaluator::new_with_state(
            LOCAL_RUNTIMES,
            serde_json::Value::Null,
            None,
            Arc::clone(&state),
        )
        .unwrap();
        let reconciler = reconciler_on(&state, LOCAL_RUNTIMES);
        let draw = JsxEvaluator::new_draw(
            LOCAL_RUNTIMES,
            serde_json::Value::Null,
            None,
            None,
            Arc::clone(&state),
        )
        .unwrap();

        // A runtime that only reads declares nothing to the store.
        reconciler.eval_units(&both).unwrap();
        draw.eval(&both).unwrap();
        assert_eq!(state.lock().unwrap().len(), 0);

        let shown = render.eval(&both).unwrap().layout;
        let declared = render.state_identities();
        assert_eq!(declared.len(), 6, "{declared:?}");
        assert_eq!(reconciler.state_identities(), declared);
        assert_eq!(draw.state_identities(), declared);
        assert_eq!(state.lock().unwrap().len(), 6);

        // The second Switch's handler writes; the reconciler's next evaluation reads it.
        let id = shown["children"][1]["children"][0]["on_click"]["$handler"]
            .as_i64()
            .unwrap_or_else(|| panic!("no handler in {shown}"));
        assert!(
            render
                .invoke_handler_outcome(id, &serde_json::Value::Null)
                .state_changed
        );
        let batches = reconciler.eval_units(&both).unwrap();
        let states: Vec<&serde_json::Value> = batches
            .iter()
            .flat_map(|batch| batch.items.iter())
            .collect();
        assert_eq!(
            serde_json::to_string(&states)
                .unwrap()
                .matches("\"on\"")
                .count(),
            1,
            "{states:?}"
        );

        // Their renders stop declaring `b`'s Stores: nothing leaves the store.
        reconciler.eval_units(&only_a).unwrap();
        draw.eval(&only_a).unwrap();
        assert_eq!(reconciler.state_identities().len(), 3);
        assert_eq!(state.lock().unwrap().len(), 6);
        // The render runtime's does: its three are dropped.
        render.eval(&only_a).unwrap();
        assert_eq!(state.lock().unwrap().len(), 3);
    }
    const LOCAL_DEFAULTS: &str = r#"import { store } from "tauler";
const Light = unit({
  key: (i) => i.entity,
  value: (i) => i.state,
  reconciler: optativeSet({ observe: () => [] }),
});
function Row({ name }) {
    const s = store({ n: 7, items: ["x", "y"], note: { text: "default" } });
    const press = () => { s.n += 1; s.items.push("z"); delete s.note; };
    const text = `${name}=${s.n}/${s.items.length}/${s.note ? s.note.text : "none"}/${Object.keys(s).join("+")}/${"note" in s}`;
    return <root><Light entity={name} state={text} /><div on_click={press}>{text}</div></root>;
}
export default function render() { return <root><Row key="a" name="a" /></root>; }"#;

    /// A runtime that only reads may reach a local Store's declaration before the
    /// render runtime has declared it. It then reads the declaration's default, nested
    /// values and keys included, and stores nothing; once the store holds the Store it
    /// reads what the store holds, and a key the render runtime deleted stays deleted.
    #[test]
    fn local_store_a_read_only_runtime_reads_the_default_until_the_store_holds_a_value() {
        let state = SharedState::default();
        let streams = HashMap::new();
        let render = JsxEvaluator::new_with_state(
            LOCAL_DEFAULTS,
            serde_json::Value::Null,
            None,
            Arc::clone(&state),
        )
        .unwrap();
        let reconciler = reconciler_on(&state, LOCAL_DEFAULTS);
        let draw = JsxEvaluator::new_draw(
            LOCAL_DEFAULTS,
            serde_json::Value::Null,
            None,
            None,
            Arc::clone(&state),
        )
        .unwrap();
        let drawn = || {
            let mut texts = Vec::new();
            layout_texts(&draw.eval(&streams).unwrap().layout, &mut texts);
            texts
        };
        let reconciled = || {
            let batches = reconciler.eval_units(&streams).unwrap();
            let values: Vec<&serde_json::Value> = batches
                .iter()
                .flat_map(|batch| batch.items.iter())
                .collect();
            serde_json::to_string(&values).unwrap()
        };

        let default = "a=7/2/default/items+n+note/true";
        for _ in 0..2 {
            assert_eq!(drawn(), [default]);
            assert!(reconciled().contains(default), "{}", reconciled());
        }
        assert_eq!(state.lock().unwrap().len(), 0, "a default is never stored");

        // The render runtime declares it, and a handler changes it.
        let shown = render.eval(&streams).unwrap().layout;
        assert_eq!(drawn(), [default]);
        let id = shown["children"][0]["children"][0]["on_click"]["$handler"]
            .as_i64()
            .unwrap_or_else(|| panic!("no handler in {shown}"));
        assert!(
            render
                .invoke_handler_outcome(id, &serde_json::Value::Null)
                .state_changed
        );
        let changed = "a=8/3/none/items+n/false";
        assert_eq!(drawn(), [changed]);
        assert!(reconciled().contains(changed), "{}", reconciled());
    }

    /// A render that throws in a runtime that only reads leaves no default behind for
    /// a local Store only that render declared: no later render would forget it. The
    /// defaults of the renders that returned are kept.
    #[test]
    fn local_store_a_read_only_runtime_keeps_no_default_from_a_render_that_threw() {
        let source = r#"import { store } from "tauler";
let renders = 0;
function Row() {
    const s = store({ n: 7 });
    return <div>{`${s.n}`}</div>;
}
function Late() {
    const s = store({ n: 1 });
    throw new Error(`late ${s.n}`);
}
export default function render() {
    renders += 1;
    return <root><Row key="kept" />{renders === 2 ? <Late key={`late`} /> : null}</root>;
}"#;
        let state = SharedState::default();
        let streams = HashMap::new();
        let reconciler = reconciler_on(&state, source);

        reconciler.eval_units(&streams).unwrap();
        let kept = reconciler.kept_defaults();
        assert_eq!(kept.len(), 1, "{kept:?}");

        let error = reconciler.eval_units(&streams).unwrap_err();
        assert!(error.to_string().contains("late 1"), "{error}");
        assert_eq!(reconciler.kept_defaults(), kept);

        reconciler.eval_units(&streams).unwrap();
        assert_eq!(reconciler.kept_defaults(), kept);
    }
}
