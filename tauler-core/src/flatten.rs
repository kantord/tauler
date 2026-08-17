//! Flattening the JSX factory's passthrough shape into the layout tree's own.
//!
//! `h` hands back `{ type, props: {...}, children }` — nested, because a caller's props
//! must not be able to collide with the wrapper's own `type` and `children` keys. Every
//! consumer downstream of it wants the flat shape instead. This is the one step between,
//! and it lives here rather than beside either engine because both engines need it: the
//! QuickJS `h` shim calls it per node, and so does the browser's (ADR 0025).

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
                for (_, v) in map.iter_mut() {
                    *v = flatten_passthrough(std::mem::take(v));
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
