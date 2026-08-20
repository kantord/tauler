//! A slider that never holds its own value.
//!
//! One element, which captures the pointer when you press it and receives every
//! motion until you let go — `setPointerCapture` and `pointermove`, the way a
//! slider on a web page works (ADR 0020). X11 supplies the capture itself: pressing
//! a button implicitly grabs the pointer to that window until it comes up.
//!
//! The runtime hands the handler a position relative to the element and nothing
//! else. Turning that into a number is this component's job, in JavaScript, which
//! is why the shim below exists — Rust draws, JavaScript maps (ADR 0021).
//!
//! ## What it does not do
//!
//! It holds nothing (ADR 0012). `value` comes from outside on every tick, and the
//! slider only sees a drag's effect once whatever owns the value has re-emitted it.
//! A `<Slider>` with no `on_change` is inert — a `<Progress>` with a thumb.

use serde_json::{Map, Value};

use crate::ui::{component, cva::Cva, rsx, Node};

const SLIDER_VARIANTS: Cva = Cva {
    base: "flex flex-row items-center w-full h-[16px]",
    variants: &[],
    defaults: &[],
};

/// Proportion of the track that is filled, from `value` within `min..max`.
fn filled(value: f64, min: f64, max: f64) -> f64 {
    if max <= min {
        return 0.0;
    }
    ((value - min) / (max - min)).clamp(0.0, 1.0)
}

fn flex(amount: f64) -> Option<Map<String, Value>> {
    let mut style = Map::new();
    style.insert("flexGrow".into(), Value::from(amount));
    style.insert("flexShrink".into(), Value::from(1.0));
    style.insert("flexBasis".into(), Value::from(0.0));
    style.insert("minWidth".into(), Value::from(0.0));
    Some(style)
}

/// Height of the track row, and the thumb that rides on it.
const TRACK_H: f64 = 16.0;
const THUMB: f64 = 14.0;

/// The thumb, taken out of flow so the fill runs *under* it rather than stopping at
/// it — a thumb that is a flex item displaces the track by its own width, leaving a
/// gap the width of the thumb just before the value it marks.
///
/// The only out-of-flow node in the tree, deliberately: takumi blanks a whole parent
/// subtree when it has two or more (`docs/takumi-absolute-sibling-bug-research.md`).
///
/// It overhangs each end by half its width at the extremes. Insetting its travel, as
/// Radix does, would put it up to 7px from where the pointer actually is — the drag
/// handler maps position across the full width, so the thumb has to agree with that
/// rather than with the track's edges.
fn thumb(done: f64) -> Node {
    let mut style = Map::new();
    style.insert("position".into(), Value::from("absolute"));
    style.insert("top".into(), Value::from((TRACK_H - THUMB) / 2.0));
    style.insert(
        "left".into(),
        Value::from(format!("calc({}% - {}px)", done * 100.0, THUMB / 2.0)),
    );
    let style = Some(style);
    rsx! { <div class="h-[14px] w-[14px] rounded-full bg-background border border-primary" style={style} /> }
}

/// A horizontal slider. Draws `value` within `min`–`max`, and reports where you
/// press or drag.
///
/// It never remembers anything. `value` is read every tick from whatever owns it,
/// and `on_change` receives the value under the pointer and returns the intents to
/// send — one, or an array of them. Pressing counts as the first drag event, so a
/// plain click sets the value too.
///
/// ```jsx
/// <Module bin="~/.cargo/bin/tauler-audio">
///   {(data, events) => (
///     <Slider
///       value={data?.volume ?? 0}
///       step={5}
///       on_change={v => events.setVolume({ volume: v })}
///     />
///   )}
/// </Module>
/// ```
///
/// The drag sets nothing locally: it sends intents, the module changes the volume,
/// and the next tick brings the new `value` back. Omit `on_change` and the slider
/// still renders — it is simply not interactive.
///
/// `min` defaults to 0, `max` to 100, and `step` to 1. `step` rounds the reported
/// value, which is also what keeps a drag from sending a message per pixel: a motion
/// that produces the intents just sent is skipped.
///
/// The example below names a module, `tauler-demo-volume`, that you will not have —
/// pasted as it stands it renders a slider that does not move. It names one anyway,
/// because a slider with a literal `value` and no source would show you a control
/// holding its own state, which is the one thing this component does not do. On this
/// page the module is a few lines of JavaScript in the browser rather than a
/// subprocess; the layout file cannot tell the difference, and that is the point.
///
/// # JSX
/// ```jsx
/// const events = useEvents("tauler-demo-volume");
/// const volume = Number(useStringStream("tauler-demo-volume")) || 40;
///
/// return (
///   <div class="flex flex-col gap-[6px] w-[200px]">
///     <div class="flex flex-row justify-between">
///       <span class="text-muted-foreground text-[11px]">Volume</span>
///       <span class="text-foreground text-[11px]">{volume}%</span>
///     </div>
///     <Slider value={volume} step={5} on_change={(v) => events.set({ value: v })} />
///   </div>
/// );
/// ```
///
/// # Shadcn
/// https://ui.shadcn.com/docs/components/slider
#[component("@ui/slider")]
pub fn slider(
    value: f64,
    min: Option<f64>,
    max: Option<f64>,
    on_drag: Option<Value>,
    class: Option<String>,
) -> Node {
    let (min, max) = (min.unwrap_or(0.0), max.unwrap_or(100.0));
    let done = filled(value, min, max);
    let root_class = SLIDER_VARIANTS.resolve(&[], class.as_deref().unwrap_or(""));
    // The two halves of the track tile the full width between them, so the fill runs
    // right up to the value; the thumb is painted over it (see `thumb`).
    let mut root_style = Map::new();
    root_style.insert("position".into(), Value::from("relative"));
    let root_style = Some(root_style);
    rsx! {
        <div class={root_class} style={root_style} on_drag={on_drag}>
            <div class="h-[6px] rounded-l-full bg-primary" style={flex(done)} />
            <div class="h-[6px] rounded-r-full bg-muted" style={flex(1.0 - done)} />
            {thumb(done)}
        </div>
    }
}

/// The `<Slider>` shim's JavaScript half.
///
/// Owns the arithmetic, because only JavaScript can run when the pointer moves: it
/// turns the position the runtime reports into a value in the author's units and
/// calls `on_change` with it (ADR 0021). Rust never learns what a range is.
pub const SLIDER_SHIM_JS: &str = r#"
    globalThis.__tauler_slider = (props) => {
        const {
            value = 0, min = 0, max = 100, step = 1, on_change, class: cls,
        } = props ?? {};
        const rendered = { value, min, max };
        if (cls != null) rendered.class = cls;
        if (typeof on_change === "function") {
            // Registered here rather than by `h`: this calls the Rust component
            // directly, so these props never pass through the node flattener.
            rendered.on_drag = __tauler_handler_ref((p) => {
                // The runtime reports an unclamped position on purpose. Dragging past
                // either end of the track should pin to that end, not run off scale.
                const along = p.width > 0 ? Math.min(Math.max(p.x / p.width, 0), 1) : 0;
                const v = __tauler_snap(min + along * (max - min), step);
                return __tauler_intents(on_change(Math.min(Math.max(v, min), max)));
            });
        }
        return __ui_slider(rendered);
    };
"#;

/// Puts both halves in place: the Rust renderer under `__ui_slider`, and the shim
/// that `@ui/slider` actually exports.
#[cfg(feature = "quickjs")]
fn register_slider(ctx: &rquickjs::Ctx<'_>) -> rquickjs::Result<()> {
    (__UI_ENTRY_SLIDER.register)(ctx)?;
    ctx.eval::<(), _>(SLIDER_SHIM_JS)
}

/// What `import { Slider } from "@ui/slider"` resolves to.
///
/// Registered in place of `__UI_ENTRY_SLIDER`: the Rust half stays reachable from
/// JavaScript but is not importable, so there is only one `Slider` and it is the one
/// that accepts `on_change`.
#[cfg(feature = "quickjs")]
pub const __UI_ENTRY_SLIDER_SHIM: crate::ui::registry::EsEntry = crate::ui::registry::EsEntry {
    module_path: "@ui/slider",
    export_name: "Slider",
    global_name: "__tauler_slider",
    register: register_slider,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::UiComponent;

    fn render(value: f64, on_drag: Option<Value>) -> Value {
        serde_json::to_value(Slider::render(SliderProps {
            value,
            min: None,
            max: None,
            on_drag,
            class: None,
        }))
        .expect("slider serialises")
    }

    fn parts(rendered: &Value) -> &Vec<Value> {
        rendered["children"].as_array().expect("children")
    }

    fn grow(part: &Value) -> f64 {
        part["style"]["flexGrow"].as_f64().expect("flexGrow")
    }

    /// One element, not one per step: the pointer position carries the value now, so
    /// there is nothing to tile (ADR 0020).
    #[test]
    fn the_whole_track_is_one_capturing_element() {
        let handler = serde_json::json!({"$handler": 3});
        let rendered = render(40.0, Some(handler.clone()));
        assert_eq!(rendered["on_drag"], handler, "the handler is on the track");
        assert!(
            parts(&rendered).iter().all(|p| p.get("on_drag").is_none()),
            "nothing inside the track handles anything"
        );
    }

    /// Only block-level elements are hittable (ADR 0018).
    #[test]
    fn the_track_is_a_block_element() {
        assert_eq!(render(0.0, None)["type"], "div");
    }

    #[test]
    fn the_fill_is_the_value_as_a_proportion_of_the_range() {
        let rendered = render(25.0, None);
        assert_eq!(grow(&parts(&rendered)[0]), 0.25, "filled");
        assert_eq!(grow(&parts(&rendered)[1]), 0.75, "remaining");
    }

    #[test]
    fn a_value_outside_the_range_pins_to_the_nearer_end() {
        assert_eq!(grow(&parts(&render(400.0, None))[0]), 1.0);
        assert_eq!(grow(&parts(&render(-400.0, None))[0]), 0.0);
    }

    /// A slider with nothing to talk to still draws, rather than vanishing.
    #[test]
    fn without_a_handler_it_renders_but_captures_nothing() {
        let rendered = render(40.0, None);
        assert!(rendered.get("on_drag").is_none());
        assert_eq!(grow(&parts(&rendered)[0]), 0.4);
    }

    /// The bug this guards: a thumb that is a flex item displaces the track, so the
    /// fill stops a thumb's width short of the value it is meant to mark.
    #[test]
    fn the_fill_runs_under_the_thumb_rather_than_stopping_at_it() {
        let rendered = render(40.0, None);
        let parts = parts(&rendered);
        assert_eq!(parts.len(), 3);
        assert_eq!(
            grow(&parts[0]) + grow(&parts[1]),
            1.0,
            "the two halves tile the whole width; the thumb takes none of it"
        );
        assert_eq!(
            parts[2]["style"]["position"], "absolute",
            "the thumb is painted over the track, not laid out in it"
        );
        assert_eq!(parts[2]["style"]["left"], "calc(40% - 7px)");
    }

    /// Exactly one out-of-flow node: takumi blanks the parent subtree with two.
    #[test]
    fn only_the_thumb_is_out_of_flow() {
        let rendered = render(40.0, None);
        let absolute = parts(&rendered)
            .iter()
            .filter(|p| p["style"]["position"] == "absolute")
            .count();
        assert_eq!(absolute, 1);
    }

    /// An inverted or empty range must draw something rather than divide by zero.
    #[test]
    fn a_degenerate_range_draws_an_empty_track() {
        let rendered = serde_json::to_value(Slider::render(SliderProps {
            value: 5.0,
            min: Some(10.0),
            max: Some(10.0),
            on_drag: None,
            class: None,
        }))
        .unwrap();
        assert_eq!(grow(&parts(&rendered)[0]), 0.0);
    }

    #[test]
    fn class_is_appended_to_the_track() {
        let rendered = serde_json::to_value(Slider::render(SliderProps {
            value: 0.0,
            min: None,
            max: None,
            on_drag: None,
            class: Some("w-[160px]".into()),
        }))
        .unwrap();
        assert!(rendered["class"].as_str().unwrap().ends_with("w-[160px]"));
    }
}
