//! Turning the layout file's tree into markup.
//!
//! The counterpart of `tauler::layout::html`, which turns the same tree into takumi nodes.
//! This walk writes tag names and attributes and lets the browser do the rest, presets
//! included — those come from its own user-agent stylesheet, not from the table vendored in
//! `layout::presets` (ADR 0024).
//!
//! Three things about the output:
//!
//! - **Nothing is resolved here.** `class` is written verbatim; theme tokens were already
//!   rewritten by [`crate::theme::resolver`] and Tailwind compiles the rest.
//! - **Every element carries `data-tauler-path`** — the same child-index path
//!   `layout::html` binds handlers to (ADR 0018), which is what lets the geometry check
//!   pair the two box trees. Handler-carrying elements also get `data-tauler-on`, so a
//!   delegated listener can find the nearest with `closest`.
//! - **Attributes are an allowlist.** A node's props are arbitrary, and writing them all
//!   out would put JSON in the DOM and hand the browser things that look like handlers.

use std::fmt::Write as _;

use serde::Serialize;
use serde_json::Value;

/// Cap on element nesting.
///
/// The same 32 as `layout::html::MAX_DEPTH`, so a layout either renders in both renderers
/// or fails in both. This walk is not itself stack-hungry.
pub const MAX_DEPTH: usize = 32;

/// Tags whose entire subtree is dropped rather than written.
///
/// The same five as `layout::html::DROPPED_TAGS`, and the reason this walk is Rust rather
/// than JavaScript in the page: `script` is on the list, and a second copy of it in another
/// language is a security-relevant duplicate (ADR 0024).
const DROPPED_TAGS: [&str; 5] = ["head", "meta", "link", "style", "script"];

/// Tags written without a closing tag, per the HTML spec's void-element list.
const VOID_TAGS: [&str; 14] = [
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// Style properties whose numeric values carry no unit. Everything else gets `px` — a bare
/// number in a layout file is a **Logical pixel**.
const UNITLESS_PROPERTIES: [&str; 9] = [
    "opacity",
    "z-index",
    "flex",
    "flex-grow",
    "flex-shrink",
    "line-height",
    "font-weight",
    "order",
    "aspect-ratio",
];

/// Props written through as HTML attributes, under the same name.
const ATTRIBUTES: [&str; 6] = ["id", "class", "src", "alt", "title", "role"];

/// Props written through only when numeric, as HTML's own presentational attributes.
const NUMERIC_ATTRIBUTES: [&str; 2] = ["width", "height"];

/// A resource tauler binds for one render. A page has no Wallpaper to slice, so such an
/// `<img>` is written without its `src` rather than left to resolve to a broken image.
const TAULER_SCHEME: &str = "tauler:";

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum DomError {
    #[error("layout nesting exceeded {0} levels")]
    MaxDepthExceeded(usize),
    #[error("a node has no `type`")]
    MissingType,
    #[error("expected a <dom> node at the root, found <{0}>")]
    NotADomSurface(String),
    #[error("a layout tree may hold only one <dom> node")]
    MultipleDomSurfaces,
}

/// What a web render hands back.
///
/// Tagged so a second kind of output is an added variant rather than a breaking change, and
/// so glue meeting one it does not know can say so instead of reaching for a missing field.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Output {
    /// Markup for a single Mount node.
    Dom { dom: String },
}

/// Render an evaluated layout tree whose root is a `<dom>` shell node.
///
/// A `<root>` wrapper is unwrapped, since a layout file that also declares desktop surfaces
/// produces one — but it may hold exactly one `<dom>`.
pub fn render_output(root: &Value) -> Result<Output, DomError> {
    let node = unwrap_root(root)?;
    let tag = node
        .get("type")
        .and_then(Value::as_str)
        .ok_or(DomError::MissingType)?;
    if tag != "dom" {
        return Err(DomError::NotADomSurface(tag.to_string()));
    }
    Ok(Output::Dom {
        dom: render_children_of(node)?,
    })
}

/// The `<dom>` node inside `root`, or `root` itself when it is already one.
fn unwrap_root(root: &Value) -> Result<&Value, DomError> {
    let tag = root
        .get("type")
        .and_then(Value::as_str)
        .ok_or(DomError::MissingType)?;
    if tag != "root" {
        return Ok(root);
    }
    let mut found: Option<&Value> = None;
    for child in children_of(root) {
        if child.get("type").and_then(Value::as_str) == Some("dom") {
            if found.is_some() {
                return Err(DomError::MultipleDomSurfaces);
            }
            found = Some(child);
        }
    }
    found.ok_or_else(|| DomError::NotADomSurface("root".to_string()))
}

/// Markup for the children of a Dom surface.
///
/// Paths are numbered as `layout::html::build_tree` numbers them, so a path means the same
/// node in both renderers: a lone child is the root and has an empty path.
fn render_children_of(node: &Value) -> Result<String, DomError> {
    let kids: Vec<&Value> = children_of(node).collect();
    let mut out = String::new();
    let mut path = Vec::new();

    if kids.len() == 1 {
        write_node(kids[0], 0, &mut path, &mut out)?;
        return Ok(out);
    }
    let mut emitted = 0usize;
    for kid in kids {
        write_sibling(kid, 0, &mut emitted, &mut path, &mut out)?;
    }
    Ok(out)
}

/// A node's children, whatever shape the evaluator left them in.
fn children_of(node: &Value) -> impl Iterator<Item = &Value> {
    match node.get("children") {
        Some(Value::Array(items)) => Box::new(items.iter()) as Box<dyn Iterator<Item = &Value>>,
        Some(other) => Box::new(std::iter::once(other)),
        None => Box::new(std::iter::empty()),
    }
}

/// Write one child, giving it the next sibling index if it produces anything.
///
/// An array child is spliced in at this level rather than nesting, so `{items.map(...)}`
/// numbers its elements as siblings of whatever surrounds them.
fn write_sibling(
    value: &Value,
    depth: usize,
    emitted: &mut usize,
    path: &mut Vec<usize>,
    out: &mut String,
) -> Result<(), DomError> {
    if let Value::Array(items) = value {
        for item in items {
            write_sibling(item, depth, emitted, path, out)?;
        }
        return Ok(());
    }
    let before = out.len();
    path.push(*emitted);
    let result = write_node(value, depth, path, out);
    path.pop();
    result?;
    if out.len() != before {
        *emitted += 1;
    }
    Ok(())
}

fn write_node(
    value: &Value,
    depth: usize,
    path: &mut Vec<usize>,
    out: &mut String,
) -> Result<(), DomError> {
    match value {
        // A bare value in the tree is a text node, and the only thing that makes one.
        Value::String(s) => {
            escape_text(s, out);
            Ok(())
        }
        Value::Number(n) => {
            escape_text(&n.to_string(), out);
            Ok(())
        }
        // `optative-script` drops `null`, `undefined` and `false` while flattening;
        // `true` survives that filter, and rendering the word "true" is nobody's intent.
        Value::Bool(_) | Value::Null => Ok(()),
        Value::Array(items) => {
            let mut emitted = 0usize;
            for item in items {
                write_sibling(item, depth, &mut emitted, path, out)?;
            }
            Ok(())
        }
        Value::Object(_) => write_element(value, depth, path, out),
    }
}

fn write_element(
    value: &Value,
    depth: usize,
    path: &mut Vec<usize>,
    out: &mut String,
) -> Result<(), DomError> {
    if depth >= MAX_DEPTH {
        return Err(DomError::MaxDepthExceeded(MAX_DEPTH));
    }
    let tag = value
        .get("type")
        .and_then(Value::as_str)
        .ok_or(DomError::MissingType)?;
    if DROPPED_TAGS.contains(&tag) {
        return Ok(());
    }

    let _ = write!(out, "<{tag}");
    write_attributes(value, path, out);
    out.push('>');

    if VOID_TAGS.contains(&tag) {
        return Ok(());
    }

    let mut emitted = 0usize;
    for kid in children_of(value) {
        write_sibling(kid, depth + 1, &mut emitted, path, out)?;
    }
    let _ = write!(out, "</{tag}>");
    Ok(())
}

fn write_attributes(value: &Value, path: &[usize], out: &mut String) {
    for name in ATTRIBUTES {
        if let Some(v) = value.get(name).and_then(Value::as_str) {
            if name == "src" && v.starts_with(TAULER_SCHEME) {
                continue;
            }
            out.push(' ');
            out.push_str(name);
            out.push_str("=\"");
            escape_attribute(v, out);
            out.push('"');
        }
    }
    for name in NUMERIC_ATTRIBUTES {
        if let Some(n) = value.get(name).and_then(Value::as_f64) {
            let _ = write!(out, " {name}=\"{}\"", trim_float(n));
        }
    }
    if let Some(Value::Object(style)) = value.get("style") {
        let css = style_to_css(style);
        if !css.is_empty() {
            out.push_str(" style=\"");
            escape_attribute(&css, out);
            out.push('"');
        }
    }

    let _ = write!(out, " data-tauler-path=\"{}\"", format_path(path));

    let mut handlers: Vec<&str> = Vec::new();
    if value.get("on_click").is_some_and(|v| !v.is_null()) {
        handlers.push("click");
    }
    if value.get("on_drag").is_some_and(|v| !v.is_null()) {
        handlers.push("drag");
    }
    if !handlers.is_empty() {
        let _ = write!(out, " data-tauler-on=\"{}\"", handlers.join(" "));
    }
}

fn format_path(path: &[usize]) -> String {
    path.iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(".")
}

/// A style object as a CSS declaration list. Keys arrive camelCased and leave kebab-cased.
fn style_to_css(style: &serde_json::Map<String, Value>) -> String {
    let mut out = String::new();
    for (key, value) in style {
        let property = kebab_case(key);
        let Some(rendered) = css_value(&property, value) else {
            continue;
        };
        if !out.is_empty() {
            out.push(' ');
        }
        let _ = write!(out, "{property}: {rendered};");
    }
    out
}

fn css_value(property: &str, value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => {
            let n = n.as_f64()?;
            if UNITLESS_PROPERTIES.contains(&property) {
                Some(trim_float(n))
            } else {
                Some(format!("{}px", trim_float(n)))
            }
        }
        _ => None,
    }
}

/// `12.0` as `12`. CSS accepts both, but a trailing `.0` makes two identical renders
/// compare unequal.
fn trim_float(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

fn kebab_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for c in s.chars() {
        if c.is_ascii_uppercase() {
            out.push('-');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

fn escape_text(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
}

fn escape_attribute(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn dom(children: Value) -> Value {
        json!({ "type": "dom", "children": children })
    }

    fn render(children: Value) -> String {
        match render_output(&dom(children)).expect("renders") {
            Output::Dom { dom } => dom,
        }
    }

    #[test]
    fn an_element_becomes_a_tag_of_the_same_name() {
        assert_eq!(
            render(json!([{ "type": "div" }])),
            r#"<div data-tauler-path=""></div>"#
        );
    }

    #[test]
    fn class_is_written_through_verbatim() {
        // The browser resolves the Tailwind, so nothing here may touch the string.
        let html = render(json!([{ "type": "div", "class": "flex gap-[4px] bg-[#1a1a1a]" }]));
        assert!(
            html.contains(r#"class="flex gap-[4px] bg-[#1a1a1a]""#),
            "got {html}"
        );
    }

    #[test]
    fn a_bare_string_becomes_a_text_node() {
        let html = render(json!([{ "type": "span", "children": ["hi"] }]));
        assert!(html.ends_with("hi</span>"), "got {html}");
    }

    #[test]
    fn text_is_escaped() {
        let html = render(json!([{ "type": "span", "children": ["<script>&"] }]));
        assert!(html.contains("&lt;script&gt;&amp;"), "got {html}");
        assert!(!html.contains("<script>"), "got {html}");
    }

    #[test]
    fn attribute_values_are_escaped() {
        let html = render(json!([{ "type": "div", "title": "a \" b < c" }]));
        assert!(html.contains(r#"title="a &quot; b &lt; c""#), "got {html}");
    }

    /// `script` is in `DROPPED_TAGS`, and this is the test that keeps the web walk in
    /// Rust rather than in the page (ADR 0024).
    #[test]
    fn dropped_tags_take_their_subtree_with_them() {
        let html = render(json!([{
            "type": "div",
            "children": [
                { "type": "script", "children": ["alert(1)"] },
                { "type": "span", "children": ["kept"] },
            ],
        }]));
        assert!(!html.contains("alert"), "got {html}");
        assert!(!html.contains("<script"), "got {html}");
        assert!(html.contains("kept"), "got {html}");
    }

    #[test]
    fn void_elements_get_no_closing_tag() {
        let html = render(json!([{ "type": "img", "src": "a.png" }]));
        assert!(!html.contains("</img>"), "got {html}");
        assert!(html.contains(r#"src="a.png""#), "got {html}");
    }

    /// A page has no Wallpaper to slice, so the backdrop resource resolves to nothing
    /// rather than to a broken image.
    #[test]
    fn the_backdrop_resource_is_not_written_as_a_src() {
        let html = render(json!([{ "type": "img", "src": "tauler:root-bg" }]));
        assert!(!html.contains("src="), "got {html}");
    }

    #[test]
    fn booleans_render_nothing() {
        let html = render(json!([{ "type": "div", "children": [true, "x"] }]));
        assert!(!html.contains("true"), "got {html}");
        assert!(html.contains("x"), "got {html}");
    }

    #[test]
    fn a_style_object_becomes_kebab_cased_css() {
        let html = render(json!([{
            "type": "div",
            "style": { "flexGrow": 1, "left": "calc(40% - 7px)" },
        }]));
        assert!(html.contains("flex-grow: 1;"), "got {html}");
        assert!(html.contains("left: calc(40% - 7px);"), "got {html}");
    }

    /// A bare number in a layout file is a logical pixel, except where CSS says the
    /// property has no unit.
    #[test]
    fn numeric_style_values_get_px_unless_the_property_is_unitless() {
        let html = render(json!([{
            "type": "div",
            "style": { "width": 12, "opacity": 1 },
        }]));
        assert!(html.contains("width: 12px;"), "got {html}");
        assert!(html.contains("opacity: 1;"), "got {html}");
    }

    /// Paths are numbered the way `layout::html::build_tree` numbers them, so that a path
    /// names the same node in both renderers. A lone root child has an empty path.
    #[test]
    fn paths_number_children_including_text_nodes() {
        let html = render(json!([{
            "type": "div",
            "children": [
                "text",
                { "type": "span" },
                { "type": "b" },
            ],
        }]));
        assert!(html.contains(r#"<span data-tauler-path="1""#), "got {html}");
        assert!(html.contains(r#"<b data-tauler-path="2""#), "got {html}");
    }

    #[test]
    fn several_roots_are_numbered_from_zero() {
        let html = render(json!([{ "type": "div" }, { "type": "span" }]));
        assert!(html.contains(r#"<div data-tauler-path="0""#), "got {html}");
        assert!(html.contains(r#"<span data-tauler-path="1""#), "got {html}");
    }

    #[test]
    fn only_handler_carrying_elements_are_marked() {
        let html = render(json!([{
            "type": "div",
            "on_click": [{ "channel": "x", "event": {} }],
            "children": [{ "type": "span" }],
        }]));
        assert!(html.contains(r#"data-tauler-on="click""#), "got {html}");
        assert_eq!(html.matches("data-tauler-on").count(), 1, "got {html}");
    }

    #[test]
    fn a_drag_handler_is_marked_as_one() {
        let html = render(json!([{ "type": "div", "on_drag": { "$handler": 0 } }]));
        assert!(html.contains(r#"data-tauler-on="drag""#), "got {html}");
    }

    /// Props that are not on the allowlist stay out of the DOM — `on_click` holds intents,
    /// and a component's own props are not attributes.
    #[test]
    fn unknown_props_are_not_written_as_attributes() {
        let html = render(json!([{
            "type": "div",
            "value": 40,
            "on_click": [],
            "variant": "outline",
        }]));
        assert!(!html.contains("variant"), "got {html}");
        assert!(!html.contains("value="), "got {html}");
        assert!(!html.contains("on_click"), "got {html}");
    }

    #[test]
    fn nesting_past_the_cap_is_an_error() {
        let mut node = json!({ "type": "div" });
        for _ in 0..MAX_DEPTH + 1 {
            node = json!({ "type": "div", "children": [node] });
        }
        assert_eq!(
            render_output(&dom(json!([node]))),
            Err(DomError::MaxDepthExceeded(MAX_DEPTH))
        );
    }

    #[test]
    fn the_root_must_be_a_dom_surface() {
        let err = render_output(&json!({ "type": "panel", "children": [] })).unwrap_err();
        assert_eq!(err, DomError::NotADomSurface("panel".to_string()));
    }

    #[test]
    fn a_root_node_is_unwrapped_to_its_dom_child() {
        let tree = json!({
            "type": "root",
            "children": [{ "type": "dom", "children": [{ "type": "div" }] }],
        });
        assert!(matches!(render_output(&tree), Ok(Output::Dom { .. })));
    }

    #[test]
    fn two_dom_surfaces_are_an_error() {
        let tree = json!({
            "type": "root",
            "children": [{ "type": "dom" }, { "type": "dom" }],
        });
        assert_eq!(render_output(&tree), Err(DomError::MultipleDomSurfaces));
    }

    /// The envelope names its own shape, so glue that meets a kind it does not know can
    /// say so rather than reaching for a field that is not there.
    #[test]
    fn the_output_serialises_with_its_kind() {
        let out = Output::Dom {
            dom: "<div></div>".to_string(),
        };
        assert_eq!(
            serde_json::to_value(&out).unwrap(),
            json!({ "kind": "dom", "dom": "<div></div>" })
        );
    }
}
