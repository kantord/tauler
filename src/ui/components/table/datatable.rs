use serde::Deserialize;
use crate::ui::{Node, UiComponent, component, rsx};
use super::{Table, TableProps, TableRow, TableRowProps};

/// A column definition for DataTable.
#[derive(Deserialize)]
pub struct ColumnDef {
    pub key: String,
    pub label: String,
    pub width: Option<u32>,
}

fn header_row(columns: &[ColumnDef]) -> Node {
    let cells: Vec<Node> = columns
        .iter()
        .map(|col| rsx! { <text tw="flex-1">{col.label.clone()}</text> })
        .collect();
    TableRow::render(TableRowProps {
        children: cells,
        tw: Some("text-muted-foreground border-border uppercase".to_string()),
    })
}

fn cell_value(row: &serde_json::Value, key: &str) -> String {
    row.get(key)
        .and_then(|v| match v {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
        .unwrap_or_default()
}

fn data_row(columns: &[ColumnDef], row: &serde_json::Value, index: usize) -> Node {
    let cells: Vec<Node> = columns
        .iter()
        .map(|col| rsx! { <text tw="flex-1">{cell_value(row, &col.key)}</text> })
        .collect();
    let bg = if index % 2 == 0 { "bg-card text-foreground" } else { "bg-muted/30 text-foreground" };
    TableRow::render(TableRowProps { children: cells, tw: Some(bg.to_string()) })
}

/// A data-driven table. Renders a header row followed by data rows with
/// alternating `bg-card` / `bg-muted/30` backgrounds. Columns map a `key`
/// (used to look up values in each row object) to a `label` (shown in the
/// header). An optional `width` constrains the column.
///
/// For full compositional control, use the `Table`, `TableHeader`, `TableBody`,
/// `TableRow`, `TableHead`, and `TableCell` primitives from `@ui/table` instead.
///
/// # JSX
/// ```jsx
/// <DataTable
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
#[component("@ui/datatable")]
pub fn data_table(columns: Vec<ColumnDef>, rows: Option<serde_json::Value>) -> Node {
    let mut all_children = vec![header_row(&columns)];
    if let Some(serde_json::Value::Array(arr)) = rows {
        for (index, row) in arr.into_iter().enumerate() {
            all_children.push(data_row(&columns, &row, index));
        }
    }
    Table::render(TableProps { children: all_children, tw: None })
}
