//! Which components exist, and how each one reaches a JavaScript realm.
//!
//! There are two realms and therefore two tables. [`UI_COMPONENTS`] is the QuickJS one and
//! is handed straight to `optative-script`; [`WEB_COMPONENTS`] is the browser one and is
//! read by the build step that generates the page's ES-module shims (ADR 0025).
//!
//! They are hand-listed rather than collected, because Rust has no way to enumerate items
//! carrying an attribute. `web_table_matches_quickjs_table` is what stops the two lists
//! drifting apart.

#[cfg(feature = "quickjs")]
pub use optative_script::EsEntry;

/// One component, as the browser needs to know it.
///
/// The Rust half arrives as a `#[wasm_bindgen]` export named `global_name` (or, for a
/// shimmed component, named by the shim's own call into it). `shim_js` is evaluated in the
/// page after the exports are assigned, and is the same source the QuickJS side evaluates.
pub struct WebComponent {
    pub module_path: &'static str,
    pub export_name: &'static str,
    /// The global `import { X } from "<module_path>"` resolves to.
    pub global_name: &'static str,
    /// JavaScript that must run before `global_name` exists.
    pub shim_js: Option<&'static str>,
}

use crate::ui::components::{knob::KNOB_SHIM_JS, slider::SLIDER_SHIM_JS};

/// Every component the browser can import.
///
/// `i3_layout` is deliberately absent: it reads `ctx.screen_width` and emits `<panel>`
/// shell nodes, neither of which a page has (ADR 0024).
pub const WEB_COMPONENTS: &[WebComponent] = &[
    web("@ui/badge", "Badge", "__ui_badge"),
    web("@ui/card", "Card", "__ui_card"),
    web("@ui/icon", "Icon", "__ui_icon"),
    web("@ui/card", "CardHeader", "__ui_card_header"),
    web("@ui/card", "CardTitle", "__ui_card_title"),
    web("@ui/card", "CardDescription", "__ui_card_description"),
    web("@ui/card", "CardContent", "__ui_card_content"),
    web("@ui/card", "CardFooter", "__ui_card_footer"),
    web("@ui/datatable", "DataTable", "__ui_data_table"),
    WebComponent {
        module_path: "@ui/knob",
        export_name: "Knob",
        global_name: "__tauler_knob",
        shim_js: Some(KNOB_SHIM_JS),
    },
    web("@ui/progress", "Progress", "__ui_progress"),
    WebComponent {
        module_path: "@ui/slider",
        export_name: "Slider",
        global_name: "__tauler_slider",
        shim_js: Some(SLIDER_SHIM_JS),
    },
    web("@ui/table", "Table", "__ui_table"),
    web("@ui/table", "TableHeader", "__ui_table_header"),
    web("@ui/table", "TableBody", "__ui_table_body"),
    web("@ui/table", "TableRow", "__ui_table_row"),
    web("@ui/table", "TableHead", "__ui_table_head"),
    web("@ui/table", "TableCell", "__ui_table_cell"),
];

const fn web(
    module_path: &'static str,
    export_name: &'static str,
    global_name: &'static str,
) -> WebComponent {
    WebComponent {
        module_path,
        export_name,
        global_name,
        shim_js: None,
    }
}

/// The JavaScript that must run once, after the wasm exports are on `globalThis` and
/// before any layout module is imported.
///
/// Only the shims. Each one is the same source the QuickJS side evaluates — a shim exists
/// because `on_change` has to be resolved in JavaScript before Rust sees any props, and
/// that argument does not change with the engine.
pub fn web_bootstrap_js() -> String {
    WEB_COMPONENTS
        .iter()
        .filter_map(|c| c.shim_js)
        .collect::<Vec<_>>()
        .join("\n")
}

/// The ES modules `@ui/*` resolve to in a browser, as `(specifier, source)`.
///
/// One module per `module_path`, exporting every component declared under it, so a layout
/// file's `import { Card, CardHeader } from "@ui/card"` works unaltered. This is the same
/// shape `optative-script::synthetic_module_source_for_entries` synthesises for QuickJS,
/// written out as files instead of resolved in memory — a page has no module loader to
/// hook, only an import map.
pub fn web_module_sources() -> Vec<(String, String)> {
    let mut modules: Vec<&'static str> = WEB_COMPONENTS.iter().map(|c| c.module_path).collect();
    modules.sort_unstable();
    modules.dedup();

    modules
        .into_iter()
        .map(|module| {
            let mut names: Vec<&str> = Vec::new();
            let mut source = String::new();
            for c in WEB_COMPONENTS.iter().filter(|c| c.module_path == module) {
                source.push_str(&format!(
                    "const {} = globalThis.{};\n",
                    c.export_name, c.global_name
                ));
                names.push(c.export_name);
            }
            source.push_str(&format!("export {{ {} }};\n", names.join(", ")));
            (module.to_string(), source)
        })
        .collect()
}

#[cfg(feature = "quickjs")]
use crate::ui::components::{
    badge::__UI_ENTRY_BADGE,
    card::{
        __UI_ENTRY_CARD, __UI_ENTRY_CARD_CONTENT, __UI_ENTRY_CARD_DESCRIPTION,
        __UI_ENTRY_CARD_FOOTER, __UI_ENTRY_CARD_HEADER, __UI_ENTRY_CARD_TITLE,
    },
    i3_layout::__UI_ENTRY_I3_LAYOUT,
    icon::__UI_ENTRY_ICON,
    // The shims, not `__UI_ENTRY_KNOB`/`__UI_ENTRY_SLIDER`: each registers its own Rust
    // half, and `on_change` has to be resolved in JavaScript before Rust sees any props.
    knob::__UI_ENTRY_KNOB_SHIM,
    progress::__UI_ENTRY_PROGRESS,
    slider::__UI_ENTRY_SLIDER_SHIM,
    table::datatable::__UI_ENTRY_DATA_TABLE,
    table::{
        __UI_ENTRY_TABLE, __UI_ENTRY_TABLE_BODY, __UI_ENTRY_TABLE_CELL, __UI_ENTRY_TABLE_HEAD,
        __UI_ENTRY_TABLE_HEADER, __UI_ENTRY_TABLE_ROW,
    },
    test_multi::{__UI_ENTRY_BAR_WIDGET, __UI_ENTRY_FOO_WIDGET, __UI_ENTRY_PAIR_WIDGET},
};

#[cfg(feature = "quickjs")]
pub const UI_COMPONENTS: &[EsEntry] = &[
    __UI_ENTRY_BADGE,
    __UI_ENTRY_CARD,
    __UI_ENTRY_I3_LAYOUT,
    __UI_ENTRY_ICON,
    __UI_ENTRY_CARD_HEADER,
    __UI_ENTRY_CARD_TITLE,
    __UI_ENTRY_CARD_DESCRIPTION,
    __UI_ENTRY_CARD_CONTENT,
    __UI_ENTRY_CARD_FOOTER,
    __UI_ENTRY_DATA_TABLE,
    __UI_ENTRY_KNOB_SHIM,
    __UI_ENTRY_PROGRESS,
    __UI_ENTRY_SLIDER_SHIM,
    __UI_ENTRY_TABLE,
    __UI_ENTRY_TABLE_HEADER,
    __UI_ENTRY_TABLE_BODY,
    __UI_ENTRY_TABLE_ROW,
    __UI_ENTRY_TABLE_HEAD,
    __UI_ENTRY_TABLE_CELL,
    __UI_ENTRY_FOO_WIDGET,
    __UI_ENTRY_BAR_WIDGET,
    __UI_ENTRY_PAIR_WIDGET,
];

#[cfg(feature = "quickjs")]
pub fn register_ui_components<'js>(ctx: &rquickjs::Ctx<'js>) -> rquickjs::Result<()> {
    for entry in UI_COMPONENTS {
        (entry.register)(ctx)?;
    }
    Ok(())
}

#[cfg(all(test, feature = "quickjs"))]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Components the browser does not get, and why.
    const WEB_EXEMPT: &[&str] = &[
        // Reads `ctx.screen_width`, emits `<panel>` shell nodes (ADR 0024).
        "__ui_i3_layout",
        // Fixtures for the multi-component-per-module path, not real components.
        "__ui_foo_widget",
        "__ui_bar_widget",
        "__ui_pair_widget",
    ];

    /// The two tables are hand-listed, so nothing but this stops a component being added
    /// to one and forgotten in the other.
    #[test]
    fn web_table_matches_quickjs_table() {
        let exempt: HashSet<&str> = WEB_EXEMPT.iter().copied().collect();
        let quickjs: Vec<(&str, &str, &str)> = UI_COMPONENTS
            .iter()
            .filter(|e| !exempt.contains(e.global_name))
            .map(|e| (e.module_path, e.export_name, e.global_name))
            .collect();
        let mut web: Vec<(&str, &str, &str)> = WEB_COMPONENTS
            .iter()
            .map(|c| (c.module_path, c.export_name, c.global_name))
            .collect();

        let mut quickjs_sorted = quickjs.clone();
        quickjs_sorted.sort_unstable();
        web.sort_unstable();

        assert_eq!(
            quickjs_sorted, web,
            "WEB_COMPONENTS and UI_COMPONENTS disagree; add the component to both, or \
             list its global in WEB_EXEMPT with the reason"
        );
    }

    /// A shim's global only exists once its JavaScript has run, so a shimmed component
    /// without `shim_js` would resolve to `undefined` in the page.
    #[test]
    fn shimmed_components_carry_their_shim_source() {
        for c in WEB_COMPONENTS {
            if c.global_name.starts_with("__tauler_") {
                assert!(
                    c.shim_js.is_some(),
                    "{} exports the shim global {} but carries no shim source",
                    c.export_name,
                    c.global_name
                );
            }
        }
    }
}
