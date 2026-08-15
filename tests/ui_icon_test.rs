mod common;

fn eval_icon(props: &str) -> serde_json::Value {
    let source = format!(
        "import {{ Icon }} from '@ui/icon';\nexport default function render() {{ return {props}; }}"
    );
    common::eval_jsx(&source).layout
}

mod node_shape {
    use super::*;

    #[test]
    fn renders_as_a_span() {
        assert_eq!(eval_icon(r#"<Icon name="md-home" />"#)["type"], "span");
    }
}

mod glyph_resolution {
    use super::*;

    #[test]
    fn known_name_resolves_to_correct_glyph() {
        let node = eval_icon(r#"<Icon name="md-home" />"#);
        assert_eq!(node["children"][0], "\u{f02dc}");
    }

    #[test]
    fn another_family_resolves_correctly() {
        let node = eval_icon(r#"<Icon name="fa-github" />"#);
        assert_eq!(node["children"][0], "\u{f09b}");
    }

    #[test]
    fn unknown_name_renders_fallback() {
        let node = eval_icon(r#"<Icon name="totally-unknown-icon" />"#);
        assert_eq!(node["children"][0], "?");
    }
}

mod tw_prop {
    use super::*;

    #[test]
    fn tw_is_appended_after_base_foreground_class() {
        let node = eval_icon(r#"<Icon name="md-home" class="text-red-500" />"#);
        assert_eq!(node["class"], "text-foreground text-red-500");
    }

    #[test]
    fn no_tw_prop_uses_foreground_color() {
        let node = eval_icon(r#"<Icon name="md-home" />"#);
        assert_eq!(node["class"], "text-foreground");
    }
}
