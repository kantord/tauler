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

mod node_shape {
    use super::*;

    #[test]
    fn renders_as_container_node() {
        assert_eq!(eval_badge("<Badge />")["type"], "container");
    }
}

mod variant_classes {
    use super::*;

    fn assert_variant(variant: &str, expected_variant_tw: &str) {
        let expected = format!("{BASE_TW} {expected_variant_tw}");
        assert_eq!(eval_badge(&format!(r#"<Badge variant="{variant}" />"#))["tw"], expected);
    }

    #[test]
    fn default_variant_applies_base_and_default_classes() {
        assert_eq!(eval_badge("<Badge />")["tw"], format!("{BASE_TW} {DEFAULT_VARIANT_TW}"));
    }

    #[test]
    fn secondary_variant_applies_correct_classes() {
        assert_variant("secondary", SECONDARY_VARIANT_TW);
    }

    #[test]
    fn destructive_variant_applies_correct_classes() {
        assert_variant("destructive", DESTRUCTIVE_VARIANT_TW);
    }

    #[test]
    fn outline_variant_applies_correct_classes() {
        assert_variant("outline", OUTLINE_VARIANT_TW);
    }

    #[test]
    fn unknown_variant_value_produces_base_classes_only() {
        assert_eq!(eval_badge(r#"<Badge variant="garbage" />"#)["tw"], BASE_TW);
    }
}

mod tw_prop {
    use super::*;

    #[test]
    fn extra_tw_is_appended_after_variant_classes() {
        let expected = format!("{BASE_TW} {DEFAULT_VARIANT_TW} extra-class");
        assert_eq!(eval_badge(r#"<Badge tw="extra-class" />"#)["tw"], expected);
    }
}
