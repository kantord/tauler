//! Flattening the JSX factory's passthrough shape into the layout tree's own.
//!
//! `h` hands back `{ type, props: {...}, children }` — nested, because a caller's props
//! must not be able to collide with the wrapper's own `type` and `children` keys. Every
//! consumer downstream of it wants the flat shape instead. This is the one step between,
//! and it lives here rather than beside either engine because both engines need it: the
//! QuickJS `h` shim calls it per node, and so does the browser's (ADR 0027).

use serde_json::Value;

/// Bridges `optative_script::register_h`'s generic passthrough shape
/// (`{ type, props: {...}, children }`) to the flat shape tauler's own downstream
/// consumers expect (`{ type, ...props, children }`), recursively.
///
/// Props are written after the tag name, so **a `type` prop overrides the tag**.
/// That is deliberate, and load-bearing: it is what makes the long-hand
/// `<surface type="wallpaper">` spelling equivalent to `<wallpaper>` (see
/// `layout::parse_root_node`). The flip side is that the rule is global — any
/// node given a `type` prop becomes that type instead.
///
/// Only the value it is given and, below that, `children` are walked. Every other prop is
/// data and is left as written, even an object that happens to have `type` and `props`
/// keys — a stream row or a library's descriptor passed as a prop is not a node.
///
/// Text needs no special case: a string child stays a string child, and the layout
/// walker is what turns it into a text node (`docs/adr/0016`).
///
/// Called per node by the `h` shim — a Rust-backed component consumes its `children` prop
/// mid-render and needs the flat shape by then — and once more over the whole tree at the
/// end of `eval()`. By then there is usually nothing left to do; it stays because "usually"
/// is not "always", a node can reach the tree without passing through `h`, and the pass is
/// idempotent.
pub fn flatten_passthrough(value: Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.into_iter().map(flatten_passthrough).collect()),
        Value::Object(mut map) => {
            let is_passthrough_node = map.contains_key("type") && map.contains_key("props");
            if !is_passthrough_node {
                if let Some(children) = map.get_mut("children") {
                    *children = flatten_passthrough(std::mem::take(children));
                }
                return Value::Object(map);
            }

            let node_type = map.remove("type").unwrap_or(Value::Null);
            let props = map.remove("props").unwrap_or(Value::Null);
            let children = map
                .remove("children")
                .map(flatten_passthrough)
                .unwrap_or(Value::Array(Vec::new()));

            let mut flat = serde_json::Map::new();
            flat.insert("type".to_string(), node_type.clone());
            if let Value::Object(props_map) = props {
                for (k, v) in props_map {
                    flat.insert(k, v);
                }
            }

            flat.insert("children".to_string(), children);

            Value::Object(flat)
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A prop value is data, not a node, even when it happens to have `type` and `props`
    /// keys: a stream row or a library's descriptor passed as a prop must arrive as written.
    #[test]
    fn a_prop_shaped_like_a_node_is_left_untouched() {
        let row = json!({ "type": "deploy", "props": { "env": "prod" }, "at": 17 });
        let tree = json!({
            "type": "div",
            "data": row,
            "rows": [row, row],
            "deep": { "inner": { "x": row } },
            "children": [],
        });

        let flat = flatten_passthrough(tree.clone());

        assert_eq!(flat, tree);
    }

    /// `children` is still walked, at any depth, so a node that reached the tree without
    /// passing through `h` is still flattened.
    #[test]
    fn children_are_still_flattened() {
        let tree = json!({
            "type": "div",
            "children": [{
                "type": "span",
                "props": { "class": "x" },
                "children": [{ "type": "b", "props": {}, "children": ["a"] }],
            }],
        });

        let flat = flatten_passthrough(tree);

        assert_eq!(
            flat,
            json!({
                "type": "div",
                "children": [{
                    "type": "span",
                    "class": "x",
                    "children": [{ "type": "b", "children": ["a"] }],
                }],
            })
        );
    }
}
