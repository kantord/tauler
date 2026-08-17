pub mod datatable;

use crate::ui::{component, cva::Cva, rsx, Node};

const TABLE_VARIANTS: Cva = Cva {
    base: "flex flex-col w-full",
    variants: &[],
    defaults: &[],
};
const TABLE_HEADER_VARIANTS: Cva = Cva {
    base: "flex flex-col w-full border-border",
    variants: &[],
    defaults: &[],
};
const TABLE_BODY_VARIANTS: Cva = Cva {
    base: "flex flex-col w-full",
    variants: &[],
    defaults: &[],
};
const TABLE_ROW_VARIANTS: Cva = Cva {
    base: "flex flex-row w-full border border-t-0 border-r-0 border-l-0 border-border",
    variants: &[],
    defaults: &[],
};
const TABLE_HEAD_VARIANTS: Cva = Cva {
    base: "flex-1 px-[4px] py-[4px] font-medium text-muted-foreground",
    variants: &[],
    defaults: &[],
};
const TABLE_CELL_VARIANTS: Cva = Cva {
    base: "flex-1 px-[4px] py-[4px] text-foreground",
    variants: &[],
    defaults: &[],
};

/// Composable table primitives. Use these to build fully custom table layouts.
/// For a data-driven table, use `DataTable` from `@ui/datatable` instead.
///
/// # JSX
/// ```jsx
/// <Table>
///   <TableHeader>
///     <TableRow>
///       <TableHead><span>SERVICE</span></TableHead>
///       <TableHead><span>STATUS</span></TableHead>
///       <TableHead><span>UPTIME</span></TableHead>
///     </TableRow>
///   </TableHeader>
///   <TableBody>
///     <TableRow>
///       <TableCell><span>nginx</span></TableCell>
///       <TableCell class="text-green-500"><span>running</span></TableCell>
///       <TableCell><span>14d</span></TableCell>
///     </TableRow>
///   </TableBody>
/// </Table>
/// ```
///
/// # Shadcn
/// https://ui.shadcn.com/docs/components/table
#[component("@ui/table")]
pub fn table(children: Vec<Node>, class: Option<String>) -> Node {
    let class = TABLE_VARIANTS.resolve(&[], class.as_deref().unwrap_or(""));
    rsx! { <div class={class}>{children}</div> }
}

#[component("@ui/table")]
pub fn table_header(children: Vec<Node>) -> Node {
    let class = TABLE_HEADER_VARIANTS.resolve(&[], "");
    rsx! { <div class={class}>{children}</div> }
}

#[component("@ui/table")]
pub fn table_body(children: Vec<Node>) -> Node {
    let class = TABLE_BODY_VARIANTS.resolve(&[], "");
    rsx! { <div class={class}>{children}</div> }
}

#[component("@ui/table")]
pub fn table_row(children: Vec<Node>, class: Option<String>) -> Node {
    let class = TABLE_ROW_VARIANTS.resolve(&[], class.as_deref().unwrap_or(""));
    rsx! { <div class={class}>{children}</div> }
}

#[component("@ui/table")]
pub fn table_head(children: Vec<Node>, class: Option<String>) -> Node {
    let class = TABLE_HEAD_VARIANTS.resolve(&[], class.as_deref().unwrap_or(""));
    rsx! { <div class={class}>{children}</div> }
}

#[component("@ui/table")]
pub fn table_cell(children: Vec<Node>, class: Option<String>) -> Node {
    let class = TABLE_CELL_VARIANTS.resolve(&[], class.as_deref().unwrap_or(""));
    rsx! { <div class={class}>{children}</div> }
}
