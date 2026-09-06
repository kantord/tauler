//! `<Workspaces>` end to end: JSX in, a bordered frame of `<panel>`s out.
//!
//! `src/workspaces.rs`'s own tests cover the arithmetic against raw JSON. These cover
//! the wiring — that the `<I3Layout>` shim reaches `__workspaces_layout`, offsets its
//! panels by the gaps already consumed by earlier `<Panel>`s, and merges the frame's
//! gaps into what the module sees — the same thing `tests/i3_layout_test.rs` checks for
//! plain `<Panel>`s.

use std::collections::HashMap;
use tauler::jsx::{EvalOutput, JsxEvaluator};

fn eval(source: &str) -> EvalOutput {
    tauler::init_global_ctx(tauler::config::FontConfig::default());
    let ctx = serde_json::json!({
        "output": "DP-4", "dpi": 96.0,
        "screen_width": 1920, "screen_height": 1080
    });
    JsxEvaluator::new(source, ctx, None)
        .expect("evaluator")
        .eval(&HashMap::new())
        .expect("eval")
}

/// A sidebar (`anchor="left" size={300}`) leaves a 1620x1080 free rect for
/// `<Workspaces>`. Its wrapper draws a 10px border around `<Contents/>` on every
/// side using plain flex rows/columns, the same shape `src/workspaces.rs`'s own
/// `bordered_wrapper()` fixture uses.
const LAYOUT: &str = r#"export default function render() {
  return <root>
    <I3Layout module="/usr/bin/tauler-i3">
      <Panel id="sidebar" anchor="left" size={300}>
        <div class="side" />
      </Panel>
      <Workspaces>
        {(Contents) => (
          <div class="flex flex-col" style={{width: 1620, height: 1080}}>
            <div style={{height: 10}} />
            <div class="flex flex-row flex-1">
              <div style={{width: 10}} />
              <Contents class="flex-1 h-full" />
              <div style={{width: 10}} />
            </div>
            <div style={{height: 10}} />
          </div>
        )}
      </Workspaces>
    </I3Layout>
  </root>;
}"#;

#[test]
fn the_frame_panels_are_positioned_relative_to_the_whole_screen() {
    let specs = tauler::parse_root_node(&eval(LAYOUT).layout).expect("root parses");
    assert_eq!(specs.len(), 5, "sidebar plus four frame panels");

    let rect = |id: &str| {
        let s = specs
            .iter()
            .find(|s| s.id == id)
            .unwrap_or_else(|| panic!("no panel {id}"));
        (s.x, s.y, s.width, s.height)
    };

    assert_eq!(rect("sidebar"), (0, 0, 300, 1080));
    assert_eq!(
        rect("workspaces-top"),
        (300, 0, 1620, 10),
        "offset by the sidebar's gap, spanning the free rect's full width"
    );
    assert_eq!(rect("workspaces-bottom"), (300, 1070, 1620, 10));
    assert_eq!(rect("workspaces-left"), (300, 10, 10, 1060));
    assert_eq!(rect("workspaces-right"), (1910, 10, 10, 1060));
}

#[test]
fn the_frames_thickness_is_added_to_the_sidebars_gaps() {
    let out = eval(LAYOUT);
    let (_, props) = out
        .module_calls
        .iter()
        .find(|(bin, _)| bin == "/usr/bin/tauler-i3")
        .expect("the module must be registered");
    assert_eq!(
        props["gaps"]["left"].as_u64(),
        Some(310),
        "300 sidebar + 10 frame"
    );
    assert_eq!(props["gaps"]["right"].as_u64(), Some(10));
    assert_eq!(props["gaps"]["top"].as_u64(), Some(10));
    assert_eq!(props["gaps"]["bottom"].as_u64(), Some(10));
}

/// A `<Workspaces>` whose wrapper fills the whole free rect (no border at all)
/// degrades to zero panels and no extra gap, same as `src/workspaces.rs`'s own
/// `a_wrapper_filling_itself_produces_no_panels`.
#[test]
fn a_borderless_wrapper_adds_no_panels_or_gaps() {
    let layout = LAYOUT.replace(
        r#"<div class="flex flex-col" style={{width: 1620, height: 1080}}>
            <div style={{height: 10}} />
            <div class="flex flex-row flex-1">
              <div style={{width: 10}} />
              <Contents class="flex-1 h-full" />
              <div style={{width: 10}} />
            </div>
            <div style={{height: 10}} />
          </div>"#,
        r#"<Contents style={{width: 1620, height: 1080}} />"#,
    );
    let specs = tauler::parse_root_node(&eval(&layout).layout).expect("root parses");
    assert_eq!(specs.len(), 1, "only the sidebar — no frame panels at all");

    let out = eval(&layout);
    let (_, props) = out
        .module_calls
        .iter()
        .find(|(bin, _)| bin == "/usr/bin/tauler-i3")
        .expect("the module must be registered");
    assert_eq!(
        props["gaps"]["left"].as_u64(),
        Some(300),
        "just the sidebar"
    );
    assert_eq!(props["gaps"]["right"].as_u64(), Some(0));
}
