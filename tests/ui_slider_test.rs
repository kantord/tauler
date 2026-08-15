//! `<Slider>` end to end: JSX in, a capturing element out, and a pointer position
//! turned into the value the module is told about.
//!
//! The unit tests in `ui::components::slider` cover the drawing. These cover the two
//! things only the whole pipeline shows: that a function handler survives evaluation
//! into the registry and can be called afterwards, and that the position the runtime
//! measures maps to the value the author asked for.

use std::collections::HashMap;
use tauler::config::FontConfig;
use tauler::jsx::{EvalOutput, JsxEvaluator};
use tauler::{hit_test, init_global_ctx};

fn evaluator(source: &str) -> JsxEvaluator {
    let ctx = serde_json::json!({
        "output": "DP-4", "dpi": 96.0,
        "screen_width": 1920, "screen_height": 1080
    });
    JsxEvaluator::new(source, ctx, None).expect("evaluator")
}

fn eval_with(e: &JsxEvaluator) -> EvalOutput {
    e.eval(&HashMap::new()).expect("eval")
}

/// The smallest layout that renders a slider: one panel, fixed width.
fn layout_with(slider: &str) -> String {
    format!(
        r#"import {{ Slider }} from "@ui/slider";
export default function render() {{
  return <root>
    <panel id="bar" x={{0}} y={{0}} width={{200}} height={{16}}>
      {slider}
    </panel>
  </root>;
}}"#
    )
}

/// What a pointer hit-tests against: `SurfaceSpec::content` is the panel's first child.
fn slider_node(out: &EvalOutput) -> serde_json::Value {
    out.layout["children"][0]["children"][0].clone()
}

const WIRED: &str = r#"<Slider value={0} min={0} max={100} step={10}
     on_change={v => ({ channel: "audio", event: { type: "setVolume", volume: v } })} />"#;

/// A handler id and a way to call it, which is all a drag needs.
fn wired() -> (JsxEvaluator, serde_json::Value, i64) {
    let e = evaluator(&layout_with(WIRED));
    let node = slider_node(&eval_with(&e));
    let id = node["on_drag"]["$handler"]
        .as_i64()
        .expect("a function handler");
    (e, node, id)
}

fn volume_at(e: &JsxEvaluator, id: i64, x: f64) -> f64 {
    let p = serde_json::json!({"x": x, "y": 8.0, "width": 200.0, "height": 16.0, "buttons": 1});
    e.invoke_handler(id, &p).expect("intents")[0]["event"]["volume"]
        .as_f64()
        .expect("a volume")
}

#[test]
fn the_track_is_one_element_that_captures() {
    let (_e, node, _id) = wired();
    assert!(
        node["children"]
            .as_array()
            .expect("children")
            .iter()
            .all(|c| c.get("on_drag").is_none() && c.get("on_click").is_none()),
        "the track handles the pointer; its parts handle nothing"
    );
}

/// The point of the registry: a function cannot cross the JSON boundary, so it stays
/// in JavaScript and the tree carries an id (ADR 0021).
#[test]
fn a_function_handler_survives_evaluation_and_can_be_called() {
    let (e, _node, id) = wired();
    let p = serde_json::json!({"x": 100.0, "y": 8.0, "width": 200.0, "height": 16.0});
    let intents = e.invoke_handler(id, &p).expect("handler returns intents");
    assert_eq!(intents[0]["channel"], "audio");
    assert_eq!(intents[0]["event"]["volume"], 50.0, "halfway is 50");
}

/// A handler is always dispatched as an array, whether it returned one or not.
#[test]
fn a_handler_returning_a_bare_intent_is_wrapped() {
    let (e, _node, id) = wired();
    let p = serde_json::json!({"x": 0.0, "y": 8.0, "width": 200.0, "height": 16.0});
    assert!(e.invoke_handler(id, &p).expect("intents").is_array());
}

/// `step` rounds the reported value, which is also what keeps a drag from sending a
/// message per pixel — repeats are skipped downstream (ADR 0020).
#[test]
fn the_reported_value_is_rounded_to_the_step() {
    let (e, _node, id) = wired();
    assert_eq!(volume_at(&e, id, 0.0), 0.0);
    assert_eq!(
        volume_at(&e, id, 150.0),
        80.0,
        "75% of the way, rounded to the nearest 10"
    );
    assert_eq!(volume_at(&e, id, 200.0), 100.0);
}

/// The runtime reports an unclamped position, so dragging past an end must pin rather
/// than run off the scale (ADR 0020).
#[test]
fn dragging_past_either_end_pins_to_that_end() {
    let (e, _node, id) = wired();
    assert_eq!(volume_at(&e, id, -500.0), 0.0, "far left of the track");
    assert_eq!(volume_at(&e, id, 9000.0), 100.0, "far right of the track");
}

#[test]
fn a_slider_without_on_change_renders_but_captures_nothing() {
    let e = evaluator(&layout_with(r#"<Slider value={40} min={0} max={100} />"#));
    let node = slider_node(&eval_with(&e));
    assert!(node.get("on_drag").is_none());
    assert_eq!(node["children"].as_array().expect("children").len(), 3);
}

/// The registry is rebuilt every tick rather than growing for the life of the process,
/// so the same tree yields the same ids each time.
#[test]
fn ids_are_reissued_each_tick_rather_than_growing_without_bound() {
    let e = evaluator(&layout_with(WIRED));
    let first = slider_node(&eval_with(&e))["on_drag"]["$handler"].clone();
    let second = slider_node(&eval_with(&e))["on_drag"]["$handler"].clone();
    assert_eq!(first, second);
}

/// The whole mechanism: a press resolves through the real layout to a box, and the
/// position within that box is what the handler is measured against.
#[test]
fn the_pressed_box_is_what_the_position_is_measured_against() {
    init_global_ctx(FontConfig::default());
    let (e, content, _) = wired();

    let hit = hit_test(&content, 200, 16, 1.0, 150.0, 8.0).expect("the track is hit");
    assert!(hit.on_drag.is_some(), "the track captures");
    assert_eq!(hit.rect.width, 200.0, "the box is the whole track");

    let pointer = hit.rect.pointer((150.0, 8.0), (150.0, 8.0), 1.0, 1);
    assert_eq!(pointer["x"], 150.0, "measured from the track's left edge");
    assert_eq!(pointer["buttons"], 1);

    let id = hit.on_drag.expect("captures")["$handler"]
        .as_i64()
        .expect("id");
    assert_eq!(
        e.invoke_handler(id, &pointer).expect("intents")[0]["event"]["volume"],
        80.0
    );
}
