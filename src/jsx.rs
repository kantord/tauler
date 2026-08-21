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
//! The `Runtime` and `Context` are created once and live forever, which is what makes the
//! second phase cost 100–200μs — small enough that re-rendering everything on every tick
//! is affordable (ADR 0007). The transform is under a millisecond and happens only in the
//! first phase. Rasterization dominates both by two orders of magnitude.
//!
//! `_jsx` is registered from Rust as a global. It takes `(tag, props, ...children)` and
//! returns `{ type, ...props, children }` — a plain object, no intermediate
//! representation.
//!
//! Everything a layout file can reach is registered here — in `JSX_GLOBALS_JS` and in the
//! setup below: `useStringStream`, `useJSONStream`, `useEvents`, `Module`, `I3Layout`,
//! `Panel`, `globals` and `ctx`. rquickjs grants nothing by default, so that list is
//! exhaustive by construction rather than by audit (ADR 0008).

use std::collections::HashMap;

use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};
use tauler_core::flatten::flatten_passthrough;
use tauler_core::globals::JSX_GLOBALS_JS;

/// Shared map of stream values: keyed by `(bin, script)`, holds the latest stdout line.
type StreamValues = Arc<RwLock<HashMap<(String, Option<String>), String>>>;
/// Recorded `useStringStream` calls made during the last render invocation.
type StreamCalls = Arc<Mutex<Vec<(String, Option<String>)>>>;

/// Return type of a successful JSX evaluation.
pub struct EvalOutput {
    pub layout: serde_json::Value,
    pub stream_calls: Vec<(String, Option<String>)>,
    pub module_calls: Vec<(String, serde_json::Value)>,
}
type EvalResult = rquickjs::Result<EvalOutput>;

use rquickjs::function::Function;
use rquickjs::loader::{Loader, Resolver};
use rquickjs::{CatchResultExt, Persistent};

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

    globalThis.__tauler_readonly = (obj, name) =>
      new Proxy(obj, {
        set(_t, prop) {
          throw new TypeError(
            "`" + name + "." + String(prop) + "` is read-only here: the bar owns " +
            name + ", a Unit's hooks only read it"
          );
        },
        deleteProperty(_t, prop) {
          throw new TypeError("`" + name + "." + String(prop) + "` is read-only here");
        },
      });

    globalThis.__tauler_dispatch_hook = (unitIndex, hook, one, payload) => {
      const unit = globalThis.__tauler_units[unitIndex];
      if (!unit) return 0;
      const batchFn = unit[hook];
      const oneFn = unit[one];
      if (batchFn && oneFn) {
        throw new TypeError(
          "a unit() may define `" + hook + "` or `" + one + "`, not both"
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

/// A persistent JSX evaluator that compiles the layout source once and re-evaluates
/// cheaply on each tick by calling the pre-compiled render function.
/// The store behind a layout file's `globals`, shareable between the two runtimes.
pub type SharedGlobals = Arc<Mutex<serde_json::Map<String, serde_json::Value>>>;

pub struct JsxEvaluator {
    context: rquickjs::Context,
    _runtime: rquickjs::Runtime,
    stream_values: StreamValues,
    calls: StreamCalls,
    module_calls: Arc<Mutex<Vec<(String, serde_json::Value)>>>,
    global_state: SharedGlobals,
    /// Always `Some` after construction; `None` only transiently during `drop`.
    render_fn: Option<Persistent<Function<'static>>>,
    loaded_paths: Arc<Mutex<Vec<PathBuf>>>,
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

/// No-op resolver/loader pair used in place of a filesystem resolver when
/// `JsxEvaluator::new` is called with `base_dir: None`. `build_runtime` always
/// composes its builtin resolver/loader with a second one, so this pair stands in
/// for "no filesystem access at all" — matching the pre-`build_runtime` behavior,
/// where the `None` branch registered only `builtin_resolver`/`builtin_loader` and
/// any relative/`./`-style import simply failed to resolve.
struct NoFsResolver;

impl Resolver for NoFsResolver {
    fn resolve<'js>(
        &mut self,
        _ctx: &rquickjs::Ctx<'js>,
        base: &str,
        name: &str,
        _attrs: Option<rquickjs::loader::ImportAttributes<'js>>,
    ) -> rquickjs::Result<String> {
        Err(rquickjs::Error::new_resolving(base, name))
    }
}

struct NoFsLoader;

impl Loader for NoFsLoader {
    fn load<'js>(
        &mut self,
        _ctx: &rquickjs::Ctx<'js>,
        name: &str,
        _attrs: Option<rquickjs::loader::ImportAttributes<'js>>,
    ) -> rquickjs::Result<rquickjs::Module<'js, rquickjs::module::Declared>> {
        Err(rquickjs::Error::new_loading(name))
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

impl JsxEvaluator {
    /// The render runtime: evaluated every Tick, and unable to touch the world.
    pub fn new(
        source: &str,
        ctx: serde_json::Value,
        base_dir: Option<&Path>,
    ) -> rquickjs::Result<Self> {
        Self::with_effects(source, ctx, base_dir, Effects::Denied)
    }

    /// The `globals` this evaluator reads and writes.
    ///
    /// Handed to the reconciler runtime so both evaluations of the layout file
    /// see the same answer to "what has the user asked for". Without it a bar
    /// button that flips a Unit's declared state changes only the render side's
    /// copy, and the Unit never notices — the most obvious thing anyone will
    /// write, silently broken.
    pub fn globals_handle(&self) -> SharedGlobals {
        Arc::clone(&self.global_state)
    }

    /// The reconciler runtime: the same layout file, evaluated off the loop, with
    /// `sh` and friends registered so a Unit's hooks can run, and reading the bar's
    /// `globals`.
    pub fn new_reconciler(
        source: &str,
        ctx: serde_json::Value,
        base_dir: Option<&Path>,
        globals: SharedGlobals,
    ) -> rquickjs::Result<Self> {
        let mut evaluator = Self::with_effects(source, ctx, base_dir, Effects::Allowed)?;
        evaluator.global_state = globals;
        Ok(evaluator)
    }

    fn with_effects(
        source: &str,
        ctx: serde_json::Value,
        base_dir: Option<&Path>,
        effects: Effects,
    ) -> rquickjs::Result<Self> {
        let loaded_paths: Arc<Mutex<Vec<PathBuf>>> = Arc::new(Mutex::new(Vec::new()));

        let (runtime, context) = if let Some(dir) = base_dir {
            optative_script::build_runtime(
                crate::ui::registry::UI_COMPONENTS,
                optative_script::loader::ConfinedFsResolver::new(dir.to_path_buf()),
                optative_script::loader::ConfinedFsLoader::new(Arc::clone(&loaded_paths)),
            )?
        } else {
            optative_script::build_runtime(
                crate::ui::registry::UI_COMPONENTS,
                NoFsResolver,
                NoFsLoader,
            )?
        };
        let stream_values: StreamValues = Arc::new(RwLock::new(HashMap::new()));
        let calls: StreamCalls = Arc::new(Mutex::new(Vec::new()));
        let module_calls: Arc<Mutex<Vec<(String, serde_json::Value)>>> =
            Arc::new(Mutex::new(Vec::new()));

        let mut stored_render_fn: Option<Persistent<Function<'static>>> = None;

        {
            let sv = Arc::clone(&stream_values);
            let calls_inner = Arc::clone(&calls);
            let module_calls_inner = Arc::clone(&module_calls);
            context.with(|qjs_ctx| {
                qjs_ctx.eval::<(), _>(JSX_GLOBALS_JS)?;
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
                qjs_ctx.eval::<(), _>(format!(
                    "globalThis.h = (type, props, ...children) => __tauler_flatten_node(__esto_h(type, __tauler_register_handlers(type, props), ...children));
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
                crate::ui::registry::register_ui_components(&qjs_ctx)?;
                if !ctx.is_null() {
                    let js_ctx = rquickjs_serde::to_value(qjs_ctx.clone(), &ctx)
                        .map_err(|_| rquickjs::Error::Unknown)?;
                    qjs_ctx.globals().set("ctx", js_ctx)?;
                }

                let js_source = optative_script::jsx::transform_source(source, "layout.jsx");
                let module = rquickjs::Module::declare(qjs_ctx.clone(), "layout.jsx", js_source)?;
                let (module, promise) = module.eval()?;
                promise.finish::<()>()?;
                let render_fn: Function = module.get("default")?;
                stored_render_fn = Some(Persistent::save(&qjs_ctx, render_fn));

                Ok::<(), rquickjs::Error>(())
            })?;
        }

        let global_state = Arc::new(Mutex::new(serde_json::Map::new()));
        Ok(Self {
            context,
            _runtime: runtime,
            stream_values,
            calls,
            module_calls,
            global_state,
            render_fn: stored_render_fn,
            loaded_paths,
        })
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

    /// A handler that throws is reported and dispatches nothing — a gesture that does
    /// nothing beats a bar that dies (ADR 0021). Reported once per handler, because this
    /// sits on the input path: a mapper that throws would otherwise log on every motion
    /// event, which is once a pixel.
    pub fn invoke_handler(
        &self,
        id: i64,
        pointer: &serde_json::Value,
    ) -> Option<serde_json::Value> {
        self.context.with(|ctx| {
            let f = ctx
                .globals()
                .get::<_, Function>("__tauler_invoke_handler")
                .ok()?;
            let arg = rquickjs_serde::to_value(ctx.clone(), pointer).ok()?;
            let out = f
                .call::<_, rquickjs::Value>((id, arg))
                .catch(&ctx)
                .map_err(|e| warn_handler_once(id, &e))
                .ok()?;
            let intents: serde_json::Value = rquickjs_serde::from_value(out).ok()?;
            (!intents.is_null()).then_some(intents)
        })
    }

    pub fn eval(
        &self,
        new_stream_values: &HashMap<(String, Option<String>), String>,
    ) -> EvalResult {
        self.stream_values
            .write()
            .unwrap()
            .clone_from(new_stream_values);
        self.calls.lock().unwrap().clear();
        self.module_calls.lock().unwrap().clear();
        self.reset_handlers();

        self.context.with(|qjs_ctx| {
            let globals_val =
                rquickjs_serde::to_value(qjs_ctx.clone(), &*self.global_state.lock().unwrap())
                    .map_err(|_| rquickjs::Error::Unknown)?;
            qjs_ctx.globals().set("globals", globals_val)?;

            let render_fn = self.render_fn.as_ref().unwrap().clone().restore(&qjs_ctx)?;
            let value: rquickjs::Value = render_fn
                .call::<(), rquickjs::Value>(())
                .catch(&qjs_ctx)
                .map_err(|e| {
                    tracing::error!(exception = %e, "JS exception");
                    rquickjs::Error::Exception
                })?;

            let updated_globals: rquickjs::Value = qjs_ctx.globals().get("globals")?;
            if let Ok(new_state) = rquickjs_serde::from_value::<
                serde_json::Map<String, serde_json::Value>,
            >(updated_globals)
            {
                *self.global_state.lock().unwrap() = new_state;
            }

            let json_str = qjs_ctx
                .json_stringify(value)?
                .ok_or(rquickjs::Error::Unknown)?
                .to_string()?;
            let layout: serde_json::Value =
                serde_json::from_str(&json_str).map_err(|_| rquickjs::Error::Unknown)?;
            let layout = crate::units::strip_items(flatten_passthrough(layout));
            Ok(EvalOutput {
                layout,
                stream_calls: self.calls.lock().unwrap().clone(),
                module_calls: self.module_calls.lock().unwrap().clone(),
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
    ) -> rquickjs::Result<Vec<crate::units::UnitBatch>> {
        self.stream_values
            .write()
            .unwrap()
            .clone_from(new_stream_values);
        self.calls.lock().unwrap().clear();
        self.module_calls.lock().unwrap().clear();
        self.reset_handlers();

        self.context.with(|qjs_ctx| {
            let globals_val =
                rquickjs_serde::to_value(qjs_ctx.clone(), &*self.global_state.lock().unwrap())
                    .map_err(|_| rquickjs::Error::Unknown)?;
            qjs_ctx.globals().set("__tauler_globals_raw", globals_val)?;
            // The bar owns `globals`; the reconciler only reads them. A hook that
            // assigns to one is trying to report, and hooks do not report — the
            // next `observe` is what says whether anything happened (ADR 0035).
            qjs_ctx.eval::<(), _>(
                "globalThis.globals = __tauler_readonly(__tauler_globals_raw, 'globals');",
            )?;

            let render_fn = self.render_fn.as_ref().unwrap().clone().restore(&qjs_ctx)?;
            let tree: rquickjs::Value = render_fn
                .call::<(), rquickjs::Value>(())
                .catch(&qjs_ctx)
                .map_err(|e| {
                tracing::error!(exception = %e, "JS exception");
                rquickjs::Error::Exception
            })?;

            qjs_ctx.eval::<(), _>(collect_units_source())?;
            qjs_ctx.eval::<(), _>(dispatch_hook_source())?;
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
                .map_err(|e| tracing::error!(exception = %e, hook, "Unit hook threw"))
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
                .map_err(|e| tracing::error!(exception = %e, projection, "projection threw"))
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
                .map_err(|e| tracing::error!(exception = %e, "observe threw"))
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
    /// — see ADR 0033 for what a Unit is.
    #[test]
    fn a_layout_file_can_declare_a_unit_and_render_an_item() {
        let result = eval(
            r#"
            const Light = unit({
              key: (i) => i.entity,
              value: (i) => i.state,
              reconciler: optativeSet({ observe: () => [] }),
              enter: (i) => `on ${i.entity}`,
            });
            export default function render() {
              return <root><Light entity="light.desk" state="on" /></root>;
            }"#,
        )
        .layout;
        let item = &result["children"][0];
        assert_eq!(
            item["entity"], "light.desk",
            "an Item's props must reach the tree: got {result}"
        );
        assert_eq!(item["state"], "on");
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
        let ctx =
            serde_json::json!({ "output": "DP-4", "dpi": 96.0, "width": 250, "outer_gap": 8 });
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

    #[test]
    fn globals_object_persists_value_across_eval_calls() {
        let evaluator = JsxEvaluator::new(
            r#"export default function render() {
globals.count ??= 0;
globals.count += 1;
return <span class="text-white">{String(globals.count)}</span>;
}"#,
            serde_json::Value::Null,
            None,
        )
        .unwrap();

        let streams = std::collections::HashMap::new();
        let r1 = evaluator.eval(&streams).unwrap().layout;
        assert_eq!(r1["children"][0], "1");

        let r2 = evaluator.eval(&streams).unwrap().layout;
        assert_eq!(r2["children"][0], "2");

        let r3 = evaluator.eval(&streams).unwrap().layout;
        assert_eq!(r3["children"][0], "3");
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
            Some(tmp_dir.as_path()),
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
            Some(tmp_dir.as_path()),
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
}
