//! `<Knob>` end to end: JSX in, a capturing dial out, and two pointer positions turned
//! into the angle the module is told about.
//!
//! The unit tests in `ui::components::knob` cover the drawing. These cover what only
//! the whole pipeline shows: that the turn is measured from the press rather than from
//! the dial, and that the trigonometry agrees with where the needle is pointing.

use std::collections::HashMap;
use tauler::config::FontConfig;
use tauler::jsx::{EvalOutput, JsxEvaluator};
use tauler::{hit_test, init_global_ctx};

/// The dial is 40×40, so its centre — where every bearing is measured from — is (20, 20).
const CENTRE: f64 = 20.0;

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

fn layout_with(knob: &str) -> String {
    format!(
        r#"import {{ Knob }} from "@ui/knob";
export default function render() {{
  return <root>
    <panel id="bar" x={{0}} y={{0}} width={{40}} height={{40}}>
      {knob}
    </panel>
  </root>;
}}"#
    )
}

/// What a pointer hit-tests against: `SurfaceSpec::content` is the panel's first child.
fn knob_node(out: &EvalOutput) -> serde_json::Value {
    out.layout["children"][0]["children"][0].clone()
}

fn wired(value: f64, step: f64) -> (JsxEvaluator, serde_json::Value, i64) {
    let src = layout_with(&format!(
        r#"<Knob value={{{value}}} step={{{step}}}
             on_change={{deg => ({{ channel: "audio", event: {{ type: "setAngle", deg }} }})}} />"#
    ));
    let e = evaluator(&src);
    let node = knob_node(&eval_with(&e));
    let id = node["on_drag"]["$handler"]
        .as_i64()
        .expect("a function handler");
    (e, node, id)
}

/// A point on the dial at `bearing` degrees from its centre — 0 is up, and the angle
/// grows clockwise, which is how the knob reads it.
fn at(bearing: f64) -> (f64, f64) {
    let r = 16.0;
    let rad = bearing.to_radians();
    (CENTRE + r * rad.sin(), CENTRE - r * rad.cos())
}

/// The angle the module is sent when the pointer goes from one point to another, or
/// `None` when the drag reports nothing at all.
fn drag(e: &JsxEvaluator, id: i64, press: (f64, f64), to: (f64, f64)) -> Option<f64> {
    let p = serde_json::json!({
        "x": to.0, "y": to.1, "press_x": press.0, "press_y": press.1,
        "width": 40.0, "height": 40.0, "buttons": 1,
    });
    let intents = e.invoke_handler(id, &p)?;
    Some(
        intents[0]["event"]["deg"]
            .as_f64()
            .expect("an angle in the intent"),
    )
}

/// Press at one bearing, drag to another, and report the angle the module is sent.
fn turn(e: &JsxEvaluator, id: i64, from: f64, to: f64) -> f64 {
    drag(e, id, at(from), at(to)).expect("a turn on the rim reports something")
}

/// The reason the press point is in the payload at all: pressing the dial anywhere is a
/// turn of nothing, so the needle never jumps to meet the pointer (ADR 0022).
#[test]
fn pressing_anywhere_on_the_dial_turns_it_by_nothing() {
    let (e, _node, id) = wired(30.0, 1.0);
    for bearing in [0.0, 90.0, 200.0, 359.0] {
        assert_eq!(
            turn(&e, id, bearing, bearing),
            30.0,
            "pressing at {bearing}° leaves the angle alone"
        );
    }
}

/// The displacement is what counts, not where either point is: the same sweep from a
/// different place on the dial gives the same turn.
#[test]
fn the_angle_moves_by_the_sweep_not_by_where_the_pointer_is() {
    let (e, _node, id) = wired(0.0, 1.0);
    assert_eq!(turn(&e, id, 0.0, 90.0), 90.0, "a quarter turn from the top");
    assert_eq!(turn(&e, id, 90.0, 180.0), 90.0, "and from the side");
    assert_eq!(turn(&e, id, 270.0, 0.0), 90.0, "and across the top");
}

/// Anticlockwise is a negative sweep, and lands where a positive one to the same place
/// would: there is no min and no max to run out of.
#[test]
fn turning_back_is_a_negative_sweep() {
    let (e, _node, id) = wired(100.0, 1.0);
    assert_eq!(turn(&e, id, 0.0, 270.0), 10.0, "70° back from 100°");
    assert_eq!(turn(&e, id, 0.0, 90.0), 190.0);
}

/// The reported angle wraps rather than growing without bound, so a knob that is turned
/// all day stays a number a module can read.
#[test]
fn the_reported_angle_wraps_into_a_full_circle() {
    let (e, _node, id) = wired(350.0, 1.0);
    assert_eq!(turn(&e, id, 0.0, 30.0), 20.0, "350 + 30 comes back as 20");
    let (e, _node, id) = wired(10.0, 1.0);
    assert_eq!(turn(&e, id, 0.0, 330.0), 340.0, "10 - 30 comes back as 340");
}

/// Sweeping past the far side of the dial is where a two-point reading could flip sign.
/// It does — and the wrap absorbs it, so the angle stays continuous all the way round.
#[test]
fn sweeping_past_the_far_side_stays_continuous() {
    let (e, _node, id) = wired(0.0, 1.0);
    assert_eq!(turn(&e, id, 0.0, 179.0), 179.0);
    assert_eq!(turn(&e, id, 0.0, 181.0), 181.0, "not -179");
    assert_eq!(turn(&e, id, 0.0, 359.0), 359.0, "not -1");
}

/// `step` rounds the turn, which is also what keeps it from sending a message per pixel
/// — repeats are skipped downstream (ADR 0020).
#[test]
fn the_turn_is_rounded_to_the_step() {
    let (e, _node, id) = wired(0.0, 15.0);
    assert_eq!(turn(&e, id, 0.0, 22.0), 15.0);
    assert_eq!(turn(&e, id, 0.0, 23.0), 30.0);
}

/// The step rounds how far you turned, not where you land. Rounding the angle instead
/// would move a knob nobody has turned, whenever its value sat off the step's grid.
#[test]
fn a_press_that_has_not_moved_reports_the_angle_it_was_drawn_at() {
    for (value, step) in [(3.0, 5.0), (37.0, 15.0), (0.5, 10.0)] {
        let (e, _node, id) = wired(value, step);
        assert_eq!(
            turn(&e, id, 40.0, 40.0),
            value,
            "value {value} with step {step} is not on the step's grid"
        );
    }
}

/// Same reason, one lap further on: a step that does not divide a circle would shift its
/// grid a little every time round if the angle were what got rounded.
#[test]
fn a_step_that_does_not_divide_a_circle_does_not_drift() {
    let (e, _node, id) = wired(353.0, 7.0);
    assert_eq!(turn(&e, id, 0.0, 10.0), 0.0, "353 + one 7° step wraps to 0");
    let (e, _node, id) = wired(0.0, 7.0);
    assert_eq!(
        turn(&e, id, 0.0, 10.0),
        7.0,
        "and the next lap starts clean"
    );
}

/// A bearing about the centre is undefined *at* the centre and swings through tens of
/// degrees per pixel near it, so the hub reports nothing rather than leaping — which is
/// the "point-at" behaviour this design exists to avoid (ADR 0022).
#[test]
fn a_press_in_the_hub_reports_nothing_however_far_it_is_dragged() {
    let (e, _node, id) = wired(30.0, 1.0);
    let centre = (CENTRE, CENTRE);
    assert_eq!(
        drag(&e, id, centre, (CENTRE + 1.0, CENTRE)),
        None,
        "1px off"
    );
    assert_eq!(drag(&e, id, centre, at(90.0)), None, "dragged to the rim");
    assert_eq!(
        drag(&e, id, (CENTRE + 0.5, CENTRE), (CENTRE, CENTRE + 0.5)),
        None,
        "a sub-pixel move either side of the centre is not a quarter turn"
    );
}

/// The other end of the same reading: a drag that starts on the rim and wanders into the
/// hub holds still until it comes back out.
#[test]
fn a_drag_that_wanders_into_the_hub_reports_nothing_until_it_leaves() {
    let (e, _node, id) = wired(0.0, 1.0);
    assert_eq!(
        drag(&e, id, at(0.0), (CENTRE, CENTRE)),
        None,
        "into the hub"
    );
    assert_eq!(
        drag(&e, id, at(0.0), at(90.0)),
        Some(90.0),
        "and reports again on the way out"
    );
}

#[test]
fn a_knob_without_on_change_renders_but_captures_nothing() {
    let e = evaluator(&layout_with(r#"<Knob value={45} />"#));
    let node = knob_node(&eval_with(&e));
    assert!(node.get("on_drag").is_none());
    assert_eq!(node["children"][0]["style"]["rotate"], "45deg");
}

/// The whole mechanism: a press resolves through the real layout to a box, and both
/// points are measured against that box.
#[test]
fn the_pressed_box_is_what_both_points_are_measured_against() {
    init_global_ctx(FontConfig::default());
    let (e, content, _) = wired(0.0, 1.0);

    let hit = hit_test(&content, 40, 40, 1.0, 20.0, 4.0).expect("the dial is hit");
    assert!(hit.on_drag.is_some(), "the dial captures");
    assert_eq!(hit.rect.width, 40.0, "the box is the whole dial");

    // Pressed at the top of the dial, dragged to its right edge: a quarter turn.
    let pointer = hit.rect.pointer((36.0, 20.0), (20.0, 4.0), 1.0, 1);
    assert_eq!(pointer["press_x"], 20.0, "from the dial's own left edge");
    assert_eq!(pointer["press_y"], 4.0);

    let id = hit.on_drag.expect("captures")["$handler"]
        .as_i64()
        .expect("id");
    assert_eq!(
        e.invoke_handler(id, &pointer).expect("intents")[0]["event"]["deg"],
        90.0
    );
}
