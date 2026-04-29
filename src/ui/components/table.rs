use serde::Deserialize;
use crate::ui::{Node, component, rsx};

/// A column definition for the Table component.
#[derive(Deserialize)]
pub struct ColumnDef {
    pub key: String,
    pub label: String,
    pub width: Option<u32>,
}

/// A structured data grid. Renders a header row (uppercase, muted, with a bottom
/// border) followed by data rows with alternating `bg-card` / `bg-muted/30`
/// backgrounds. Each column definition maps a `key` (used to look up row values)
/// to a `label` (shown in the header). An optional `width` constrains the column.
///
/// # JSX
/// ```jsx
/// <Table
///   columns={[{key:"service", label:"SERVICE"}, {key:"status", label:"STATUS"}, {key:"uptime", label:"UPTIME"}]}
///   rows={[
///     {service:"nginx", status:"running", uptime:"14d"},
///     {service:"postgres", status:"running", uptime:"7d"},
///     {service:"redis", status:"stopped", uptime:"—"},
///   ]}
/// />
/// ```
///
/// # Shadcn
/// https://ui.shadcn.com/docs/components/table
#[component("@ui/table")]
pub fn table(columns: Vec<ColumnDef>, rows: Option<serde_json::Value>) -> Node {
    let header_children: Vec<Node> = columns
        .iter()
        .map(|col| rsx! { <text tw="flex-1">{col.label.clone()}</text> })
        .collect();
    let row_nodes: Vec<Node> = if let Some(serde_json::Value::Array(arr)) = rows {
        arr.into_iter()
            .enumerate()
            .map(|(index, row)| {
                let cells: Vec<Node> = columns
                    .iter()
                    .map(|col| {
                        let val = row.get(&col.key)
                            .and_then(|v| match v {
                                serde_json::Value::String(s) => Some(s.clone()),
                                serde_json::Value::Number(n) => Some(n.to_string()),
                                _ => None,
                            })
                            .unwrap_or_default();
                        rsx! { <text tw="flex-1">{val}</text> }
                    })
                    .collect();
                let tw = if index % 2 == 0 {
                    "text-foreground bg-card flex flex-row gap-[8px] px-[8px] py-[4px] w-full"
                } else {
                    "text-foreground bg-muted/30 flex flex-row gap-[8px] px-[8px] py-[4px] w-full"
                };
                rsx! {
                    <container tw={tw}>
                        {cells}
                    </container>
                }
            })
            .collect()
    } else {
        vec![]
    };
    rsx! {
        <container tw="flex flex-col w-full">
            <container tw="flex flex-row gap-[8px] px-[8px] py-[4px] text-muted-foreground border-border uppercase">
                {header_children}
            </container>
            {row_nodes}
        </container>
    }
}
