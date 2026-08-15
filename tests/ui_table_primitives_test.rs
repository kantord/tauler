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

    fn node() -> serde_json::Value {
        eval_table("<Table />")
    }

    #[test]
    fn has_base_tw() {
        assert_eq!(node()["class"], "flex flex-col w-full");
    }

    #[test]
    fn renders_as_container() {
        assert_eq!(node()["type"], "div");
    }

    #[test]
    fn passes_children_through() {
        let children = eval_table(r#"<Table><span class="text-white">{"hello"}</span></Table>"#)
            ["children"]
            .as_array()
            .expect("children array")
            .clone();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0]["type"], "span");
        assert_eq!(children[0]["class"], "text-white");
    }

    #[test]
    fn merges_optional_tw_prop() {
        let tw = eval_table(r#"<Table class="my-extra" />"#)["class"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(
            tw.starts_with("flex flex-col w-full"),
            "base tw missing: {tw}"
        );
        assert!(tw.contains("my-extra"), "extra tw not appended: {tw}");
    }
}

mod table_header {
    use super::eval_table;

    fn tw() -> String {
        eval_table("<TableHeader />")["class"]
            .as_str()
            .unwrap()
            .to_string()
    }

    #[test]
    fn tw_contains_border_border() {
        let tw = tw();
        assert!(tw.contains("border-border"), "border-border missing: {tw}");
    }

    #[test]
    fn tw_contains_flex_col() {
        let tw = tw();
        assert!(tw.contains("flex-col"), "flex-col missing: {tw}");
    }

    #[test]
    fn renders_as_container() {
        assert_eq!(eval_table("<TableHeader />")["type"], "div");
    }
}

mod table_body {
    use super::eval_table;

    #[test]
    fn has_base_tw() {
        assert_eq!(eval_table("<TableBody />")["class"], "flex flex-col w-full");
    }

    #[test]
    fn renders_as_container() {
        assert_eq!(eval_table("<TableBody />")["type"], "div");
    }
}

mod table_row {
    use super::eval_table;

    const BASE_TW: &str =
        "flex flex-row w-full border border-t-0 border-r-0 border-l-0 border-border";

    #[test]
    fn has_base_tw() {
        assert_eq!(eval_table("<TableRow />")["class"], BASE_TW);
    }

    #[test]
    fn renders_as_container() {
        assert_eq!(eval_table("<TableRow />")["type"], "div");
    }

    #[test]
    fn merges_optional_tw_prop() {
        let tw = eval_table(r#"<TableRow class="row-extra" />"#)["class"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(tw.starts_with(BASE_TW), "base tw missing: {tw}");
        assert!(tw.contains("row-extra"), "extra tw not appended: {tw}");
    }

    #[test]
    fn passes_children_through() {
        let children =
            eval_table(r#"<TableRow><span class="cell-text">{"data"}</span></TableRow>"#)
                ["children"]
                .as_array()
                .expect("children array")
                .clone();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0]["type"], "span");
        assert_eq!(children[0]["class"], "cell-text");
    }
}

mod table_head {
    use super::eval_table;

    fn tw() -> String {
        eval_table("<TableHead />")["class"]
            .as_str()
            .unwrap()
            .to_string()
    }

    #[test]
    fn tw_contains_flex_1() {
        let tw = tw();
        assert!(tw.contains("flex-1"), "flex-1 missing: {tw}");
    }

    #[test]
    fn tw_contains_text_muted_foreground() {
        let tw = tw();
        assert!(
            tw.contains("text-muted-foreground"),
            "text-muted-foreground missing: {tw}"
        );
    }

    #[test]
    fn tw_contains_font_medium() {
        let tw = tw();
        assert!(tw.contains("font-medium"), "font-medium missing: {tw}");
    }

    #[test]
    fn tw_contains_py_4px() {
        let tw = tw();
        assert!(tw.contains("py-[4px]"), "py-[4px] missing: {tw}");
    }

    #[test]
    fn renders_as_container() {
        assert_eq!(eval_table("<TableHead />")["type"], "div");
    }

    #[test]
    fn merges_optional_tw_prop() {
        let tw = eval_table(r#"<TableHead class="head-extra" />"#)["class"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(tw.contains("flex-1"), "base tw (flex-1) missing: {tw}");
        assert!(tw.contains("head-extra"), "extra tw not appended: {tw}");
    }

    #[test]
    fn passes_children_through() {
        let children =
            eval_table(r#"<TableHead><span class="col-label">{"NAME"}</span></TableHead>"#)
                ["children"]
                .as_array()
                .expect("children array")
                .clone();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0]["type"], "span");
        assert_eq!(children[0]["class"], "col-label");
    }
}

mod table_cell {
    use super::eval_table;

    fn tw() -> String {
        eval_table("<TableCell />")["class"]
            .as_str()
            .unwrap()
            .to_string()
    }

    #[test]
    fn tw_contains_flex_1() {
        let tw = tw();
        assert!(tw.contains("flex-1"), "flex-1 missing: {tw}");
    }

    #[test]
    fn tw_contains_text_foreground() {
        let tw = tw();
        assert!(
            tw.contains("text-foreground"),
            "text-foreground missing: {tw}"
        );
    }

    #[test]
    fn tw_contains_px_4px() {
        let tw = tw();
        assert!(tw.contains("px-[4px]"), "px-[4px] missing: {tw}");
    }

    #[test]
    fn tw_contains_py_4px() {
        let tw = tw();
        assert!(tw.contains("py-[4px]"), "py-[4px] missing: {tw}");
    }

    #[test]
    fn renders_as_container() {
        assert_eq!(eval_table("<TableCell />")["type"], "div");
    }

    #[test]
    fn merges_optional_tw_prop() {
        let tw = eval_table(r#"<TableCell class="cell-extra" />"#)["class"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(tw.contains("flex-1"), "base tw (flex-1) missing: {tw}");
        assert!(tw.contains("cell-extra"), "extra tw not appended: {tw}");
    }

    #[test]
    fn passes_children_through() {
        let children = eval_table(r#"<TableCell><span class="cell-val">{"42"}</span></TableCell>"#)
            ["children"]
            .as_array()
            .expect("children array")
            .clone();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0]["type"], "span");
        assert_eq!(children[0]["class"], "cell-val");
    }
}
