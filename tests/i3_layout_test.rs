//! `<I3Layout>` end to end: JSX in, positioned `<panel>` nodes and a gap
//! declaration out.
//!
//! The unit tests in `ui::components::i3_layout` cover the arithmetic. These
//! cover the wiring — that the shim reaches the Rust component, that the emitted
//! panels reach `parse_root_node` as real surfaces, and that the gaps reach the
//! module despite being registered after the children.

use std::collections::HashMap;
use tauler::jsx::{EvalOutput, JsxEvaluator};

fn eval(source: &str) -> EvalOutput {
    let ctx = serde_json::json!({
        "output": "DP-4", "dpi": 96.0,
        "screen_width": 1920, "screen_height": 1080
    });
    JsxEvaluator::new(source, ctx, None)
        .expect("evaluator")
        .eval(&HashMap::new())
        .expect("eval")
}

const LAYOUT: &str = r#"export default function render() {
  return <root>
    <I3Layout module="/usr/bin/tauler-i3">
      <Panel id="sidebar" anchor="left" size={300}>
        <div class="side" />
      </Panel>
      <Panel id="topbar" anchor="top" size={50}>
        <div class="top" />
      </Panel>
    </I3Layout>
  </root>;
}"#;

#[test]
fn emitted_panels_are_real_surfaces_with_computed_geometry() {
    let specs = tauler::parse_root_node(&eval(LAYOUT).layout).expect("root parses");

    let sidebar = specs.iter().find(|s| s.id == "sidebar").expect("sidebar");
    assert_eq!((sidebar.x, sidebar.y), (0, 0));
    assert_eq!((sidebar.width, sidebar.height), (300, 1080));

    let topbar = specs.iter().find(|s| s.id == "topbar").expect("topbar");
    assert_eq!(
        (topbar.x, topbar.y),
        (300, 0),
        "the top bar starts right of the sidebar declared before it"
    );
    assert_eq!((topbar.width, topbar.height), (1620, 50));
}

/// The reason `registerModule` had to stop dropping later registrations:
/// `<I3Layout>` can only know the gaps after its children have been evaluated,
/// so it always registers after the `<Module>` inside them.
#[test]
fn the_gaps_reach_the_module_even_though_they_are_registered_last() {
    let out = eval(&LAYOUT.replace(
        r#"<div class="side" />"#,
        r#"<Module bin="/usr/bin/tauler-i3">{(d, e) => <div class="side" />}</Module>"#,
    ));
    let (_, props) = out
        .module_calls
        .iter()
        .find(|(bin, _)| bin == "/usr/bin/tauler-i3")
        .expect("the module must be registered");
    assert_eq!(props["gaps"]["left"].as_u64(), Some(300));
    assert_eq!(props["gaps"]["top"].as_u64(), Some(50));
    assert_eq!(props["gaps"]["right"].as_u64(), Some(0));
}

#[test]
fn panel_children_survive_the_round_trip() {
    let specs = tauler::parse_root_node(&eval(LAYOUT).layout).unwrap();
    let sidebar = specs.iter().find(|s| s.id == "sidebar").unwrap();
    assert_eq!(sidebar.content["class"], "side");
}

/// Without `module`, `<I3Layout>` is pure geometry and registers nothing.
#[test]
fn omitting_the_module_registers_nothing() {
    let out = eval(&LAYOUT.replace(r#" module="/usr/bin/tauler-i3""#, ""));
    assert!(out.module_calls.is_empty());
    assert_eq!(tauler::parse_root_node(&out.layout).unwrap().len(), 2);
}
