//! What a browser can call.
//!
//! Glue only. The page assigns these onto `globalThis` under the names a layout file
//! expects, and the layout file then runs in the browser's own engine against the same
//! globals QuickJS gives it (ADR 0027).
//!
//! The Transport lives here rather than in JavaScript so the stream map has one
//! implementation whichever transport fills it — a subprocess on a desktop, a worker or a
//! socket in a page.

use std::cell::RefCell;
use std::collections::HashMap;

use serde::Serialize;
use serde_json::Value;
use wasm_bindgen::prelude::*;

thread_local! {
    static STREAM_VALUES: RefCell<HashMap<(String, Option<String>), String>> =
        RefCell::new(HashMap::new());
}

/// Convert to JavaScript as plain objects, not `Map`s.
///
/// `serde_wasm_bindgen` defaults to `Map`, which breaks quietly: the tree still renders,
/// because it goes straight back into Rust to be walked, but every JavaScript read of it —
/// a shim reaching for `props.on_change`, the runtime walking `node.children` — yields
/// `undefined`.
fn to_js<T: Serialize + ?Sized>(value: &T) -> Result<JsValue, JsValue> {
    let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
    Ok(value.serialize(&serializer)?)
}

/// Push a Stream value in. The whole of the Transport's write side.
#[wasm_bindgen(js_name = taulerSetStreamValue)]
pub fn set_stream_value(bin: String, script: Option<String>, line: String) {
    STREAM_VALUES.with(|v| v.borrow_mut().insert((bin, script), line));
}

/// `globalThis.useStringStream`.
#[wasm_bindgen(js_name = taulerUseStringStream)]
pub fn use_string_stream(bin: String, script: Option<String>) -> String {
    STREAM_VALUES.with(|v| v.borrow().get(&(bin, script)).cloned().unwrap_or_default())
}

/// `globalThis.registerModule` — accepts and ignores.
///
/// On a desktop this is what makes tauler spawn a subprocess. A page's transports are
/// registered by whoever owns them, so the declaration tells it nothing it does not know.
#[wasm_bindgen(js_name = taulerRegisterModule)]
pub fn register_module(_bin: String, _props: JsValue) {}

/// Rewrite a tree's theme tokens for one mode, as the desktop does before rendering.
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
#[wasm_bindgen(js_name = taulerRender)]
pub fn render(tree: JsValue) -> Result<JsValue, JsValue> {
    let tree: Value = serde_wasm_bindgen::from_value(tree)?;
    let out = crate::dom::render_output(&tree).map_err(|e| JsValue::from_str(&e.to_string()))?;
    to_js(&out)
}

/// The component shims, for the page to evaluate once after the exports are in place.
#[wasm_bindgen(js_name = taulerBootstrapJs)]
pub fn bootstrap_js() -> String {
    crate::ui::registry::web_bootstrap_js()
}

/// The globals a layout file is evaluated against — the same string `jsx.rs` gives QuickJS.
#[wasm_bindgen(js_name = taulerGlobalsJs)]
pub fn globals_js() -> String {
    crate::globals::JSX_GLOBALS_JS.to_string()
}

/// Diff declared Items against observed ones for one Unit — the identical
/// reconciliation a native Unit runs (`units_reconcile`, ADR 0036). No shell,
/// no second runtime: the caller already *is* the browser's own JS engine, so
/// `key`/`value`/`observe` stay plain JS and only the diff crosses into Rust.
///
/// `desired`/`observed` are arrays already projected to `{key, value, props,
/// order}` — the shape `units_reconcile::SweepItem` deserializes from.
/// Returns `{exit, update, enter}`, each ready to hand a hook straight.
#[wasm_bindgen(js_name = taulerReconcileUnit)]
pub fn reconcile_unit(desired: JsValue, observed: JsValue) -> Result<JsValue, JsValue> {
    let desired: Vec<crate::units_reconcile::SweepItem> = serde_wasm_bindgen::from_value(desired)?;
    let observed: Vec<crate::units_reconcile::SweepItem> =
        serde_wasm_bindgen::from_value(observed)?;
    let (exit, update, enter) = crate::units_reconcile::reconcile(desired, observed);
    to_js(&serde_json::json!({ "exit": exit, "update": update, "enter": enter }))
}
