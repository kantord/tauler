use serde_json::{Map, Value};
use crate::ui::{component, rsx, cva::Cva};

const PROGRESS_VARIANTS: Cva = Cva {
    base: "flex flex-row w-full h-[4px] rounded-full bg-muted",
    variants: &[],
    defaults: &[],
};

/// A horizontal progress bar. Renders a muted track with a filled segment
/// proportional to `value` (0–100). An optional `color` prop overrides the
/// fill colour; `tw` applies extra Tailwind classes to the track.
///
/// # JSX
/// ```jsx
/// <container tw="flex flex-col gap-[6px] w-[200px]">
///   <container tw="flex flex-row justify-between">
///     <text tw="text-muted-foreground text-[11px]">Memory</text>
///     <text tw="text-foreground text-[11px]">72%</text>
///   </container>
///   <Progress value={72} />
/// </container>
/// ```
///
/// # Shadcn
/// https://ui.shadcn.com/docs/components/progress
#[component("@ui/progress")]
pub fn progress(value: f32, color: Option<String>, tw: Option<String>) -> Node {
    let value = value.clamp(0.0, 100.0);
    let track_tw = PROGRESS_VARIANTS.resolve(&[], tw.as_deref().unwrap_or(""));
    rsx! {
        <container tw={track_tw}>
            <Fill value={value} color={color} />
            <Remainder value={value} />
        </container>
    }
}

#[component]
fn fill(value: f32, color: Option<String>) -> Node {
    let has_color = color.is_some();
    let fill_tw = if has_color {
        "h-[4px] rounded-full".to_string()
    } else {
        "h-[4px] rounded-full bg-primary".to_string()
    };
    let mut style = Map::new();
    style.insert("flex".into(), Value::from(value as f64));
    if let Some(c) = color {
        style.insert("backgroundColor".into(), Value::String(c));
    }
    let fill_style = Some(style);
    rsx! { <container tw={fill_tw} style={fill_style} /> }
}

#[component]
fn remainder(value: f32) -> Node {
    let mut style = Map::new();
    style.insert("flex".into(), Value::from((100.0 - value) as f64));
    let remainder_style = Some(style);
    rsx! { <container style={remainder_style} /> }
}
