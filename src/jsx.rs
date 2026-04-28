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

use rquickjs::{CatchResultExt, Persistent};
use rquickjs::function::Function;
use rquickjs::loader::{BuiltinLoader, BuiltinResolver, Loader, Resolver};

use std::path::PathBuf;

use oxc_allocator::Allocator;
use oxc_codegen::Codegen;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use oxc_transformer::{JsxOptions, JsxRuntime, TransformOptions, Transformer};

const JSX_GLOBALS_JS: &str = r#"
    globalThis._jsx = (tag, props, ...children) => {
        const flat = children.flat().filter(c => c !== null && c !== undefined && c !== false);
        if (typeof tag === 'function') {
            return tag({ ...props, children: flat });
        }
        if (tag === 'text') {
            const text = flat.length === 1 && typeof flat[0] === 'object'
                ? flat[0]
                : flat.join('');
            return { type: tag, ...props, text };
        }
        return { type: tag, ...props, children: flat };
    };
    globalThis.useJSONStream = (bin, script) => {
        const str = useStringStream(bin, script);
        if (!str) return null;
        try { return JSON.parse(str); } catch { return null; }
    };
    globalThis.Module = ({ bin, children, ...rest }) => {
        const child = Array.isArray(children) ? children[0] : children;
        if (typeof child === 'function') {
            registerModule(bin, rest);
            const data = useJSONStream(bin);
            const events = new Proxy({}, {
                get: (_, type) => ({ __channel__: bin, type: String(type) })
            });
            return child(data, events);
        }
        return { "bin@": bin, ...rest };
    };
"#;

/// Resolves relative import specifiers (starting with `./` or `../`) against
/// a fixed `base_dir`. Paths outside `allowed_root` are rejected.
struct CostaeResolver {
    allowed_root: PathBuf,
    base_dir: PathBuf,
    resolver: oxc_resolver::Resolver,
}

impl CostaeResolver {
    fn new(base_dir: PathBuf) -> Self {
        let canonical_root = base_dir.canonicalize().unwrap_or_else(|_| base_dir.clone());
        let resolver = oxc_resolver::Resolver::new(oxc_resolver::ResolveOptions {
            modules: vec![],
            extensions: vec![".js".into(), ".jsx".into(), ".ts".into(), ".tsx".into()],
            ..oxc_resolver::ResolveOptions::default()
        });
        Self { allowed_root: canonical_root.clone(), base_dir: canonical_root, resolver }
    }
}

impl Resolver for CostaeResolver {
    fn resolve(&mut self, _ctx: &rquickjs::Ctx, base: &str, name: &str) -> rquickjs::Result<String> {
        if !name.starts_with("./") && !name.starts_with("../") {
            return Err(rquickjs::Error::new_resolving(base, name));
        }

        let resolve_dir = if Path::new(base).is_absolute() {
            Path::new(base)
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| self.base_dir.clone())
        } else {
            self.base_dir.clone()
        };

        let resolution = self.resolver
            .resolve(&resolve_dir, name)
            .map_err(|_| rquickjs::Error::new_resolving(base, name))?;

        let resolved = resolution.full_path().to_path_buf();
        let canonical = resolved
            .canonicalize()
            .unwrap_or_else(|_| resolved.clone());

        if !canonical.starts_with(&self.allowed_root) {
            return Err(rquickjs::Error::new_resolving(base, name));
        }

        canonical
            .to_str()
            .map(|s| s.to_string())
            .ok_or_else(|| rquickjs::Error::new_resolving(base, name))
    }
}

/// Loads JS/JSX modules from disk, running `transform_jsx` on each file before
/// handing the source to QuickJS. Records each successfully-loaded path into
/// the shared `loaded_paths` vec.
struct CostaeLoader {
    loaded_paths: Arc<Mutex<Vec<PathBuf>>>,
}

impl CostaeLoader {
    fn new(loaded_paths: Arc<Mutex<Vec<PathBuf>>>) -> Self {
        Self { loaded_paths }
    }
}

impl Loader for CostaeLoader {
    fn load<'js>(&mut self, ctx: &rquickjs::Ctx<'js>, name: &str) -> rquickjs::Result<rquickjs::Module<'js>> {
        let source = std::fs::read_to_string(name)
            .map_err(|_| rquickjs::Error::new_loading(name))?;
        self.loaded_paths.lock().unwrap().push(PathBuf::from(name));
        let transformed = transform_jsx(&source);
        rquickjs::Module::declare(ctx.clone(), name, transformed)
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

impl JsxEvaluator {
    pub fn new(source: &str, ctx: serde_json::Value, base_dir: Option<&Path>) -> rquickjs::Result<Self> {
        let runtime = rquickjs::Runtime::new()?;
        let loaded_paths: Arc<Mutex<Vec<PathBuf>>> = Arc::new(Mutex::new(Vec::new()));

        let builtin_resolver = crate::ui::registry::UI_COMPONENTS.iter().fold(
            BuiltinResolver::default(),
            |r, e| r.with_module(e.module_path),
        );
        let builtin_loader = crate::ui::registry::UI_COMPONENTS.iter().fold(
            BuiltinLoader::default(),
            |l, e| l.with_module(e.module_path, crate::ui::registry::synthetic_module_source(e)),
        );
        if let Some(dir) = base_dir {
            runtime.set_loader(
                (builtin_resolver, CostaeResolver::new(dir.to_path_buf())),
                (builtin_loader, CostaeLoader::new(Arc::clone(&loaded_paths))),
            );
        } else {
            runtime.set_loader(builtin_resolver, builtin_loader);
        }

        let context = rquickjs::Context::full(&runtime)?;
        let stream_values: StreamValues = Arc::new(RwLock::new(HashMap::new()));
        let calls: StreamCalls = Arc::new(Mutex::new(Vec::new()));
        let module_calls: Arc<Mutex<Vec<(String, serde_json::Value)>>> = Arc::new(Mutex::new(Vec::new()));

        let mut stored_render_fn: Option<Persistent<Function<'static>>> = None;

        {
            let sv = Arc::clone(&stream_values);
            let calls_inner = Arc::clone(&calls);
            let module_calls_inner = Arc::clone(&module_calls);
            context.with(|qjs_ctx| {
                qjs_ctx.eval::<(), _>(JSX_GLOBALS_JS)?;
                let func = rquickjs::Function::new(qjs_ctx.clone(), move |bin: String, script: Option<String>| {
                    calls_inner.lock().unwrap().push((bin.clone(), script.clone()));
                    sv.read().unwrap().get(&(bin, script)).cloned().unwrap_or_default()
                })?;
                qjs_ctx.globals().set("useStringStream", func)?;
                let func2 = rquickjs::Function::new(qjs_ctx.clone(), move |bin: String, props: rquickjs::Value| {
                    let props: serde_json::Value = rquickjs_serde::from_value(props)
                        .unwrap_or(serde_json::Value::Null);
                    let mut mc = module_calls_inner.lock().unwrap();
                    if !mc.iter().any(|(b, _)| b == &bin) {
                        mc.push((bin, props));
                    }
                })?;
                qjs_ctx.globals().set("registerModule", func2)?;
                crate::ui::registry::register_ui_components(&qjs_ctx)?;
                if !ctx.is_null() {
                    let js_ctx = rquickjs_serde::to_value(qjs_ctx.clone(), &ctx)
                        .map_err(|_| rquickjs::Error::Unknown)?;
                    qjs_ctx.globals().set("ctx", js_ctx)?;
                }

                let js_source = transform_jsx(source);
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

    pub fn eval(
        &self,
        new_stream_values: &HashMap<(String, Option<String>), String>,
    ) -> EvalResult {
        self.stream_values.write().unwrap().clone_from(new_stream_values);
        self.calls.lock().unwrap().clear();
        self.module_calls.lock().unwrap().clear();

        self.context.with(|qjs_ctx| {
            let globals_val = rquickjs_serde::to_value(qjs_ctx.clone(), &*self.global_state.lock().unwrap())
                .map_err(|_| rquickjs::Error::Unknown)?;
            qjs_ctx.globals().set("globals", globals_val)?;

            let render_fn = self.render_fn.as_ref().unwrap().clone().restore(&qjs_ctx)?;
            let value: rquickjs::Value = render_fn.call::<(), rquickjs::Value>(())
                .catch(&qjs_ctx)
                .map_err(|e| { tracing::error!(exception = %e, "JS exception"); rquickjs::Error::Exception })?;

            let updated_globals: rquickjs::Value = qjs_ctx.globals().get("globals")?;
            if let Ok(new_state) = rquickjs_serde::from_value::<serde_json::Map<String, serde_json::Value>>(updated_globals) {
                *self.global_state.lock().unwrap() = new_state;
            }

            let json_str = qjs_ctx
                .json_stringify(value)?
                .ok_or(rquickjs::Error::Unknown)?
                .to_string()?;
            let layout = serde_json::from_str(&json_str).map_err(|_| rquickjs::Error::Unknown)?;
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

pub fn transform_jsx(source: &str) -> String {
    let allocator = Allocator::default();
    let source_type = SourceType::jsx();

    let ret = Parser::new(&allocator, source, source_type).parse();
    let mut program = ret.program;

    let scoping = SemanticBuilder::new()
        .with_excess_capacity(2.0)
        .build(&program)
        .semantic
        .into_scoping();

    let options = TransformOptions {
        jsx: JsxOptions {
            runtime: JsxRuntime::Classic,
            pragma: Some("_jsx".to_string()),
            pragma_frag: Some("_jsxFrag".to_string()),
            ..JsxOptions::enable()
        },
        ..TransformOptions::default()
    };

    Transformer::new(&allocator, Path::new("input.jsx"), &options)
        .build_with_scoping(scoping, &mut program);

    Codegen::new().build(&program).code
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
        let result = eval(r#"export default function render() { return <text tw="flex">{"hello"}</text>; }"#).layout;
        assert_eq!(result["type"], "text");
        assert_eq!(result["tw"], "flex");
        assert_eq!(result["text"], "hello");
    }

    #[test]
    fn transform_jsx_self_closing_element_with_tw_prop() {
        let result = transform_jsx(r#"<text tw="flex" />"#);
        assert!(result.contains("_jsx"), "expected '_jsx' in output, got: {result}");
        assert!(result.contains("\"text\""), "expected '\"text\"' in output, got: {result}");
        assert!(result.contains("\"flex\""), "expected '\"flex\"' in output, got: {result}");
    }

    #[test]
    fn jsx_evaluator_nested_tree_parses_to_node() {
        let result = eval(r#"export default function render() { return <container tw="flex flex-col"><text tw="text-white">{"hello"}</text></container>; }"#).layout;
        let node = crate::parse_layout(&result);
        assert!(node.is_ok(), "parse_layout failed: {:?}", node);
    }

    #[test]
    fn use_string_stream_returns_injected_value() {
        let mut streams = std::collections::HashMap::new();
        streams.insert(("/usr/bin/bash".to_string(), Some("echo hi".to_string())), "hello".to_string());
        let result = JsxEvaluator::new(
            r#"export default function render() { return <text tw="text-white">{useStringStream("/usr/bin/bash", "echo hi")}</text>; }"#,
            serde_json::Value::Null,
            None,
        ).unwrap().eval(&streams).unwrap().layout;
        assert_eq!(result["text"], "hello");
    }

    #[test]
    fn jsx_evaluator_injects_ctx_into_script() {
        let ctx = serde_json::json!({ "output": "DP-4", "dpi": 96.0, "width": 250, "outer_gap": 8 });
        let value = JsxEvaluator::new(
            r#"export default function render() { return <text tw="text-white">{ctx.output}</text>; }"#,
            ctx,
            None,
        ).unwrap().eval(&std::collections::HashMap::new()).unwrap().layout;
        let node = crate::parse_layout(&value);
        assert!(node.is_ok(), "parse_layout failed: {:?}", node);
    }

    #[test]
    fn jsx_evaluator_records_stream_calls() {
        let streams_called = eval(
            r#"export default function render() { return <text tw="text-white">{useStringStream("/bin/bash", "script1")}{useStringStream("/bin/bash", "script2")}</text>; }"#,
        ).stream_calls;
        assert!(streams_called.contains(&("/bin/bash".to_string(), Some("script1".to_string()))));
        assert!(streams_called.contains(&("/bin/bash".to_string(), Some("script2".to_string()))));
    }

    #[test]
    fn module_render_prop_exposes_channel_in_events() {
        let result = eval(
            r#"export default function render() { return <Module bin="/usr/bin/test">{(data, events) => <text tw="text-white">{events.doThing.__channel__}</text>}</Module>; }"#,
        ).layout;
        assert_eq!(result["text"], "/usr/bin/test");
    }

    #[test]
    fn use_json_stream_parses_latest_json_output() {
        let mut streams = std::collections::HashMap::new();
        streams.insert(("/usr/bin/test".to_string(), None), r#"{"name":"hello"}"#.to_string());
        let result = JsxEvaluator::new(
            r#"export default function render() { return <text tw="text-white">{useJSONStream("/usr/bin/test").name}</text>; }"#,
            serde_json::Value::Null,
            None,
        ).unwrap().eval(&streams).unwrap().layout;
        assert_eq!(result["text"], "hello");
    }

    #[test]
    fn module_component_records_module_call() {
        let module_calls = eval(
            r#"export default function render() { return <Module bin="/usr/bin/test-module">{(data, events) => <text tw="text-white">hi</text>}</Module>; }"#,
        ).module_calls;
        assert!(module_calls.iter().any(|(bin, _)| bin == "/usr/bin/test-module"));
    }

    #[test]
    fn globals_object_persists_value_across_eval_calls() {
        let evaluator = JsxEvaluator::new(
            r#"export default function render() {
globals.count ??= 0;
globals.count += 1;
return <text tw="text-white">{String(globals.count)}</text>;
}"#,
            serde_json::Value::Null,
            None,
        ).unwrap();

        let streams = std::collections::HashMap::new();
        let r1 = evaluator.eval(&streams).unwrap().layout;
        assert_eq!(r1["text"], "1");

        let r2 = evaluator.eval(&streams).unwrap().layout;
        assert_eq!(r2["text"], "2");

        let r3 = evaluator.eval(&streams).unwrap().layout;
        assert_eq!(r3["text"], "3");
    }

    #[test]
    fn jsx_evaluator_reflects_updated_stream_values_on_second_call() {
        let evaluator = JsxEvaluator::new(
            r#"export default function render() { return <text tw="text-white">{useStringStream("/bin/bash", "echo hi")}</text>; }"#,
            serde_json::Value::Null,
            None,
        ).unwrap();

        let mut streams1 = std::collections::HashMap::new();
        streams1.insert(("/bin/bash".to_string(), Some("echo hi".to_string())), "first".to_string());
        let result1 = evaluator.eval(&streams1).unwrap().layout;
        assert_eq!(result1["text"], "first");

        let mut streams2 = std::collections::HashMap::new();
        streams2.insert(("/bin/bash".to_string(), Some("echo hi".to_string())), "second".to_string());
        let result2 = evaluator.eval(&streams2).unwrap().layout;
        assert_eq!(result2["text"], "second");
    }

    /// Regression: stream keys `(bin, None)` and `(bin, Some(""))` must be distinct.
    #[test]
    fn stream_key_none_and_some_empty_are_not_interchangeable() {
        let key_for_none: (String, Option<String>) = ("/usr/bin/foo".to_string(), None);
        let key_for_empty: (String, Option<String>) = ("/usr/bin/foo".to_string(), Some("".to_string()));
        let mut map: std::collections::HashMap<(String, Option<String>), &str> = std::collections::HashMap::new();
        map.insert(key_for_none, "value_for_none");
        map.insert(key_for_empty, "value_for_empty");
        assert_eq!(map.len(), 2, "(bin, None) and (bin, Some(\"\")) must be distinct map keys");
    }

    #[test]
    fn jsx_evaluator_supports_export_default_render_function() {
        let streams = std::collections::HashMap::new();
        let result = JsxEvaluator::new(
            r#"export default function render() { return <text tw="text-white">hello</text>; }"#,
            serde_json::Value::Null,
            None,
        ).unwrap().eval(&streams).unwrap().layout;
        assert_eq!(result["type"], "text", "expected type=text, got: {:?}", result);
        assert_eq!(result["text"], "hello", "expected text=hello, got: {:?}", result);
    }

    #[test]
    fn jsx_evaluator_resolves_sibling_import_from_disk() {
        let tmp_dir = std::env::temp_dir().join(format!("costae_sibling_import_{}", std::process::id()));
        std::fs::create_dir_all(&tmp_dir).expect("failed to create temp dir");
        std::fs::write(tmp_dir.join("Foo.jsx"), "export default function Foo() { return 42; }")
            .expect("failed to write Foo.jsx");

        let layout_source = r#"import Foo from './Foo.jsx';
export default function render() { return <text tw="text-white">{String(Foo())}</text>; }"#;

        let streams = std::collections::HashMap::new();
        let result = JsxEvaluator::new(layout_source, serde_json::Value::Null, Some(tmp_dir.as_path()))
            .unwrap()
            .eval(&streams)
            .unwrap().layout;

        assert_eq!(result["text"], "42", "expected text=42 from imported Foo, got: {:?}", result);
        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn loaded_paths_includes_imported_sibling() {
        let tmp_dir = std::env::temp_dir()
            .join(format!("costae_loaded_paths_{}", std::process::id()));
        std::fs::create_dir_all(&tmp_dir).expect("failed to create temp dir");
        std::fs::write(tmp_dir.join("Comp.jsx"), "export default function Comp() { return 1; }")
            .expect("failed to write Comp.jsx");

        let layout_source = r#"import Comp from './Comp.jsx';
export default function render() { return <text tw="text-white">{String(Comp())}</text>; }"#;

        let evaluator = JsxEvaluator::new(
            layout_source,
            serde_json::Value::Null,
            Some(tmp_dir.as_path()),
        )
        .expect("JsxEvaluator::new failed");

        let canonical_comp = tmp_dir.join("Comp.jsx").canonicalize()
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
            r#"export default function render() { return <text tw="text-white">hi</text>; }"#,
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
        let result = eval(r#"export default function render() {
const show = false;
return <container tw="flex">
  <text tw="text-white">visible</text>
  {show && <text tw="text-white">hidden</text>}
  {null}
</container>;
}"#).layout;
        let children = result["children"].as_array().unwrap();
        assert_eq!(children.len(), 1, "expected 1 child, got: {:?}", children);
        assert_eq!(children[0]["text"], "visible");
    }
}
