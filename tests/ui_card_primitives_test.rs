mod common;

fn eval_card(inner_jsx: &str) -> serde_json::Value {
    let source = format!(
        "import {{ Card, CardHeader, CardTitle, CardDescription, CardContent, CardFooter }} from '@ui/card';\
\nexport default function render() {{ return {inner_jsx}; }}"
    );
    common::eval_jsx(&source).layout
}

mod card_header {
    use super::eval_card;

    fn tw() -> String { eval_card("<CardHeader />")["tw"].as_str().unwrap().to_string() }

    #[test]
    fn renders_as_container() { assert_eq!(eval_card("<CardHeader />")["type"], "container"); }

    #[test]
    fn has_flex_col() { assert!(tw().contains("flex-col"), "flex-col missing: {}", tw()); }

    #[test]
    fn merges_tw_prop() {
        let tw = eval_card(r#"<CardHeader tw="extra" />"#)["tw"].as_str().unwrap().to_string();
        assert!(tw.contains("extra"), "extra tw not appended: {tw}");
    }
}

mod card_title {
    use super::eval_card;

    fn tw() -> String { eval_card("<CardTitle />")["tw"].as_str().unwrap().to_string() }

    #[test]
    fn renders_as_container() { assert_eq!(eval_card("<CardTitle />")["type"], "container"); }

    #[test]
    fn has_font_semibold() { assert!(tw().contains("font-semibold"), "font-semibold missing: {}", tw()); }

    #[test]
    fn merges_tw_prop() {
        let tw = eval_card(r#"<CardTitle tw="extra" />"#)["tw"].as_str().unwrap().to_string();
        assert!(tw.contains("extra"), "extra tw not appended: {tw}");
    }
}

mod card_description {
    use super::eval_card;

    fn tw() -> String { eval_card("<CardDescription />")["tw"].as_str().unwrap().to_string() }

    #[test]
    fn renders_as_container() { assert_eq!(eval_card("<CardDescription />")["type"], "container"); }

    #[test]
    fn has_muted_foreground() { assert!(tw().contains("text-muted-foreground"), "text-muted-foreground missing: {}", tw()); }

    #[test]
    fn merges_tw_prop() {
        let tw = eval_card(r#"<CardDescription tw="extra" />"#)["tw"].as_str().unwrap().to_string();
        assert!(tw.contains("extra"), "extra tw not appended: {tw}");
    }
}

mod card_content {
    use super::eval_card;

    fn tw() -> String { eval_card("<CardContent />")["tw"].as_str().unwrap().to_string() }

    #[test]
    fn renders_as_container() { assert_eq!(eval_card("<CardContent />")["type"], "container"); }

    #[test]
    fn has_flex_col() { assert!(tw().contains("flex-col"), "flex-col missing: {}", tw()); }

    #[test]
    fn merges_tw_prop() {
        let tw = eval_card(r#"<CardContent tw="extra" />"#)["tw"].as_str().unwrap().to_string();
        assert!(tw.contains("extra"), "extra tw not appended: {tw}");
    }
}

mod card_footer {
    use super::eval_card;

    fn tw() -> String { eval_card("<CardFooter />")["tw"].as_str().unwrap().to_string() }

    #[test]
    fn renders_as_container() { assert_eq!(eval_card("<CardFooter />")["type"], "container"); }

    #[test]
    fn has_flex_row() { assert!(tw().contains("flex-row"), "flex-row missing: {}", tw()); }

    #[test]
    fn has_items_center() { assert!(tw().contains("items-center"), "items-center missing: {}", tw()); }

    #[test]
    fn merges_tw_prop() {
        let tw = eval_card(r#"<CardFooter tw="extra" />"#)["tw"].as_str().unwrap().to_string();
        assert!(tw.contains("extra"), "extra tw not appended: {tw}");
    }
}
