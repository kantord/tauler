use serde::Deserialize;
use crate::ui::{Node, tw_merge, ui};

const BASE_TW: &str = "rounded-lg border border-border bg-card text-card-foreground px-3 py-[10px]";

#[derive(Deserialize, Default)]
pub struct CardProps {
    #[serde(default)]
    pub children: Vec<Node>,
    #[serde(default)]
    pub tw: Option<String>,
}

fn card_impl(props: CardProps) -> Node {
    let tw = tw_merge(BASE_TW, props.tw.as_deref().unwrap_or(""));
    ui! {
        <container tw={tw}>
            {props.children}
        </container>
    }
}

pub fn card<'js>(
    ctx: rquickjs::Ctx<'js>,
    props: rquickjs::Value<'js>,
) -> rquickjs::Result<rquickjs::Value<'js>> {
    let card_props: CardProps = rquickjs_serde::from_value(props).map_err(|_| rquickjs::Error::Unknown)?;
    rquickjs_serde::to_value(ctx, &card_impl(card_props)).map_err(|_| rquickjs::Error::Unknown)
}
