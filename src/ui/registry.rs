use crate::ui::components::{
    card::__UI_ENTRY_CARD,
    table::datatable::__UI_ENTRY_DATA_TABLE,
    progress::__UI_ENTRY_PROGRESS,
    table::{
        __UI_ENTRY_TABLE,
        __UI_ENTRY_TABLE_HEADER,
        __UI_ENTRY_TABLE_BODY,
        __UI_ENTRY_TABLE_ROW,
        __UI_ENTRY_TABLE_HEAD,
        __UI_ENTRY_TABLE_CELL,
    },
    test_multi::{__UI_ENTRY_FOO_WIDGET, __UI_ENTRY_BAR_WIDGET},
};

pub struct UiEntry {
    pub module_path: &'static str,
    pub export_name: &'static str,
    pub global_name: &'static str,
    pub register: fn(&rquickjs::Ctx<'_>) -> rquickjs::Result<()>,
}

pub const UI_COMPONENTS: &[UiEntry] = &[
    __UI_ENTRY_CARD,
    __UI_ENTRY_DATA_TABLE,
    __UI_ENTRY_PROGRESS,
    __UI_ENTRY_TABLE,
    __UI_ENTRY_TABLE_HEADER,
    __UI_ENTRY_TABLE_BODY,
    __UI_ENTRY_TABLE_ROW,
    __UI_ENTRY_TABLE_HEAD,
    __UI_ENTRY_TABLE_CELL,
    __UI_ENTRY_FOO_WIDGET,
    __UI_ENTRY_BAR_WIDGET,
];

pub fn synthetic_module_source(entry: &UiEntry) -> String {
    format!(
        "const {export} = {global}; export {{ {export} }};",
        export = entry.export_name,
        global = entry.global_name,
    )
}

pub fn synthetic_module_source_for_entries(entries: &[&UiEntry]) -> String {
    let bindings: Vec<String> = entries
        .iter()
        .map(|e| format!("const {} = {};", e.export_name, e.global_name))
        .collect();
    let exports: Vec<&str> = entries.iter().map(|e| e.export_name).collect();
    format!("{} export {{ {} }};", bindings.join(" "), exports.join(", "))
}

pub fn register_ui_components<'js>(ctx: &rquickjs::Ctx<'js>) -> rquickjs::Result<()> {
    for entry in UI_COMPONENTS {
        (entry.register)(ctx)?;
    }
    Ok(())
}
