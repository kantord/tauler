use serde::Deserialize;
use crate::ui::{ContainerNode, Node, tw_merge};

const BASE_TW: &str = "rounded-lg border border-border bg-card text-card-foreground px-3 py-[10px]";

#[derive(Deserialize, Default)]
pub struct CardProps {
    #[serde(default)]
    pub children: Vec<Node>,
    #[serde(default)]
    pub tw: Option<String>,
}

pub fn card<'js>(
    ctx: rquickjs::Ctx<'js>,
    props: rquickjs::Value<'js>,
) -> rquickjs::Result<rquickjs::Value<'js>> {
    let card_props: CardProps = rquickjs_serde::from_value(props).unwrap_or_default();
    let tw = tw_merge(BASE_TW, card_props.tw.as_deref().unwrap_or(""));
    let node = Node::Container(ContainerNode {
        tw: Some(tw),
        children: card_props.children,
    });
    rquickjs_serde::to_value(ctx, &node).map_err(|_| rquickjs::Error::Unknown)
}
