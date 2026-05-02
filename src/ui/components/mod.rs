pub mod badge;
pub mod card;
pub mod progress;
pub mod table;
pub mod test_multi;

#[cfg(test)]
mod composition_tests {
    use super::progress::{Progress, ProgressProps};
    use crate::ui::{rsx, Node, UiComponent};

    /// Style 1: render into a variable, interpolate with {bar}
    #[test]
    fn component_can_embed_another_via_variable() {
        let bar = Progress::render(ProgressProps {
            value: 60.0,
            ..Default::default()
        });
        let node = rsx! {
            <container tw="flex flex-col gap-[4px]">
                {bar}
            </container>
        };
        let Node::Container(c) = &node else {
            panic!("expected container")
        };
        assert_eq!(c.tw.as_deref(), Some("flex flex-col gap-[4px]"));
        assert_eq!(c.children.len(), 1);
        let Node::Container(track) = &c.children[0] else {
            panic!("expected progress track")
        };
        assert!(track.tw.as_deref().unwrap_or("").contains("bg-muted"));
    }

    /// Style 2: <Component /> PascalCase syntax inside rsx!
    #[test]
    fn component_can_nest_another_with_pascal_case_syntax() {
        use super::card::Card;
        let node = rsx! {
            <Card>
                <Progress value={60.0} />
            </Card>
        };
        let Node::Container(card) = &node else {
            panic!("expected card container")
        };
        assert!(card.tw.as_deref().unwrap_or("").contains("bg-card"));
        assert_eq!(card.children.len(), 1);
        let Node::Container(track) = &card.children[0] else {
            panic!("expected progress track")
        };
        assert!(track.tw.as_deref().unwrap_or("").contains("bg-muted"));
        assert_eq!(track.children.len(), 2);
    }

    /// Both styles mixed in one tree
    #[test]
    fn both_composition_styles_can_be_mixed() {
        use super::card::Card;
        let bar = Progress::render(ProgressProps {
            value: 30.0,
            ..Default::default()
        });
        let node = rsx! {
            <Card tw="mt-2">
                {bar}
                <Progress value={70.0} />
            </Card>
        };
        let Node::Container(card) = &node else {
            panic!("expected card")
        };
        assert_eq!(card.children.len(), 2);
    }
}
