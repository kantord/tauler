mod common;

const BASE_TW: &str =
    "inline-flex items-center rounded-full border px-[10px] py-[2px] text-[12px] font-semibold";

const DEFAULT_VARIANT_TW: &str = "border-transparent bg-primary text-primary-foreground";
const SECONDARY_VARIANT_TW: &str = "border-transparent bg-secondary text-secondary-foreground";
const DESTRUCTIVE_VARIANT_TW: &str = "border-transparent bg-destructive text-destructive-foreground";
const OUTLINE_VARIANT_TW: &str = "text-foreground";

fn eval_badge(inner_jsx: &str) -> serde_json::Value {
    let source = format!(
        "import {{ Badge }} from '@ui/badge';\nexport default function render() {{ return {inner_jsx}; }}"
    );
    common::eval_jsx(&source).layout
}

// --- node shape ---

#[test]
fn badge_renders_as_container_node() {
    assert_eq!(eval_badge("<Badge />")["type"], "container");
}

// --- variant classes ---

#[test]
fn badge_default_variant_applies_base_and_default_variant_classes() {
    let expected = format!("{BASE_TW} {DEFAULT_VARIANT_TW}");
    assert_eq!(eval_badge("<Badge />")["tw"], expected);
}

#[test]
fn badge_secondary_variant_applies_base_and_secondary_classes() {
    let expected = format!("{BASE_TW} {SECONDARY_VARIANT_TW}");
    assert_eq!(
        eval_badge(r#"<Badge variant="secondary" />"#)["tw"],
        expected
    );
}

#[test]
fn badge_destructive_variant_applies_base_and_destructive_classes() {
    let expected = format!("{BASE_TW} {DESTRUCTIVE_VARIANT_TW}");
    assert_eq!(
        eval_badge(r#"<Badge variant="destructive" />"#)["tw"],
        expected
    );
}

#[test]
fn badge_outline_variant_applies_base_and_outline_classes() {
    let expected = format!("{BASE_TW} {OUTLINE_VARIANT_TW}");
    assert_eq!(
        eval_badge(r#"<Badge variant="outline" />"#)["tw"],
        expected
    );
}

// --- tw prop ---

#[test]
fn badge_extra_tw_prop_is_appended_after_variant_classes() {
    let expected = format!("{BASE_TW} {DEFAULT_VARIANT_TW} extra-class");
    assert_eq!(
        eval_badge(r#"<Badge tw="extra-class" />"#)["tw"],
        expected
    );
}
