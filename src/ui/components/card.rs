use serde::Deserialize;
use crate::ui::{Node, UiComponent, tw_merge, ui};

const BASE_TW: &str = "rounded-lg border border-border bg-card text-card-foreground px-3 py-[10px]";

#[derive(Deserialize, Default)]
pub struct CardProps {
    #[serde(default)]
    pub children: Vec<Node>,
    #[serde(default)]
    pub tw: Option<String>,
}

pub struct Card;

impl UiComponent for Card {
    type Props = CardProps;

    fn render(props: CardProps) -> Node {
        let tw = tw_merge(BASE_TW, props.tw.as_deref().unwrap_or(""));
        ui! {
            <container tw={tw}>
                {props.children}
            </container>
        }
    }
}
