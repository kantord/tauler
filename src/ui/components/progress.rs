use serde::Deserialize;
use serde_json::{Map, Value};
use crate::ui::{Node, UiComponent, tw_merge, ui};

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

pub struct Progress;

impl UiComponent for Progress {
    type Props = ProgressProps;

    fn render(props: ProgressProps) -> Node {
        let value = props.value.clamp(0.0, 100.0) as f64;
        let track_tw = tw_merge(TRACK_TW, props.tw.as_deref().unwrap_or(""));
        let fill_tw = if props.color.is_some() {
            "h-[4px] rounded-full".to_string()
        } else {
            "h-[4px] rounded-full bg-primary".to_string()
        };
        let fill_style = if let Some(color) = &props.color {
            style(&[("flex", Value::from(value)), ("backgroundColor", Value::String(color.clone()))])
        } else {
            style(&[("flex", Value::from(value))])
        };
        let remainder_style = style(&[("flex", Value::from(100.0 - value))]);
        ui! {
            <container tw={track_tw}>
                <container tw={fill_tw} style={fill_style} />
                <container style={remainder_style} />
            </container>
        }
    }
}
