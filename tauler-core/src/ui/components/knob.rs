//! A rotary knob that never holds its own angle.
//!
//! The same machinery as `<Slider>` — one element that captures the pointer and
//! receives every motion until you let go (ADR 0020) — turned round. What differs is
//! the mapping: a slider reads a position along its width, a knob reads the angle from
//! its centre, and reads it *relative to where you pressed* (ADR 0022).
//!
//! That relative reading is the whole reason a knob needs no `min` and no `max`. It
//! never asks "what value is under the pointer", which would need a scale; it asks
//! "how far has this turned since the press", which needs only two points, both of
//! which the runtime supplies. The knob adds that displacement to the angle it was
//! drawn at and reports the result.
//!
//! ## What it does not do
//!
//! It holds nothing (ADR 0012). `value` comes from outside on every tick, and the knob
//! only sees a turn's effect once whatever owns the angle has re-emitted it. A `<Knob>`
//! with no `on_change` is inert.
//!
//! It counts no turns. Two points read a displacement of at most half a circle either
//! way, and the reported angle wraps into 0–360 — which is exactly enough, because on a
//! circle a sweep of +270° arrives where one of -90° does. What has no answer is "how
//! many times round", and a knob with no `min` and no `max` never asks.

use serde_json::{Map, Value};

use crate::ui::{component, cva::Cva, rsx, Node};

const KNOB_VARIANTS: Cva = Cva {
    base: "flex w-[40px] h-[40px] rounded-full border border-border bg-muted",
    variants: &[],
    defaults: &[],
};

/// The needle's carrier fills the dial, so rotating it turns the needle about the
/// dial's centre — which is what `rotate` uses as its origin by default.
///
/// Done this way rather than with an absolutely-positioned needle because takumi
/// blanks a whole parent subtree once it has two or more out-of-flow children
/// (`docs/takumi-absolute-sibling-bug-research.md`), and a knob that later grows a
/// second overlay should not have to be rebuilt to survive it.
fn needle(degrees: f64) -> Node {
    let mut style = Map::new();
    style.insert("rotate".into(), Value::from(format!("{degrees}deg")));
    let style = Some(style);
    rsx! {
        <div class="w-full h-full flex flex-row justify-center items-start pt-[4px]" style={style}>
            <div class="w-[3px] h-[11px] rounded-full bg-primary" />
        </div>
    }
}

/// A rotary knob. Draws `value` as an angle in degrees — 0 points up, and the angle
/// increases clockwise — and reports the angle you turn it to.
///
/// It never remembers anything. `value` is read every tick from whatever owns it, and
/// `on_change` receives the new angle and returns the intents to send — one, or an
/// array of them.
///
/// ```jsx
/// <Module bin="~/.cargo/bin/tauler-audio">
///   {(data, events) => (
///     <Knob
///       value={data?.balance ?? 0}
///       step={5}
///       on_change={deg => events.setBalance({ deg })}
///     />
///   )}
/// </Module>
/// ```
///
/// The turn sets nothing locally: it sends intents, the module changes the angle, and
/// the next tick brings the new `value` back. Omit `on_change` and the knob still
/// renders — it is simply not interactive.
///
/// There is no `min` and no `max`, because the knob measures how far you have turned
/// it rather than where on a scale you are pointing. Pressing it anywhere is a turn of
/// zero, so it never jumps to meet the pointer, and a fast flick and a slow drag that
/// end in the same place give the same angle.
///
/// The inner third is a hub that reports nothing. A bearing taken there is meaningless
/// — undefined at the exact centre, and swinging through tens of degrees per pixel
/// around it — so a press that lands in the hub, or a drag that wanders into it, is
/// ignored rather than allowed to leap. Turn it by the rim.
///
/// `value` and the reported angle have deliberately different domains. `value` is drawn
/// as given, so `450` and `-90` point where they say. What `on_change` reports is always
/// wrapped into 0–360, so turning past the top comes round rather than running off and a
/// module's stored number cannot drift out to thousands. What it cannot report is how
/// many whole turns you made — there is no scale for them to mean anything on.
///
/// `step` defaults to 1 and rounds the *turn*, not the angle it lands on. Rounding the
/// angle would move a press that has not travelled at all, and would shift the grid a
/// little further every lap for a step that does not divide a circle. Rounding is also
/// what keeps a turn from sending a message per pixel: a motion that produces the
/// intents just sent is skipped.
///
/// # JSX
/// ```jsx
/// <div class="flex flex-row gap-[12px] items-center">
///   <Knob value={0} />
///   <Knob value={45} />
///   <Knob value={135} />
///   <Knob value={250} />
/// </div>
/// ```
#[component("@ui/knob")]
pub fn knob(value: f64, on_drag: Option<Value>, class: Option<String>) -> Node {
    let root_class = KNOB_VARIANTS.resolve(&[], class.as_deref().unwrap_or(""));
    rsx! {
        <div class={root_class} on_drag={on_drag}>
            {needle(value)}
        </div>
    }
}

/// The `<Knob>` shim's JavaScript half.
///
/// Owns the trigonometry, because only JavaScript can run when the pointer moves: it
/// turns the two positions the runtime reports into an angle in degrees and calls
/// `on_change` with it (ADR 0021). Rust never learns what a turn is.
pub const KNOB_SHIM_JS: &str = r#"
    globalThis.__tauler_knob = (props) => {
        const { value = 0, step = 1, on_change, class: cls } = props ?? {};
        const rendered = { value };
        if (cls != null) rendered.class = cls;
        if (typeof on_change === "function") {
            // Registered here rather than by `h`: this calls the Rust component
            // directly, so these props never pass through the node flattener.
            rendered.on_drag = __tauler_handler_ref((p) => {
                const cx = p.width / 2, cy = p.height / 2;
                // A bearing taken near the middle means nothing: one pixel of movement
                // swings it through tens of degrees, and at the exact centre it is not
                // defined at all. So the inner third reports nothing rather than
                // leaping — scaled per axis, so an oval dial gets an oval hub.
                const outsideHub = (x, y) => {
                    const dx = (x - cx) / cx, dy = (y - cy) / cy;
                    return dx * dx + dy * dy >= 0.3 * 0.3;
                };
                if (!outsideHub(p.x, p.y) || !outsideHub(p.press_x, p.press_y)) {
                    return null;
                }
                // Measured from the centre, with 0 pointing up and the angle growing
                // clockwise, so it reads the way the dial is drawn.
                const bearing = (x, y) => Math.atan2(x - cx, cy - y) * 180 / Math.PI;
                // Into -180..180: the short way round is always the way you turned,
                // and half a circle is as much as two points can express.
                const turned = ((bearing(p.x, p.y) - bearing(p.press_x, p.press_y))
                    % 360 + 540) % 360 - 180;
                // The turn is what gets snapped, not the angle it lands on. Snapping
                // the angle would move a press that has not travelled at all, and would
                // shift the grid a little further every lap for a step that does not
                // divide a circle.
                const v = (value + __tauler_snap(turned, step)) % 360;
                return __tauler_intents(on_change(__tauler_snap((v + 360) % 360, 0)));
            });
        }
        return __ui_knob(rendered);
    };
"#;

/// Puts both halves in place: the Rust renderer under `__ui_knob`, and the shim that
/// `@ui/knob` actually exports.
#[cfg(feature = "quickjs")]
fn register_knob(ctx: &rquickjs::Ctx<'_>) -> rquickjs::Result<()> {
    (__UI_ENTRY_KNOB.register)(ctx)?;
    ctx.eval::<(), _>(KNOB_SHIM_JS)
}

/// What `import { Knob } from "@ui/knob"` resolves to.
///
/// Registered in place of `__UI_ENTRY_KNOB`: the Rust half stays reachable from
/// JavaScript but is not importable, so there is only one `Knob` and it is the one
/// that accepts `on_change`.
#[cfg(feature = "quickjs")]
pub const __UI_ENTRY_KNOB_SHIM: crate::ui::registry::EsEntry = crate::ui::registry::EsEntry {
    module_path: "@ui/knob",
    export_name: "Knob",
    global_name: "__tauler_knob",
    register: register_knob,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::UiComponent;

    fn render(value: f64, on_drag: Option<Value>) -> Value {
        serde_json::to_value(Knob::render(KnobProps {
            value,
            on_drag,
            class: None,
        }))
        .expect("knob serialises")
    }

    fn rotation(rendered: &Value) -> &Value {
        &rendered["children"][0]["style"]["rotate"]
    }

    /// One element, and it is the dial: the angle comes from the pointer, so there is
    /// nothing inside to aim at (ADR 0020).
    #[test]
    fn the_whole_dial_is_one_capturing_element() {
        let handler = serde_json::json!({"$handler": 2});
        let rendered = render(30.0, Some(handler.clone()));
        assert_eq!(rendered["on_drag"], handler, "the handler is on the dial");
        assert!(
            rendered["children"][0].get("on_drag").is_none(),
            "nothing inside the dial handles anything"
        );
    }

    /// Only block-level elements are hittable (ADR 0018).
    #[test]
    fn the_dial_is_a_block_element() {
        assert_eq!(render(0.0, None)["type"], "div");
    }

    #[test]
    fn the_needle_is_rotated_by_the_value_in_degrees() {
        assert_eq!(rotation(&render(0.0, None)), "0deg");
        assert_eq!(rotation(&render(135.0, None)), "135deg");
    }

    /// No `min` and no `max`, so nothing is clamped away — an angle outside 0–360 is
    /// drawn where it points rather than pinned to an end.
    #[test]
    fn an_angle_past_a_full_turn_is_drawn_where_it_points() {
        assert_eq!(rotation(&render(-90.0, None)), "-90deg");
        assert_eq!(rotation(&render(450.0, None)), "450deg");
    }

    /// A knob with nothing to talk to still draws, rather than vanishing.
    #[test]
    fn without_a_handler_it_renders_but_captures_nothing() {
        let rendered = render(45.0, None);
        assert!(rendered.get("on_drag").is_none());
        assert_eq!(rotation(&rendered), "45deg");
    }

    /// The needle turns about the dial's centre, which is where `rotate` puts its
    /// origin — so the carrier has to fill the dial rather than wrap the needle.
    #[test]
    fn the_needle_rides_a_carrier_that_fills_the_dial() {
        let rendered = render(90.0, None);
        let carrier = &rendered["children"][0];
        let class = carrier["class"].as_str().expect("a class");
        assert!(class.contains("w-full") && class.contains("h-full"));
    }

    #[test]
    fn class_is_appended_to_the_dial() {
        let rendered = serde_json::to_value(Knob::render(KnobProps {
            value: 0.0,
            on_drag: None,
            class: Some("w-[64px]".into()),
        }))
        .unwrap();
        assert!(rendered["class"].as_str().unwrap().ends_with("w-[64px]"));
    }
}
