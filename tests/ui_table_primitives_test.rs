mod common;

/// Helper that imports all six table primitives and renders the given JSX.
fn eval_table(inner_jsx: &str) -> serde_json::Value {
    let source = format!(
        "import {{ Table, TableHeader, TableBody, TableRow, TableHead, TableCell }} from '@ui/table';\
\nexport default function render() {{ return {inner_jsx}; }}"
    );
    common::eval_jsx(&source).layout
}

// ─── Table ───────────────────────────────────────────────────────────────────

#[test]
fn table_has_base_tw() {
    assert_eq!(eval_table("<Table />")["tw"], "flex flex-col w-full");
}

#[test]
fn table_renders_as_container() {
    assert_eq!(eval_table("<Table />")["type"], "container");
}

#[test]
fn table_passes_children_through() {
    let children = eval_table(r#"<Table><text tw="text-white">{"hello"}</text></Table>"#)
        ["children"]
        .as_array()
        .expect("children array")
        .clone();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0]["type"], "text");
    assert_eq!(children[0]["tw"], "text-white");
}

#[test]
fn table_merges_optional_tw_prop() {
    let tw = eval_table(r#"<Table tw="my-extra" />"#)["tw"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(tw.starts_with("flex flex-col w-full"), "base tw missing: {tw}");
    assert!(tw.contains("my-extra"), "extra tw not appended: {tw}");
}

// ─── TableHeader ─────────────────────────────────────────────────────────────

#[test]
fn table_header_tw_contains_border_border() {
    let tw = eval_table("<TableHeader />")["tw"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(tw.contains("border-border"), "border-border missing: {tw}");
}

#[test]
fn table_header_renders_as_container() {
    assert_eq!(eval_table("<TableHeader />")["type"], "container");
}

// ─── TableBody ───────────────────────────────────────────────────────────────

#[test]
fn table_body_has_base_tw() {
    assert_eq!(eval_table("<TableBody />")["tw"], "flex flex-col w-full");
}

#[test]
fn table_body_renders_as_container() {
    assert_eq!(eval_table("<TableBody />")["type"], "container");
}

// ─── TableRow ────────────────────────────────────────────────────────────────

const ROW_BASE_TW: &str = "flex flex-row gap-[8px] px-[8px] py-[4px] w-full";

#[test]
fn table_row_has_base_tw() {
    assert_eq!(eval_table("<TableRow />")["tw"], ROW_BASE_TW);
}

#[test]
fn table_row_renders_as_container() {
    assert_eq!(eval_table("<TableRow />")["type"], "container");
}

#[test]
fn table_row_merges_optional_tw_prop() {
    let tw = eval_table(r#"<TableRow tw="row-extra" />"#)["tw"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(tw.starts_with(ROW_BASE_TW), "base tw missing: {tw}");
    assert!(tw.contains("row-extra"), "extra tw not appended: {tw}");
}

#[test]
fn table_row_passes_children_through() {
    let children = eval_table(r#"<TableRow><text tw="cell-text">{"data"}</text></TableRow>"#)
        ["children"]
        .as_array()
        .expect("children array")
        .clone();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0]["type"], "text");
    assert_eq!(children[0]["tw"], "cell-text");
}

// ─── TableHead ───────────────────────────────────────────────────────────────

#[test]
fn table_head_tw_contains_flex_1() {
    let tw = eval_table("<TableHead />")["tw"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(tw.contains("flex-1"), "flex-1 missing: {tw}");
}

#[test]
fn table_head_tw_contains_text_muted_foreground() {
    let tw = eval_table("<TableHead />")["tw"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        tw.contains("text-muted-foreground"),
        "text-muted-foreground missing: {tw}"
    );
}

#[test]
fn table_head_tw_contains_uppercase() {
    let tw = eval_table("<TableHead />")["tw"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(tw.contains("uppercase"), "uppercase missing: {tw}");
}

#[test]
fn table_head_tw_contains_text_11px() {
    let tw = eval_table("<TableHead />")["tw"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(tw.contains("text-[11px]"), "text-[11px] missing: {tw}");
}

#[test]
fn table_head_renders_as_container() {
    assert_eq!(eval_table("<TableHead />")["type"], "container");
}

#[test]
fn table_head_merges_optional_tw_prop() {
    let tw = eval_table(r#"<TableHead tw="head-extra" />"#)["tw"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(tw.contains("flex-1"), "base tw (flex-1) missing: {tw}");
    assert!(tw.contains("head-extra"), "extra tw not appended: {tw}");
}

#[test]
fn table_head_passes_children_through() {
    let children = eval_table(r#"<TableHead><text tw="col-label">{"NAME"}</text></TableHead>"#)
        ["children"]
        .as_array()
        .expect("children array")
        .clone();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0]["type"], "text");
    assert_eq!(children[0]["tw"], "col-label");
}

// ─── TableCell ───────────────────────────────────────────────────────────────

#[test]
fn table_cell_tw_contains_flex_1() {
    let tw = eval_table("<TableCell />")["tw"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(tw.contains("flex-1"), "flex-1 missing: {tw}");
}

#[test]
fn table_cell_tw_contains_text_foreground() {
    let tw = eval_table("<TableCell />")["tw"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        tw.contains("text-foreground"),
        "text-foreground missing: {tw}"
    );
}

#[test]
fn table_cell_renders_as_container() {
    assert_eq!(eval_table("<TableCell />")["type"], "container");
}

#[test]
fn table_cell_merges_optional_tw_prop() {
    let tw = eval_table(r#"<TableCell tw="cell-extra" />"#)["tw"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(tw.contains("flex-1"), "base tw (flex-1) missing: {tw}");
    assert!(tw.contains("cell-extra"), "extra tw not appended: {tw}");
}

#[test]
fn table_cell_passes_children_through() {
    let children =
        eval_table(r#"<TableCell><text tw="cell-val">{"42"}</text></TableCell>"#)["children"]
            .as_array()
            .expect("children array")
            .clone();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0]["type"], "text");
    assert_eq!(children[0]["tw"], "cell-val");
}
