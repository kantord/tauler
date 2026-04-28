use crate::ui::components::card::card;

pub struct UiEntry {
    pub module_path: &'static str,
    pub export_name: &'static str,
    pub global_name: &'static str,
}

pub const UI_COMPONENTS: &[UiEntry] = &[
    UiEntry {
        module_path: "@ui/card",
        export_name: "Card",
        global_name: "__ui_card",
    },
];

pub fn synthetic_module_source(entry: &UiEntry) -> String {
    format!(
        "const {export} = {global}; export {{ {export} }};",
        export = entry.export_name,
        global = entry.global_name,
    )
}

pub fn register_ui_components<'js>(ctx: &rquickjs::Ctx<'js>) -> rquickjs::Result<()> {
    let f = rquickjs::Function::new(ctx.clone(), card)?;
    ctx.globals().set("__ui_card", f)?;
    Ok(())
}
