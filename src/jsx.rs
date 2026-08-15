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

const JSX_GLOBALS_JS: &str = r#"
    globalThis.useJSONStream = (bin, script) => {
        const str = useStringStream(bin, script);
        if (!str) return null;
        try { return JSON.parse(str); } catch { return null; }
    };
    // `props` are merged into the module's init event (see merge_module_props in app.rs),
    // so they are load-bearing. Registering the same bin more than once contributes
    // every declaration's props (see registerModule): one subprocess, union of props.
    globalThis.useEvents = (bin, props) => {
        registerModule(bin, props ?? {});
        return new Proxy({}, {
            get: (_, type) => (args) => ({
                channel: bin,
                event: { type: String(type), ...args }
            })
        });
    };
    // Declaration only: <I3Layout> reads these, positions them, and emits the
    // real <panel> nodes. Kept a marker rather than a panel because a panel's
    // position depends on every sibling declared before it.
    globalThis.Panel = (props) => ({ ...props, __i3panel: true });
    // Dispatch only — the layout arithmetic is `ui::components::i3_layout`.
    // The gaps must be registered here rather than in Rust: registration is a
    // JS-side call, and a Rust component has no context to make one.
    globalThis.I3Layout = ({ module, children }) => {
        const decls = (Array.isArray(children) ? children : [children]).filter(Boolean);
        const out = __ui_i3_layout({
            children: decls,
            width: ctx.screen_width,
            height: ctx.screen_height,
        });
        if (module) useEvents(module, { gaps: out.gaps });
        return out.panels;
    };
    // Handlers that are functions cannot cross the JSON boundary, so they stay here
    // and the tree carries `{$handler: n}` instead (ADR 0021). Rebuilt every tick;
    // a drag holds its own reference in __tauler_captured, so clearing is safe
    // mid-gesture.
    globalThis.__tauler_handlers = [];
    globalThis.__tauler_captured = null;
    // Only real elements. A component's props are its own — turning `on_change` into
    // a handler id would hand <Slider> an object where it expects a function.
    // Park a function in the registry and hand back the reference the tree carries.
    // Anything that is not a function passes through, so a plain intent array is left
    // exactly as written. A JS shim that calls a Rust component directly has to use
    // this itself — those props never pass through `h`.
    globalThis.__tauler_handler_ref = (fn) =>
        typeof fn === "function" ? { $handler: __tauler_handlers.push(fn) - 1 } : fn;
    globalThis.__tauler_register_handlers = (type, props) => {
        if (typeof type !== "string" || !props) return props;
        let out = null;
        for (const key in props) {
            if (key.startsWith("on_") && typeof props[key] === "function") {
                out = out ?? { ...props };
                out[key] = __tauler_handler_ref(props[key]);
            }
        }
        return out ?? props;
    };
    globalThis.__tauler_capture_handler = (id) => {
        __tauler_captured = __tauler_handlers[id] ?? null;
    };
    globalThis.__tauler_release_handler = () => { __tauler_captured = null; };
    // `id < 0` means the captured one, which outlives the tick it was registered in.
    // A handler may return one intent or several; downstream only ever sees an array.
    globalThis.__tauler_intents = (out) =>
        out == null ? null : (Array.isArray(out) ? out : [out]);
    globalThis.__tauler_invoke_handler = (id, pointer) => {
        const fn = id < 0 ? __tauler_captured : __tauler_handlers[id];
        if (typeof fn !== "function") return null;
        return __tauler_intents(fn(pointer));
    };
    globalThis.Module = ({ bin, children, ...rest }) => {
        const child = Array.isArray(children) ? children[0] : children;
        if (typeof child === 'function') return child(useJSONStream(bin), useEvents(bin, rest));
        return { "bin@": bin, ...rest };
    };
"#;

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
pub struct JsxEvaluator {
    context: rquickjs::Context,
    _runtime: rquickjs::Runtime,
    stream_values: StreamValues,
    calls: StreamCalls,
    module_calls: Arc<Mutex<Vec<(String, serde_json::Value)>>>,
    global_state: Arc<Mutex<serde_json::Map<String, serde_json::Value>>>,
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

impl JsxEvaluator {
    pub fn new(
        source: &str,
        ctx: serde_json::Value,
        base_dir: Option<&Path>,
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
                let flatten_node_fn =
                    rquickjs::Function::new(qjs_ctx.clone(), tauler_flatten_node)?;
                qjs_ctx.globals().set("__tauler_flatten_node", flatten_node_fn)?;
                // `h` isn't aliased directly to `__esto_h`: its generic-tag output nests
                // props under a `props` key (`{type, props, children}`), but Rust-backed UI
                // components (e.g. `@ui/card`) deserialize their `children: Vec<Node>` prop
                // eagerly, mid-render, expecting the flat shape — so each node must be
                // reshaped as soon as it's produced, not just once at the very end.
                qjs_ctx.eval::<(), _>(format!(
                    "globalThis.h = (type, props, ...children) => __tauler_flatten_node(__esto_h(type, __tauler_register_handlers(type, props), ...children));
                    globalThis.Fragment = {{ {}: true }};",
                    optative_script::tags::ESTO_FRAGMENT
                ))?;
                qjs_ctx.globals().set(
                    "useStringStream",
                    rquickjs::Function::new(
                        qjs_ctx.clone(),
                        move |bin: String, script: Option<String>| {
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
            let layout = flatten_passthrough(layout);
            Ok(EvalOutput {
                layout,
                stream_calls: self.calls.lock().unwrap().clone(),
                module_calls: self.module_calls.lock().unwrap().clone(),
            })
        })
    }

    /// Returns the canonicalized paths of all files loaded via import statements
    /// during `new()`. Does not include the inline layout source itself.
    pub fn loaded_paths(&self) -> Vec<PathBuf> {
        self.loaded_paths.lock().unwrap().clone()
    }
}

/// The `h` shim's per-call hook (see `JsxEvaluator::new`): reshapes `__esto_h`'s result
/// via [`flatten_passthrough`] immediately after each `h()` call, not just once at the
/// end of `eval()`. This matters because Rust-backed UI components (e.g. `@ui/card`)
/// deserialize their `children` prop *during* render, synchronously, expecting the flat
/// shape already — by the time the whole tree is done and `eval()`'s own
/// `flatten_passthrough` pass runs, it would be too late for those components.
/// Non-passthrough results (arrays from `Fragment`, primitives, already-flat
/// Rust-component output) are returned untouched.
fn tauler_flatten_node<'js>(
    ctx: rquickjs::Ctx<'js>,
    value: rquickjs::Value<'js>,
) -> rquickjs::Result<rquickjs::Value<'js>> {
    let is_passthrough_node = match value.as_object() {
        Some(obj) => obj.contains_key("type")? && obj.contains_key("props")?,
        None => false,
    };
    if !is_passthrough_node {
        return Ok(value);
    }
    let as_json: serde_json::Value =
        rquickjs_serde::from_value(value).map_err(|_| rquickjs::Error::Unknown)?;
    let flattened = flatten_passthrough(as_json);
    rquickjs_serde::to_value(ctx, &flattened).map_err(|_| rquickjs::Error::Unknown)
}

/// Bridges `optative_script::register_h`'s generic passthrough shape
/// (`{ type, props: {...}, children }`) to the flat shape tauler's own downstream
/// consumers expect (`{ type, ...props, children }`), recursively.
///
/// Props are written after the tag name, so **a `type` prop overrides the tag**.
/// That is deliberate, and load-bearing: it is what makes the long-hand
/// `<surface type="wallpaper">` spelling equivalent to `<wallpaper>` (see
/// `layout::parse_root_node`). The flip side is that the rule is global — any
/// node given a `type` prop becomes that type instead.
///
/// Text needs no special case: a string child stays a string child, and the layout
/// walker is what turns it into a text node (`docs/adr/0016`).
///
/// Applied twice: once per node via [`tauler_flatten_node`] (the `h` shim's hook, so
/// each node is already flat by the time any Rust-backed component consumes it as a
/// `children` prop), and once more here over the whole tree in `eval()` as a final,
/// now-idempotent safety net.
fn flatten_passthrough(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.into_iter().map(flatten_passthrough).collect())
        }
        serde_json::Value::Object(mut map) => {
            let is_passthrough_node = map.contains_key("type") && map.contains_key("props");
            if !is_passthrough_node {
                for (_, v) in map.iter_mut() {
                    *v = flatten_passthrough(std::mem::take(v));
                }
                return serde_json::Value::Object(map);
            }

            let node_type = map.remove("type").unwrap_or(serde_json::Value::Null);
            let props = map.remove("props").unwrap_or(serde_json::Value::Null);
            let children = map
                .remove("children")
                .map(flatten_passthrough)
                .unwrap_or(serde_json::Value::Array(Vec::new()));

            let mut flat = serde_json::Map::new();
            flat.insert("type".to_string(), node_type.clone());
            if let serde_json::Value::Object(props_map) = props {
                for (k, v) in props_map {
                    flat.insert(k, v);
                }
            }

            flat.insert("children".to_string(), children);

            serde_json::Value::Object(flat)
        }
        other => other,
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
