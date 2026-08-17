//! The wasm module a page loads.
//!
//! Deliberately almost empty. Every export is `tauler_core`'s — the components generated
//! by `#[component]`, and the glue in `tauler_core::web`. This crate exists only because
//! `wasm-bindgen` needs a `cdylib` to process, and because making it a separate crate
//! keeps `cargo build` on a desktop from producing a shared object nobody wants.
//!
//! `re_export` is load-bearing rather than decorative: `wasm-bindgen`'s descriptors live
//! in a custom section emitted per crate, and referencing a symbol from `tauler_core` is
//! what stops the linker discarding the section that describes them.

use wasm_bindgen::prelude::*;

/// Forces `tauler_core`'s `wasm-bindgen` section to be linked, and gives the page a
/// cheap way to check that the module it loaded is the one it expected.
#[wasm_bindgen(js_name = taulerVersion)]
pub fn version() -> String {
    // Touching a real export, not just naming the crate: a `use` alone can be discarded.
    let _ = tauler_core::ui::registry::WEB_COMPONENTS.len();
    env!("CARGO_PKG_VERSION").to_string()
}
