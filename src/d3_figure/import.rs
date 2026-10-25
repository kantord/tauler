//! The d3 import — `import * as d3 from "d3"` (and `import { scaleLinear } from
//! "d3"`) in a layout file: turns d3's official UMD build, as
//! [`super::fetch::Fetcher`] supplies it, into an ES module the resolver in
//! `jsx.rs` can hand to QuickJS.
//!
//! d3 ships no ES-module single file, only the UMD `d3.min.js`. Its header picks
//! CommonJS, then AMD, and otherwise assigns `globalThis.d3`. [`es_module_source`]
//! runs it with `globalThis`/`self` shadowed by a private object, so the layout's
//! realm is left with no `d3` global, and re-exports every name in
//! [`d3_exports`] plus the whole namespace as the default export.
//!
//! The export list is static (`d3-7.9.0-exports.txt`, the sorted
//! `Object.keys(d3)` of the pinned file) because ES module exports must be known
//! before the ES module runs. A name in the list that the UMD file does not define
//! exports `undefined`; that is what lets the jsx tests run a tiny fake UMD file
//! through the real list.

/// The specifier a layout file imports d3 by.
pub const SPECIFIER: &str = "d3";

/// Every name d3 7.9.0 exports, in sorted order.
pub fn d3_exports() -> Vec<&'static str> {
    include_str!("d3-7.9.0-exports.txt")
        .lines()
        .filter(|line| !line.is_empty())
        .collect()
}

/// An ES module that runs `umd` in a private scope (no `d3` global left behind)
/// and exports each of `exports` by name plus the whole namespace as default.
///
/// The UMD header's last branch does `(t = globalThis || t || self).d3 = ...`, so
/// passing `__scope` as both `globalThis` and `self` makes it write there instead
/// of on the realm's global. Its `t` is `this`, which is `undefined` in module
/// code; the `globalThis` branch wins before `t` is read.
///
/// Each name is bound under a `__d3_` alias and exported as itself, never as a
/// module-level binding of its own name: d3 exports `window`, and its UMD body
/// reads the global `window`, which a module-level `const window` would shadow
/// (and leave uninitialized while the UMD runs).
pub fn es_module_source(umd: &str, exports: &[&str]) -> String {
    let bindings: Vec<String> = exports.iter().map(|n| format!("{n}: __d3_{n}")).collect();
    let renames: Vec<String> = exports.iter().map(|n| format!("__d3_{n} as {n}")).collect();
    format!(
        "const __scope = {{}};\n\
         (function (globalThis, self) {{\n{umd}\n}}).call(undefined, __scope, __scope);\n\
         const __d3 = __scope.d3;\n\
         export default __d3;\n\
         const {{ {bindings} }} = __d3;\n\
         export {{ {renames} }};\n",
        bindings = bindings.join(", "),
        renames = renames.join(", "),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Shaped like d3's own UMD header, but defines only two names.
    const FAKE_UMD: &str = r#"!function(t,n){"object"==typeof exports&&"undefined"!=typeof module?n(exports):"function"==typeof define&&define.amd?define(["exports"],n):n((t="undefined"!=typeof globalThis?globalThis:t||self).d3=t.d3||{})}(this,(function(t){t.answer=42;t.double=x=>2*x}));"#;

    #[test]
    fn d3_exports_lists_578_unique_names() {
        let exports = d3_exports();
        let unique: HashSet<_> = exports.iter().collect();

        assert_eq!(exports.len(), 578);
        assert_eq!(unique.len(), 578);
    }

    #[test]
    fn es_module_source_exports_the_umd_names_and_a_default_and_leaves_no_d3_global() {
        let runtime = rquickjs::Runtime::new().expect("runtime");
        let context = rquickjs::Context::full(&runtime).expect("context");
        let source = es_module_source(FAKE_UMD, &["answer", "double"]);

        context.with(|ctx| {
            let (module, promise) = rquickjs::Module::declare(ctx.clone(), "d3", source)
                .expect("declare")
                .eval()
                .expect("eval");
            promise.finish::<()>().expect("module evaluation");
            let ns = module.namespace().expect("namespace");

            let answer: i32 = ns.get("answer").expect("answer");
            let double: rquickjs::Function = ns.get("double").expect("double");
            let doubled: i32 = double.call((2,)).expect("double(2)");
            let default: rquickjs::Object = ns.get("default").expect("default");
            let default_answer: i32 = default.get("answer").expect("default.answer");
            let default_double: rquickjs::Function = default.get("double").expect("default.double");
            let typeof_d3: String = ctx.eval("typeof globalThis.d3").expect("typeof");

            assert_eq!(answer, 42);
            assert_eq!(doubled, 4);
            assert_eq!(default_answer, 42);
            assert_eq!(
                default_double
                    .call::<_, i32>((3,))
                    .expect("default.double(3)"),
                6
            );
            assert_eq!(typeof_d3, "undefined");
        });
    }

    /// The guard that the static export list matches the pinned d3 file: every
    /// listed name is defined once the real UMD runs through [`es_module_source`].
    #[test]
    #[ignore = "downloads d3 from npm"]
    fn every_listed_export_is_defined_by_the_real_d3() {
        use crate::d3_figure::assets;
        let dir = tempfile::tempdir().expect("tempdir");
        let body = crate::d3_figure::fetch::download_https(assets::D3.url).expect("download");
        assets::store(&assets::D3, dir.path(), &body).expect("store");
        let real_d3 = assets::load(&assets::D3, dir.path()).expect("load");

        let runtime = rquickjs::Runtime::new().expect("runtime");
        let context = rquickjs::Context::full(&runtime).expect("context");
        let source = es_module_source(&real_d3, &d3_exports());

        context.with(|ctx| {
            let (module, promise) = rquickjs::Module::declare(ctx.clone(), "d3", source)
                .expect("declare")
                .eval()
                .expect("eval");
            promise.finish::<()>().expect("module evaluation");
            let ns = module.namespace().expect("namespace");

            let missing: Vec<_> = d3_exports()
                .into_iter()
                .filter(|name| {
                    let value: rquickjs::Value = ns.get(*name).expect("get");
                    value.is_undefined()
                })
                .collect();
            let typeof_d3: String = ctx.eval("typeof globalThis.d3").expect("typeof");

            assert!(missing.is_empty(), "undefined exports: {missing:?}");
            assert_eq!(typeof_d3, "undefined");
        });
    }
}
