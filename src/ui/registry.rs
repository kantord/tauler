use crate::ui::components::{card::card, progress::progress};

pub struct UiEntry {
    pub module_path: &'static str,
    pub export_name: &'static str,
    pub global_name: &'static str,
}

pub const UI_COMPONENTS: &[UiEntry] = &[
    UiEntry { module_path: "@ui/card", export_name: "Card", global_name: "__ui_card" },
    UiEntry { module_path: "@ui/progress", export_name: "Progress", global_name: "__ui_progress" },
];

pub fn synthetic_module_source(entry: &UiEntry) -> String {
    format!(
        "const {export} = {global}; export {{ {export} }};",
        export = entry.export_name,
        global = entry.global_name,
    )
}

pub fn register_ui_components<'js>(ctx: &rquickjs::Ctx<'js>) -> rquickjs::Result<()> {
    ctx.globals().set("__ui_card", rquickjs::Function::new(ctx.clone(), card)?)?;
    ctx.globals().set("__ui_progress", rquickjs::Function::new(ctx.clone(), progress)?)?;
    Ok(())
}
