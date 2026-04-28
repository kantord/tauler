use serde::Deserialize;
use crate::ui::{ContainerNode, Node};

#[derive(Deserialize, Default)]
pub struct CardProps {
    #[serde(default)]
    pub children: Vec<Node>,
}

pub fn card<'js>(
    ctx: rquickjs::Ctx<'js>,
    props: rquickjs::Value<'js>,
) -> rquickjs::Result<rquickjs::Value<'js>> {
    let card_props: CardProps = rquickjs_serde::from_value(props).unwrap_or_default();
    let node = Node::Container(ContainerNode {
        tw: Some("rounded-lg border border-border bg-card text-card-foreground px-3 py-[10px]".into()),
        children: card_props.children,
    });
    rquickjs_serde::to_value(ctx, &node).map_err(|_| rquickjs::Error::Unknown)
}
