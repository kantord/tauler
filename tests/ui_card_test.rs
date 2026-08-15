mod common;

const BASE_TW: &str = "rounded-lg border border-border bg-card text-card-foreground px-3 py-[10px]";

fn eval_card(inner_jsx: &str) -> serde_json::Value {
    let source = format!(
        "import {{ Card }} from '@ui/card';\nexport default function render() {{ return {inner_jsx}; }}"
    );
    common::eval_jsx(&source).layout
}

// --- tw prop ---

#[test]
fn card_has_base_tw_by_default() {
    assert_eq!(eval_card("<Card />")["class"], BASE_TW);
}

#[test]
fn card_appends_extra_tw_classes() {
    assert_eq!(
        eval_card(r#"<Card class="flex flex-col" />"#)["class"],
        format!("{BASE_TW} flex flex-col"),
    );
}

#[test]
fn card_tw_override_is_appended_for_last_wins_resolution() {
    // takumi applies declarations in order (last-wins), so py-[8px] after py-[10px] wins
    assert_eq!(
        eval_card(r#"<Card class="py-[8px]" />"#)["class"],
        format!("{BASE_TW} py-[8px]"),
    );
}

// --- node shape ---

#[test]
fn card_renders_as_a_div() {
    assert_eq!(eval_card("<Card />")["type"], "div");
}

#[test]
fn card_with_no_children_omits_children_key() {
    assert!(eval_card("<Card />").get("children").is_none());
}

// --- children ---

#[test]
fn card_passes_single_child_through() {
    let children = eval_card(r#"<Card><span class="text-white">{"hello"}</span></Card>"#)
        ["children"]
        .as_array()
        .expect("children array")
        .clone();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0]["type"], "span");
    assert_eq!(children[0]["class"], "text-white");
    assert_eq!(children[0]["children"][0], "hello");
}

#[test]
fn card_preserves_child_order() {
    let children = eval_card(
        r#"<Card><span class="a">{"first"}</span><span class="b">{"second"}</span></Card>"#,
    )["children"]
        .as_array()
        .expect("children array")
        .clone();
    assert_eq!(children.len(), 2);
    assert_eq!(children[0]["children"][0], "first");
    assert_eq!(children[1]["children"][0], "second");
}
