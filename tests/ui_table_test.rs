mod common;

fn eval_table(inner_jsx: &str) -> serde_json::Value {
    let source = format!(
        "import {{ DataTable }} from '@ui/datatable';\nexport default function render() {{ return {inner_jsx}; }}"
    );
    common::eval_jsx(&source).layout
}

// --- header labels ---

/// The header row must contain a text node for each column label.
#[test]
fn table_header_contains_column_labels() {
    let node = eval_table(
        r#"<DataTable columns={[{key:"repo", label:"REPO"}, {key:"prs", label:"PR", width:24}]} rows={[]} />"#,
    );
    // Collect all text values from the node tree recursively.
    fn collect_texts(node: &serde_json::Value, out: &mut Vec<String>) {
        if node["type"] == "text" {
            if let Some(t) = node["text"].as_str() {
                out.push(t.to_string());
            }
        }
        if let Some(children) = node["children"].as_array() {
            for child in children {
                collect_texts(child, out);
            }
        }
    }
    let mut texts = Vec::new();
    collect_texts(&node, &mut texts);
    assert!(
        texts.iter().any(|t| t == "REPO"),
        "expected label 'REPO' in rendered tree; got: {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t == "PR"),
        "expected label 'PR' in rendered tree; got: {texts:?}"
    );
}

/// The header container must carry the `text-muted-foreground` Tailwind class.
#[test]
fn table_header_has_text_muted_foreground_tw() {
    let node = eval_table(
        r#"<DataTable columns={[{key:"repo", label:"REPO"}, {key:"prs", label:"PR", width:24}]} rows={[]} />"#,
    );
    // The header is expected to be the first child of the root Table container.
    let header = &node["children"][0];
    let header_tw = header["tw"].as_str().unwrap_or("");
    assert!(
        header_tw.contains("text-muted-foreground"),
        "expected 'text-muted-foreground' in header tw; got: '{header_tw}'"
    );
}

// --- rows rendering ---

/// Helper: collect all text node values from a tree recursively.
fn collect_texts(node: &serde_json::Value, out: &mut Vec<String>) {
    if node["type"] == "text" {
        if let Some(t) = node["text"].as_str() {
            out.push(t.to_string());
        }
    }
    if let Some(children) = node["children"].as_array() {
        for child in children {
            collect_texts(child, out);
        }
    }
}

/// Helper: collect all `tw` values from container nodes whose tw contains the given class.
fn collect_containers_with_tw(node: &serde_json::Value, class: &str, out: &mut Vec<String>) {
    if node["type"] != "text" {
        if let Some(tw) = node["tw"].as_str() {
            if tw.contains(class) {
                out.push(tw.to_string());
            }
        }
    }
    if let Some(children) = node["children"].as_array() {
        for child in children {
            collect_containers_with_tw(child, class, out);
        }
    }
}

/// Table renders a text node for each entry in `rows`.
#[test]
fn table_rows_render_cell_text_values() {
    let node = eval_table(
        r#"<DataTable
            columns={[{key:"repo", label:"REPO"}, {key:"prs", label:"PR", width:24}]}
            rows={[{repo:"costae", prs:3}, {repo:"dotfiles", prs:1}]}
        />"#,
    );
    let mut texts = Vec::new();
    collect_texts(&node, &mut texts);
    assert!(
        texts.iter().any(|t| t == "costae"),
        "expected text node 'costae' in rendered tree; got: {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t == "dotfiles"),
        "expected text node 'dotfiles' in rendered tree; got: {texts:?}"
    );
}

/// Each row container carries `text-foreground` in its tw class.
#[test]
fn table_row_containers_have_text_foreground_tw() {
    let node = eval_table(
        r#"<DataTable
            columns={[{key:"repo", label:"REPO"}, {key:"prs", label:"PR", width:24}]}
            rows={[{repo:"costae", prs:3}, {repo:"dotfiles", prs:1}]}
        />"#,
    );
    let mut row_containers = Vec::new();
    collect_containers_with_tw(&node, "text-foreground", &mut row_containers);
    assert_eq!(
        row_containers.len(),
        2,
        "expected exactly 2 row containers with 'text-foreground' tw; got: {row_containers:?}"
    );
}

// --- header styling ---

/// The header container must carry the `border-border` Tailwind class.
#[test]
fn table_header_has_border_border_tw() {
    let node = eval_table(r#"<DataTable columns={[{key:"repo", label:"REPO"}]} rows={[]} />"#);
    let header = &node["children"][0];
    let header_tw = header["tw"].as_str().unwrap_or("");
    assert!(
        header_tw.contains("border-border"),
        "expected 'border-border' in header tw; got: '{header_tw}'"
    );
}

/// The header container must carry the `uppercase` Tailwind class.
#[test]
fn table_header_has_uppercase_tw() {
    let node = eval_table(r#"<DataTable columns={[{key:"repo", label:"REPO"}]} rows={[]} />"#);
    let header = &node["children"][0];
    let header_tw = header["tw"].as_str().unwrap_or("");
    assert!(
        header_tw.contains("uppercase"),
        "expected 'uppercase' in header tw; got: '{header_tw}'"
    );
}

// --- alternating row backgrounds ---

/// Helper: collect the `tw` value of each direct row child (children[1..]) of the root container.
fn collect_row_tws(node: &serde_json::Value) -> Vec<String> {
    let children = match node["children"].as_array() {
        Some(c) => c,
        None => return vec![],
    };
    // children[0] is the header; rows start at index 1
    children
        .iter()
        .skip(1)
        .map(|row| row["tw"].as_str().unwrap_or("").to_string())
        .collect()
}

/// Even-indexed rows (0, 2, …) carry `bg-card` in their tw class.
#[test]
fn table_even_rows_have_bg_card_tw() {
    let node = eval_table(
        r#"<DataTable
            columns={[{key:"name", label:"NAME"}]}
            rows={[{name:"a"}, {name:"b"}, {name:"c"}]}
        />"#,
    );
    let tws = collect_row_tws(&node);
    assert!(
        tws.len() >= 3,
        "expected at least 3 row containers; got: {tws:?}"
    );
    assert!(
        tws[0].contains("bg-card"),
        "expected row 0 tw to contain 'bg-card'; got: '{}'",
        tws[0]
    );
    assert!(
        tws[2].contains("bg-card"),
        "expected row 2 tw to contain 'bg-card'; got: '{}'",
        tws[2]
    );
}

/// Odd-indexed rows (1, 3, …) carry `bg-muted/30` in their tw class.
#[test]
fn table_odd_rows_have_bg_muted_tw() {
    let node = eval_table(
        r#"<DataTable
            columns={[{key:"name", label:"NAME"}]}
            rows={[{name:"a"}, {name:"b"}, {name:"c"}]}
        />"#,
    );
    let tws = collect_row_tws(&node);
    assert!(
        tws.len() >= 2,
        "expected at least 2 row containers; got: {tws:?}"
    );
    assert!(
        tws[1].contains("bg-muted/30"),
        "expected row 1 tw to contain 'bg-muted/30'; got: '{}'",
        tws[1]
    );
}
