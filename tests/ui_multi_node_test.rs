//! A Rust component may return several nodes, the way a JSX fragment does.
//!
//! This is what lets a wrapper component emit N `<panel>`s from one element —
//! `<I3Layout>` computing positions for its children, say.

use tauler::jsx::JsxEvaluator;

fn eval(source: &str) -> serde_json::Value {
    JsxEvaluator::new(source, serde_json::Value::Null, None)
        .expect("evaluator")
        .eval(&std::collections::HashMap::new())
        .expect("eval")
        .layout
}

#[test]
fn a_component_returning_a_vec_splices_its_nodes_into_the_parent() {
    let layout = eval(
        r#"import { PairWidget } from '@ui/test-multi';
export default function render() {
  return <container tw="host"><PairWidget /></container>;
}"#,
    );
    let kids = layout["children"].as_array().expect("children array");
    let tws: Vec<&str> = kids.iter().filter_map(|c| c["tw"].as_str()).collect();
    assert_eq!(
        tws,
        vec!["pair-first", "pair-second"],
        "both nodes must land as siblings, not nested or wrapped"
    );
}

/// The case that matters: surfaces must reach `<root>` as direct children, or
/// `parse_root_node` will not see them.
#[test]
fn spliced_nodes_are_visible_to_the_root_parser() {
    let layout = eval(
        r#"export default function render() {
  return <root>
    <panel id="a" width={10} height={10} />
    <panel id="b" width={10} height={10} />
  </root>;
}"#,
    );
    let specs = tauler::parse_root_node(&layout).expect("root parses");
    assert_eq!(specs.len(), 2);
}
