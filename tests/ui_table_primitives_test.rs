mod common;

fn eval_table(inner_jsx: &str) -> serde_json::Value {
    let source = format!(
        "import {{ Table, TableHeader, TableBody, TableRow, TableHead, TableCell }} from '@ui/table';\
\nexport default function render() {{ return {inner_jsx}; }}"
    );
    common::eval_jsx(&source).layout
}

mod table {
    use super::eval_table;

    fn node() -> serde_json::Value { eval_table("<Table />") }

    #[test]
    fn has_base_tw() { assert_eq!(node()["tw"], "flex flex-col w-full"); }

    #[test]
    fn renders_as_container() { assert_eq!(node()["type"], "container"); }

    #[test]
    fn passes_children_through() {
        let children = eval_table(r#"<Table><text tw="text-white">{"hello"}</text></Table>"#)
            ["children"].as_array().expect("children array").clone();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0]["type"], "text");
        assert_eq!(children[0]["tw"], "text-white");
    }

    #[test]
    fn merges_optional_tw_prop() {
        let tw = eval_table(r#"<Table tw="my-extra" />"#)["tw"].as_str().unwrap().to_string();
        assert!(tw.starts_with("flex flex-col w-full"), "base tw missing: {tw}");
        assert!(tw.contains("my-extra"), "extra tw not appended: {tw}");
    }
}

mod table_header {
    use super::eval_table;

    fn tw() -> String { eval_table("<TableHeader />")["tw"].as_str().unwrap().to_string() }

    #[test]
    fn tw_contains_border_border() { let tw = tw(); assert!(tw.contains("border-border"), "border-border missing: {tw}"); }

    #[test]
    fn tw_contains_flex_col() { let tw = tw(); assert!(tw.contains("flex-col"), "flex-col missing: {tw}"); }

    #[test]
    fn renders_as_container() { assert_eq!(eval_table("<TableHeader />")["type"], "container"); }
}

mod table_body {
    use super::eval_table;

    #[test]
    fn has_base_tw() { assert_eq!(eval_table("<TableBody />")["tw"], "flex flex-col w-full"); }

    #[test]
    fn renders_as_container() { assert_eq!(eval_table("<TableBody />")["type"], "container"); }
}

mod table_row {
    use super::eval_table;

    const BASE_TW: &str =
        "flex flex-row w-full border border-t-0 border-r-0 border-l-0 border-border";

    #[test]
    fn has_base_tw() { assert_eq!(eval_table("<TableRow />")["tw"], BASE_TW); }

    #[test]
    fn renders_as_container() { assert_eq!(eval_table("<TableRow />")["type"], "container"); }

    #[test]
    fn merges_optional_tw_prop() {
        let tw = eval_table(r#"<TableRow tw="row-extra" />"#)["tw"].as_str().unwrap().to_string();
        assert!(tw.starts_with(BASE_TW), "base tw missing: {tw}");
        assert!(tw.contains("row-extra"), "extra tw not appended: {tw}");
    }

    #[test]
    fn passes_children_through() {
        let children =
            eval_table(r#"<TableRow><text tw="cell-text">{"data"}</text></TableRow>"#)["children"]
                .as_array().expect("children array").clone();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0]["type"], "text");
        assert_eq!(children[0]["tw"], "cell-text");
    }
}

mod table_head {
    use super::eval_table;

    fn tw() -> String { eval_table("<TableHead />")["tw"].as_str().unwrap().to_string() }

    #[test]
    fn tw_contains_flex_1() { let tw = tw(); assert!(tw.contains("flex-1"), "flex-1 missing: {tw}"); }

    #[test]
    fn tw_contains_text_muted_foreground() { let tw = tw(); assert!(tw.contains("text-muted-foreground"), "text-muted-foreground missing: {tw}"); }

    #[test]
    fn tw_contains_font_medium() { let tw = tw(); assert!(tw.contains("font-medium"), "font-medium missing: {tw}"); }

    #[test]
    fn tw_contains_py_4px() { let tw = tw(); assert!(tw.contains("py-[4px]"), "py-[4px] missing: {tw}"); }

    #[test]
    fn renders_as_container() { assert_eq!(eval_table("<TableHead />")["type"], "container"); }

    #[test]
    fn merges_optional_tw_prop() {
        let tw = eval_table(r#"<TableHead tw="head-extra" />"#)["tw"].as_str().unwrap().to_string();
        assert!(tw.contains("flex-1"), "base tw (flex-1) missing: {tw}");
        assert!(tw.contains("head-extra"), "extra tw not appended: {tw}");
    }

    #[test]
    fn passes_children_through() {
        let children =
            eval_table(r#"<TableHead><text tw="col-label">{"NAME"}</text></TableHead>"#)["children"]
                .as_array().expect("children array").clone();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0]["type"], "text");
        assert_eq!(children[0]["tw"], "col-label");
    }
}

mod table_cell {
    use super::eval_table;

    fn tw() -> String { eval_table("<TableCell />")["tw"].as_str().unwrap().to_string() }

    #[test]
    fn tw_contains_flex_1() { let tw = tw(); assert!(tw.contains("flex-1"), "flex-1 missing: {tw}"); }

    #[test]
    fn tw_contains_text_foreground() { let tw = tw(); assert!(tw.contains("text-foreground"), "text-foreground missing: {tw}"); }

    #[test]
    fn tw_contains_px_4px() { let tw = tw(); assert!(tw.contains("px-[4px]"), "px-[4px] missing: {tw}"); }

    #[test]
    fn tw_contains_py_4px() { let tw = tw(); assert!(tw.contains("py-[4px]"), "py-[4px] missing: {tw}"); }

    #[test]
    fn renders_as_container() { assert_eq!(eval_table("<TableCell />")["type"], "container"); }

    #[test]
    fn merges_optional_tw_prop() {
        let tw = eval_table(r#"<TableCell tw="cell-extra" />"#)["tw"].as_str().unwrap().to_string();
        assert!(tw.contains("flex-1"), "base tw (flex-1) missing: {tw}");
        assert!(tw.contains("cell-extra"), "extra tw not appended: {tw}");
    }

    #[test]
    fn passes_children_through() {
        let children =
            eval_table(r#"<TableCell><text tw="cell-val">{"42"}</text></TableCell>"#)["children"]
                .as_array().expect("children array").clone();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0]["type"], "text");
        assert_eq!(children[0]["tw"], "cell-val");
    }
}
