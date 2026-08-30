pub mod badge;
pub mod card;
pub mod i3_layout;
pub mod icon;
pub mod knob;
pub mod progress;
pub mod scroll_area;
pub mod scroll_bar;
pub mod slider;
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
            <div class="flex flex-col gap-[4px]">
                {bar}
            </div>
        };
        let Node::Element(c) = &node else {
            panic!("expected an element")
        };
        assert_eq!(c.class.as_deref(), Some("flex flex-col gap-[4px]"));
        assert_eq!(c.children.len(), 1);
        let Node::Element(track) = &c.children[0] else {
            panic!("expected progress track")
        };
        assert!(track.class.as_deref().unwrap_or("").contains("bg-muted"));
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
        let Node::Element(card) = &node else {
            panic!("expected card element")
        };
        assert!(card.class.as_deref().unwrap_or("").contains("bg-card"));
        assert_eq!(card.children.len(), 1);
        let Node::Element(track) = &card.children[0] else {
            panic!("expected progress track")
        };
        assert!(track.class.as_deref().unwrap_or("").contains("bg-muted"));
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
            <Card class="mt-2">
                {bar}
                <Progress value={70.0} />
            </Card>
        };
        let Node::Element(card) = &node else {
            panic!("expected card")
        };
        assert_eq!(card.children.len(), 2);
    }
}
