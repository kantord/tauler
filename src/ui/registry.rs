pub use optative_script::EsEntry;

use crate::ui::components::{
    badge::__UI_ENTRY_BADGE,
    card::{
        __UI_ENTRY_CARD, __UI_ENTRY_CARD_CONTENT, __UI_ENTRY_CARD_DESCRIPTION,
        __UI_ENTRY_CARD_FOOTER, __UI_ENTRY_CARD_HEADER, __UI_ENTRY_CARD_TITLE,
    },
    icon::__UI_ENTRY_ICON,
    progress::__UI_ENTRY_PROGRESS,
    table::datatable::__UI_ENTRY_DATA_TABLE,
    table::{
        __UI_ENTRY_TABLE, __UI_ENTRY_TABLE_BODY, __UI_ENTRY_TABLE_CELL, __UI_ENTRY_TABLE_HEAD,
        __UI_ENTRY_TABLE_HEADER, __UI_ENTRY_TABLE_ROW,
    },
    test_multi::{__UI_ENTRY_BAR_WIDGET, __UI_ENTRY_FOO_WIDGET},
};

pub const UI_COMPONENTS: &[EsEntry] = &[
    __UI_ENTRY_BADGE,
    __UI_ENTRY_CARD,
    __UI_ENTRY_ICON,
    __UI_ENTRY_CARD_HEADER,
    __UI_ENTRY_CARD_TITLE,
    __UI_ENTRY_CARD_DESCRIPTION,
    __UI_ENTRY_CARD_CONTENT,
    __UI_ENTRY_CARD_FOOTER,
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

pub fn register_ui_components<'js>(ctx: &rquickjs::Ctx<'js>) -> rquickjs::Result<()> {
    for entry in UI_COMPONENTS {
        (entry.register)(ctx)?;
    }
    Ok(())
}
