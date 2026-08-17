//! What a browser can call.
//!
//! Every export here is glue: it holds no logic that the desktop does not also run. The
//! page assigns these onto `globalThis` under the names a layout file expects, evaluates
//! [`crate::ui::registry::web_bootstrap_js`], and from then on the layout file is running
//! in the browser's own engine with the same globals it has in QuickJS (ADR 0025).
//!
//! The Transport lives here rather than in JavaScript. A Stream is `(bin, script) →
//! latest line` pushed in from outside, and that map, its key normalisation and its
//! missing-value behaviour have one implementation whichever transport is filling it —
//! a subprocess on a desktop, a worker or a socket or an SSH bridge in a page.

use std::cell::RefCell;
use std::collections::HashMap;

use serde::Serialize;
use serde_json::Value;
use wasm_bindgen::prelude::*;

/// Convert a value to JavaScript as **plain objects**, not `Map`s.
///
/// `serde_wasm_bindgen`'s default is a `Map`, and it is the wrong default here for a
/// reason that is invisible until something reads the result: a layout tree crossing this
/// boundary is read by JavaScript — a component shim reaching for `props.on_change`, the
/// runtime walking `node.children` to find a handler — and every one of those reads
/// silently yields `undefined` on a `Map`. The tree renders, because it comes straight
/// back into Rust to be walked; only the JavaScript that looks inside it breaks, and it
/// breaks quietly.
fn to_js<T: Serialize + ?Sized>(value: &T) -> Result<JsValue, JsValue> {
    let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
    Ok(value.serialize(&serializer)?)
}

thread_local! {
    /// The latest line of every Stream, exactly as `jsx.rs` keeps it on the desktop.
    static STREAM_VALUES: RefCell<HashMap<(String, Option<String>), String>> =
        RefCell::new(HashMap::new());
    /// Every `useStringStream` call made during the last render, so a host can learn
    /// which streams a layout actually asked for.
    static STREAM_CALLS: RefCell<Vec<(String, Option<String>)>> = const { RefCell::new(Vec::new()) };
    /// Every `registerModule` declaration, merged by bin: one subprocess, union of props.
    static MODULE_CALLS: RefCell<Vec<(String, Value)>> = const { RefCell::new(Vec::new()) };
}

/// Push a Stream value in. The whole of the Transport's write side.
#[wasm_bindgen(js_name = taulerSetStreamValue)]
pub fn set_stream_value(bin: String, script: Option<String>, line: String) {
    STREAM_VALUES.with(|v| v.borrow_mut().insert((bin, script), line));
}

/// `globalThis.useStringStream`. Records the call, then answers with the latest line.
#[wasm_bindgen(js_name = taulerUseStringStream)]
pub fn use_string_stream(bin: String, script: Option<String>) -> String {
    STREAM_CALLS.with(|c| c.borrow_mut().push((bin.clone(), script.clone())));
    STREAM_VALUES.with(|v| v.borrow().get(&(bin, script)).cloned().unwrap_or_default())
}

/// `globalThis.registerModule`. Declaring the same bin twice contributes both
/// declarations' props, which is what makes one subprocess serve several `<Module>`s.
#[wasm_bindgen(js_name = taulerRegisterModule)]
pub fn register_module(bin: String, props: JsValue) -> Result<(), JsValue> {
    let props: Value = serde_wasm_bindgen::from_value(props).unwrap_or(Value::Null);
    MODULE_CALLS.with(|m| {
        let mut m = m.borrow_mut();
        match m.iter_mut().find(|(b, _)| b == &bin) {
            Some((_, existing)) => merge_missing(existing, props),
            None => m.push((bin, props)),
        }
    });
    Ok(())
}

/// Merge `incoming`'s keys into `existing`, keeping what is already there.
///
/// The same rule `app.rs` applies to module props on the desktop: first declaration wins,
/// later ones only add.
fn merge_missing(existing: &mut Value, incoming: Value) {
    let (Value::Object(existing), Value::Object(incoming)) = (existing, incoming) else {
        return;
    };
    for (k, v) in incoming {
        existing.entry(k).or_insert(v);
    }
}

/// Which streams the last render read, and clear the record.
#[wasm_bindgen(js_name = taulerTakeStreamCalls)]
pub fn take_stream_calls() -> Result<JsValue, JsValue> {
    let calls: Vec<Value> = STREAM_CALLS.with(|c| {
        c.borrow_mut()
            .drain(..)
            .map(|(bin, script)| serde_json::json!({ "bin": bin, "script": script }))
            .collect()
    });
    to_js(&calls)
}

/// Which modules the last render declared, and clear the record.
#[wasm_bindgen(js_name = taulerTakeModuleCalls)]
pub fn take_module_calls() -> Result<JsValue, JsValue> {
    let calls: Vec<Value> = MODULE_CALLS.with(|m| {
        m.borrow_mut()
            .drain(..)
            .map(|(bin, props)| serde_json::json!({ "bin": bin, "props": props }))
            .collect()
    });
    to_js(&calls)
}

/// Rewrite a tree's theme tokens for one mode, in place, exactly as the desktop does
/// before rendering.
#[wasm_bindgen(js_name = taulerResolveTheme)]
pub fn resolve_theme(tree: JsValue, mode: &str) -> Result<JsValue, JsValue> {
    let mut tree: Value = serde_wasm_bindgen::from_value(tree)?;
    let theme = crate::theme::Theme::default_theme();
    let mode = match mode {
        "light" => crate::theme::ThemeMode::Light,
        _ => crate::theme::ThemeMode::Dark,
    };
    crate::theme::resolver::resolve_theme_tokens(&mut tree, &theme, mode);
    to_js(&tree)
}

/// A layout tree in, an [`crate::dom::Output`] envelope out.
///
/// The envelope names its own kind so that glue meeting one it does not recognise can say
/// so, rather than reaching for `.dom` and getting `undefined`.
#[wasm_bindgen(js_name = taulerRender)]
pub fn render(tree: JsValue) -> Result<JsValue, JsValue> {
    let tree: Value = serde_wasm_bindgen::from_value(tree)?;
    let out = crate::dom::render_output(&tree).map_err(|e| JsValue::from_str(&e.to_string()))?;
    to_js(&out)
}

/// The shim sources, for the page to evaluate once after the exports are in place.
#[wasm_bindgen(js_name = taulerBootstrapJs)]
pub fn bootstrap_js() -> String {
    crate::ui::registry::web_bootstrap_js()
}

/// `h`'s per-node hook: the passthrough shape in, the layout tree's flat shape out.
///
/// The browser's `h` is JavaScript because `optative-script`'s is bound to QuickJS
/// (ADR 0025), but tauler's own half of the factory is not duplicated — this is the same
/// function the desktop calls, reached across the wasm boundary.
#[wasm_bindgen(js_name = taulerFlattenNode)]
pub fn flatten_node(node: JsValue) -> Result<JsValue, JsValue> {
    let node: Value = serde_wasm_bindgen::from_value(node)?;
    Ok(serde_wasm_bindgen::to_value(
        &crate::flatten::flatten_passthrough(node),
    )?)
}

/// The globals a layout file is evaluated against, as source for the page to `eval`.
///
/// The same string `jsx.rs` evaluates into a QuickJS realm — see [`crate::globals`].
#[wasm_bindgen(js_name = taulerGlobalsJs)]
pub fn globals_js() -> String {
    crate::globals::JSX_GLOBALS_JS.to_string()
}
