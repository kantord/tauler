use serde::Deserialize;
use serde_json::{Map, Value};
use crate::ui::{ContainerNode, Node, tw_merge};

const TRACK_TW: &str = "flex flex-row w-full h-[4px] rounded-full bg-muted";

#[derive(Deserialize, Default)]
pub struct ProgressProps {
    pub value: f32,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub tw: Option<String>,
}

fn style(pairs: &[(&str, Value)]) -> Option<Map<String, Value>> {
    let mut m = Map::new();
    for (k, v) in pairs {
        m.insert(k.to_string(), v.clone());
    }
    Some(m)
}

fn progress_impl(props: ProgressProps) -> Node {
    let value = props.value.clamp(0.0, 100.0) as f64;
    let track_tw = tw_merge(TRACK_TW, props.tw.as_deref().unwrap_or(""));
    let fill_tw = if props.color.is_some() {
        "h-[4px] rounded-full".to_string()
    } else {
        "h-[4px] rounded-full bg-primary".to_string()
    };

    let fill_style = if let Some(color) = &props.color {
        style(&[
            ("flex", Value::from(value)),
            ("backgroundColor", Value::String(color.clone())),
        ])
    } else {
        style(&[("flex", Value::from(value))])
    };

    Node::Container(ContainerNode {
        tw: Some(track_tw),
        style: None,
        children: vec![
            Node::Container(ContainerNode {
                tw: Some(fill_tw),
                style: fill_style,
                children: vec![],
            }),
            Node::Container(ContainerNode {
                tw: None,
                style: style(&[("flex", Value::from(100.0 - value))]),
                children: vec![],
            }),
        ],
    })
}

pub fn progress<'js>(
    ctx: rquickjs::Ctx<'js>,
    props: rquickjs::Value<'js>,
) -> rquickjs::Result<rquickjs::Value<'js>> {
    let props: ProgressProps =
        rquickjs_serde::from_value(props).map_err(|_| rquickjs::Error::Unknown)?;
    rquickjs_serde::to_value(ctx, &progress_impl(props)).map_err(|_| rquickjs::Error::Unknown)
}
