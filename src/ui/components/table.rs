use crate::ui::{Node, component, rsx, tw_merge};

const BASE_TW: &str = "flex flex-col w-full";
const HEADER_BASE_TW: &str = "border-border";
const BODY_BASE_TW: &str = "flex flex-col w-full";
const ROW_BASE_TW: &str = "flex flex-row gap-[8px] px-[8px] py-[4px] w-full";
const HEAD_BASE_TW: &str = "flex-1 text-muted-foreground uppercase text-[11px]";
const CELL_BASE_TW: &str = "flex-1 text-foreground";

/// Composable table primitives. Use these to build fully custom table layouts.
/// For a data-driven table, use `DataTable` from `@ui/datatable` instead.
///
/// # JSX
/// ```jsx
/// <Table>
///   <TableHeader>
///     <TableRow>
///       <TableHead><text>SERVICE</text></TableHead>
///       <TableHead><text>STATUS</text></TableHead>
///       <TableHead><text>UPTIME</text></TableHead>
///     </TableRow>
///   </TableHeader>
///   <TableBody>
///     <TableRow>
///       <TableCell><text>nginx</text></TableCell>
///       <TableCell tw="text-green-500"><text>running</text></TableCell>
///       <TableCell><text>14d</text></TableCell>
///     </TableRow>
///   </TableBody>
/// </Table>
/// ```
///
/// # Shadcn
/// https://ui.shadcn.com/docs/components/table
#[component("@ui/table")]
pub fn table(children: Vec<Node>, tw: Option<String>) -> Node {
    let tw = tw_merge(BASE_TW, tw.as_deref().unwrap_or(""));
    rsx! {
        <container tw={tw}>
            {children}
        </container>
    }
}

#[component("@ui/table")]
pub fn table_header(children: Vec<Node>) -> Node {
    rsx! {
        <container tw={HEADER_BASE_TW}>
            {children}
        </container>
    }
}

#[component("@ui/table")]
pub fn table_body(children: Vec<Node>) -> Node {
    rsx! {
        <container tw={BODY_BASE_TW}>
            {children}
        </container>
    }
}

#[component("@ui/table")]
pub fn table_row(children: Vec<Node>, tw: Option<String>) -> Node {
    let tw = tw_merge(ROW_BASE_TW, tw.as_deref().unwrap_or(""));
    rsx! {
        <container tw={tw}>
            {children}
        </container>
    }
}

#[component("@ui/table")]
pub fn table_head(children: Vec<Node>, tw: Option<String>) -> Node {
    let tw = tw_merge(HEAD_BASE_TW, tw.as_deref().unwrap_or(""));
    rsx! {
        <container tw={tw}>
            {children}
        </container>
    }
}

#[component("@ui/table")]
pub fn table_cell(children: Vec<Node>, tw: Option<String>) -> Node {
    let tw = tw_merge(CELL_BASE_TW, tw.as_deref().unwrap_or(""));
    rsx! {
        <container tw={tw}>
            {children}
        </container>
    }
}
