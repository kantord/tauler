//! Pulling Items out of an evaluated layout tree.
//!
//! A Unit is declared by a `unit()` call and used as a component, so an Item is a
//! node whose `type` is the Unit descriptor rather than a tag name — see ADR 0033.
//! Its hooks are functions and do not survive serialization; what is left in the
//! tree is the Unit's id and the Item's own props, which is exactly what crosses
//! to the reconciler (ADR 0034).

use serde_json::Value;

/// One Item, as it crosses out of the render runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// Which Unit this is an Item of. Assigned by `unit()`, per runtime.
    pub unit_id: u64,
    /// The Item's own props. `type` and `children` are structure, not props, so
    /// neither is here.
    pub props: Value,
}

/// Every Item in `tree`, in document order.
pub fn collect_items(tree: &Value) -> Vec<Item> {
    let mut items = Vec::new();
    walk(tree, &mut items);
    items
}

fn walk(node: &Value, items: &mut Vec<Item>) {
    // A text node is a bare value; only an object can be an Item or have children.
    let Some(obj) = node.as_object() else {
        return;
    };
    if let Some(unit_id) = unit_id_of(obj.get("type")) {
        items.push(Item {
            unit_id,
            props: props_of(obj),
        });
    }
    if let Some(children) = obj.get("children").and_then(Value::as_array) {
        for child in children {
            walk(child, items);
        }
    }
}

/// An Item's `type` is the Unit descriptor `unit()` returned, tagged and numbered.
/// An ordinary node's is a tag name.
fn unit_id_of(ty: Option<&Value>) -> Option<u64> {
    let ty = ty?.as_object()?;
    if ty.get(optative_script::tags::ESTO_KIND)?.as_bool() != Some(true) {
        return None;
    }
    ty.get(optative_script::tags::ESTO_ID)?.as_u64()
}

fn props_of(node: &serde_json::Map<String, Value>) -> Value {
    Value::Object(
        node.iter()
            .filter(|(k, _)| k.as_str() != "type" && k.as_str() != "children")
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn unit_type(id: u64) -> Value {
        json!({ "__estoKind": true, "__estoId": id })
    }

    #[test]
    fn collects_an_item_from_anywhere_in_the_tree() {
        let tree = json!({
            "type": "root",
            "children": [
                { "type": "panel", "children": [
                    { "type": unit_type(1), "entity": "light.desk", "children": [] }
                ]}
            ]
        });
        assert_eq!(
            collect_items(&tree),
            vec![Item {
                unit_id: 1,
                props: json!({ "entity": "light.desk" }),
            }]
        );
    }

    #[test]
    fn ordinary_nodes_are_not_items() {
        let tree = json!({
            "type": "root",
            "children": [{ "type": "span", "class": "flex", "children": ["hi"] }]
        });
        assert_eq!(collect_items(&tree), vec![]);
    }

    /// Document order, so two Items of one Unit reach the reconciler in the order
    /// the layout file wrote them.
    #[test]
    fn items_come_back_in_document_order() {
        let tree = json!({
            "type": "root",
            "children": [
                { "type": unit_type(2), "n": 1, "children": [] },
                { "type": "panel", "children": [{ "type": unit_type(2), "n": 2, "children": [] }] },
                { "type": unit_type(7), "n": 3, "children": [] },
            ]
        });
        let ids: Vec<(u64, i64)> = collect_items(&tree)
            .iter()
            .map(|i| (i.unit_id, i.props["n"].as_i64().unwrap()))
            .collect();
        assert_eq!(ids, vec![(2, 1), (2, 2), (7, 3)]);
    }

    /// A text node is a bare string, not an object — the walk must not assume
    /// every child has a shape.
    #[test]
    fn text_children_are_walked_past() {
        let tree = json!({ "type": "div", "children": ["hello", 3, null] });
        assert_eq!(collect_items(&tree), vec![]);
    }

    /// The end-to-end claim: what a real evaluation puts in the tree is what this
    /// module reads. The two halves are written a long way apart, and a change to
    /// either that the other does not follow is invisible in the unit tests above.
    #[test]
    fn items_are_collected_from_a_real_evaluated_layout() {
        let output = crate::jsx::JsxEvaluator::new(
            r#"
            const Light = unit({
              key: (i) => i.entity,
              value: (i) => i.state,
              reconciler: optativeSet({ observe: () => [] }),
              enter: (i) => `on ${i.entity}`,
            });
            export default function render() {
              return <root>
                <panel id="sidebar"><Light entity="light.desk" state="on" /></panel>
              </root>;
            }"#,
            serde_json::Value::Null,
            None,
        )
        .unwrap()
        .eval(&std::collections::HashMap::new())
        .unwrap();

        let items = collect_items(&output.layout);
        assert_eq!(items.len(), 1, "got {items:?}");
        assert_eq!(items[0].props["entity"], "light.desk");
        assert_eq!(items[0].props["state"], "on");
        assert!(
            items[0].props.get("type").is_none(),
            "the Unit descriptor is structure, not a prop"
        );
    }
}
