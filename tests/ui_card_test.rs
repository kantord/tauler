mod common;

const BASE_TW: &str =
    "rounded-lg border border-border bg-card text-card-foreground px-3 py-[10px]";

fn eval_card(inner_jsx: &str) -> serde_json::Value {
    let source = format!(
        "import {{ Card }} from '@ui/card';\nexport default function render() {{ return {inner_jsx}; }}"
    );
    common::eval_jsx(&source).layout
}

// --- tw prop ---

#[test]
fn card_has_base_tw_by_default() {
    assert_eq!(eval_card("<Card />")["tw"], BASE_TW);
}

#[test]
fn card_appends_extra_tw_classes() {
    assert_eq!(
        eval_card(r#"<Card tw="flex flex-col" />"#)["tw"],
        format!("{BASE_TW} flex flex-col"),
    );
}

#[test]
fn card_tw_override_is_appended_for_last_wins_resolution() {
    // takumi applies declarations in order (last-wins), so py-[8px] after py-[10px] wins
    assert_eq!(
        eval_card(r#"<Card tw="py-[8px]" />"#)["tw"],
        format!("{BASE_TW} py-[8px]"),
    );
}

// --- node shape ---

#[test]
fn card_renders_as_container_node() {
    assert_eq!(eval_card("<Card />")["type"], "container");
}

#[test]
fn card_with_no_children_omits_children_key() {
    assert!(eval_card("<Card />").get("children").is_none());
}

// --- children ---

#[test]
fn card_passes_single_child_through() {
    let children = eval_card(r#"<Card><text tw="text-white">{"hello"}</text></Card>"#)
        ["children"]
        .as_array()
        .expect("children array")
        .clone();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0]["type"], "text");
    assert_eq!(children[0]["tw"], "text-white");
    assert_eq!(children[0]["text"], "hello");
}

#[test]
fn card_preserves_child_order() {
    let children = eval_card(
        r#"<Card><text tw="a">{"first"}</text><text tw="b">{"second"}</text></Card>"#,
    )["children"]
        .as_array()
        .expect("children array")
        .clone();
    assert_eq!(children.len(), 2);
    assert_eq!(children[0]["text"], "first");
    assert_eq!(children[1]["text"], "second");
}
