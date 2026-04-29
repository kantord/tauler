use crate::ui::components::{card::__UI_ENTRY_CARD, progress::__UI_ENTRY_PROGRESS};

pub struct UiEntry {
    pub module_path: &'static str,
    pub export_name: &'static str,
    pub global_name: &'static str,
    pub register: fn(&rquickjs::Ctx<'_>) -> rquickjs::Result<()>,
}

pub const UI_COMPONENTS: &[UiEntry] = &[
    __UI_ENTRY_CARD,
    __UI_ENTRY_PROGRESS,
];

pub fn synthetic_module_source(entry: &UiEntry) -> String {
    format!(
        "const {export} = {global}; export {{ {export} }};",
        export = entry.export_name,
        global = entry.global_name,
    )
}

pub fn register_ui_components<'js>(ctx: &rquickjs::Ctx<'js>) -> rquickjs::Result<()> {
    for entry in UI_COMPONENTS {
        (entry.register)(ctx)?;
    }
    Ok(())
}
