mod common;

const HEADER_BASE_TW: &str = "flex flex-col gap-[4px]";
const TITLE_BASE_TW: &str = "font-semibold text-[14px] leading-none tracking-tight";
const DESCRIPTION_BASE_TW: &str = "text-[12px] text-muted-foreground";
const CONTENT_BASE_TW: &str = "flex flex-col";
const FOOTER_BASE_TW: &str = "flex flex-row items-center";

fn eval_card(inner_jsx: &str) -> serde_json::Value {
    let source = format!(
        "import {{ Card, CardHeader, CardTitle, CardDescription, CardContent, CardFooter }} from '@ui/card';\
\nexport default function render() {{ return {inner_jsx}; }}"
    );
    common::eval_jsx(&source).layout
}

fn assert_tw_prop_appended(component_jsx: &str, base: &str) {
    let with_extra = component_jsx.replace(" />", r#" tw="extra" />"#);
    let tw = eval_card(&with_extra)["tw"].as_str().unwrap().to_string();
    assert_eq!(tw, format!("{base} extra"));
}

mod card_header {
    use super::*;

    #[test]
    fn renders_as_container() { assert_eq!(eval_card("<CardHeader />")["type"], "container"); }

    #[test]
    fn has_base_tw() { assert_eq!(eval_card("<CardHeader />")["tw"], HEADER_BASE_TW); }

    #[test]
    fn merges_tw_prop() { assert_tw_prop_appended("<CardHeader />", HEADER_BASE_TW); }
}

mod card_title {
    use super::*;

    #[test]
    fn renders_as_container() { assert_eq!(eval_card("<CardTitle />")["type"], "container"); }

    #[test]
    fn has_base_tw() { assert_eq!(eval_card("<CardTitle />")["tw"], TITLE_BASE_TW); }

    #[test]
    fn merges_tw_prop() { assert_tw_prop_appended("<CardTitle />", TITLE_BASE_TW); }
}

mod card_description {
    use super::*;

    #[test]
    fn renders_as_container() { assert_eq!(eval_card("<CardDescription />")["type"], "container"); }

    #[test]
    fn has_base_tw() { assert_eq!(eval_card("<CardDescription />")["tw"], DESCRIPTION_BASE_TW); }

    #[test]
    fn merges_tw_prop() { assert_tw_prop_appended("<CardDescription />", DESCRIPTION_BASE_TW); }
}

mod card_content {
    use super::*;

    #[test]
    fn renders_as_container() { assert_eq!(eval_card("<CardContent />")["type"], "container"); }

    #[test]
    fn has_base_tw() { assert_eq!(eval_card("<CardContent />")["tw"], CONTENT_BASE_TW); }

    #[test]
    fn merges_tw_prop() { assert_tw_prop_appended("<CardContent />", CONTENT_BASE_TW); }
}

mod card_footer {
    use super::*;

    #[test]
    fn renders_as_container() { assert_eq!(eval_card("<CardFooter />")["type"], "container"); }

    #[test]
    fn has_base_tw() { assert_eq!(eval_card("<CardFooter />")["tw"], FOOTER_BASE_TW); }

    #[test]
    fn merges_tw_prop() { assert_tw_prop_appended("<CardFooter />", FOOTER_BASE_TW); }
}
