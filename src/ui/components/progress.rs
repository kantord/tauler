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

pub struct Progress;

impl UiComponent for Progress {
    type Props = ProgressProps;

    fn render(props: ProgressProps) -> Node {
        let value = props.value.clamp(0.0, 100.0);
        let track_tw = tw_merge(TRACK_TW, props.tw.as_deref().unwrap_or(""));
        let color = props.color;
        ui! {
            <container tw={track_tw}>
                <Fill value={value} color={color} />
                <Remainder value={value} />
            </container>
        }
    }
}

#[derive(Deserialize, Default)]
struct FillProps {
    value: f32,
    #[serde(default)]
    color: Option<String>,
}

struct Fill;

impl UiComponent for Fill {
    type Props = FillProps;

    fn render(props: FillProps) -> Node {
        let flex = props.value as f64;
        let has_color = props.color.is_some();
        let fill_tw = if has_color {
            "h-[4px] rounded-full".to_string()
        } else {
            "h-[4px] rounded-full bg-primary".to_string()
        };
        let mut style = Map::new();
        style.insert("flex".into(), Value::from(flex));
        if let Some(color) = props.color {
            style.insert("backgroundColor".into(), Value::String(color));
        }
        let fill_style = Some(style);
        ui! { <container tw={fill_tw} style={fill_style} /> }
    }
}

#[derive(Deserialize, Default)]
struct RemainderProps {
    value: f32,
}

struct Remainder;

impl UiComponent for Remainder {
    type Props = RemainderProps;

    fn render(props: RemainderProps) -> Node {
        let flex = (100.0 - props.value) as f64;
        let mut style = Map::new();
        style.insert("flex".into(), Value::from(flex));
        let remainder_style = Some(style);
        ui! { <container style={remainder_style} /> }
    }
}
