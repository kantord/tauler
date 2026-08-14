//! Turning the layout file's tree into takumi nodes.
//!
//! The JSX evaluator hands us `{ type: "div", class: "…", children: [...] }`. takumi
//! wants a [`Node`], which has three kinds — container, text, image — and carries the
//! tag name as metadata. This module is the walk between the two, and it is the whole
//! of tauler's HTML support: there is no parser here, because there is no text to
//! parse. See `docs/adr/0017` for why `takumi-html` does not do this job for us.
//!
//! Three things about the shape of the output are worth knowing before reading:
//!
//! **Every element is a container.** `<span>` is not a text node; it is a container
//! whose preset leaves it `display: inline`. Text exists only where a string was
//! written, which is why there is no element that means "text".
//!
//! **Presets are what make the tags real.** Applying the tag name without
//! [`super::presets`] would give you `<p>` with no margins and `<div>` that does not
//! stack — the names would be decoration. They are applied under `class` and `style`,
//! so authored styling always wins.
//!
//! **Falsy children are already gone.** `optative-script` drops `null`, `undefined`
//! and `false` while flattening, so `{cond && <div/>}` never reaches us as a value. We
//! drop booleans anyway, because `true` survives that filter and rendering the word
//! "true" is nobody's intent.

use std::str::FromStr;

use serde_json::Value;
use takumi::prelude::{ImageData, ImageSourceInput, Node, Style, TailwindValues};

use super::presets::preset_for_tag;

/// Cap on element nesting, guarding the recursive walk against a layout file that
/// nests without bound. Matches Blink's limit, by way of takumi-html.
pub const MAX_DEPTH: usize = 512;

/// Tags whose entire subtree is dropped. Distinct from the tags whose preset is
/// `display: none` — those are laid out and hidden, these never exist.
const VOID_TAGS: [&str; 5] = ["head", "meta", "link", "style", "script"];

/// Why a layout tree could not be turned into takumi nodes.
#[derive(Debug, thiserror::Error)]
pub enum LayoutError {
    #[error("<img> needs a non-empty src")]
    MissingImageSrc,
    #[error("element nesting exceeded the maximum depth of {0}")]
    MaxDepthExceeded(usize),
    #[error("inline <svg> is not supported — put the SVG in a data URI on an <img src>")]
    InlineSvg,
    #[error("expected an element or a string, found {0}")]
    NotANode(&'static str),
    #[error("element has no tag name")]
    MissingTag,
    #[error("invalid style: {0}")]
    Style(serde_json::Error),
}

/// Where each `on_click` sits in the finished node tree, as a child-index path.
///
/// Recorded here rather than recovered later, because there is no way to recover it:
/// the tree takumi lays out is not index-comparable with the tree the layout file
/// wrote (`docs/adr/0018`). The walk knows both at once, so it is the only place the
/// two can be tied together without guessing.
pub type Handlers = Vec<(Vec<usize>, Value)>;

/// Build the takumi node tree for one surface's contents.
pub fn build_node(value: &Value) -> Result<Node, LayoutError> {
    Ok(build_tree(value)?.0)
}

/// As [`build_node`], and also where every click handler ended up.
///
/// A tree that produces exactly one node yields it directly; anything else is wrapped
/// in a container, so a surface always has a single root to render. Paths are
/// collected as though the wrapper were always there, then shortened by one when it
/// turns out not to be.
pub fn build_tree(value: &Value) -> Result<(Node, Handlers), LayoutError> {
    let mut nodes = Vec::new();
    let mut handlers = Handlers::new();
    let mut path = Vec::new();
    push_node(value, 0, &mut nodes, &mut path, &mut handlers)?;

    if nodes.len() == 1 {
        for (path, _) in handlers.iter_mut() {
            path.remove(0);
        }
        Ok((nodes.remove(0), handlers))
    } else {
        Ok((Node::container(nodes), handlers))
    }
}

fn push_node(
    value: &Value,
    depth: usize,
    out: &mut Vec<Node>,
    path: &mut Vec<usize>,
    handlers: &mut Handlers,
) -> Result<(), LayoutError> {
    match value {
        Value::String(s) => out.push(Node::text(s.clone())),
        Value::Number(n) => out.push(Node::text(n.to_string())),
        Value::Bool(_) | Value::Null => {}
        Value::Array(items) => {
            for item in items {
                push_node(item, depth, out, path, handlers)?;
            }
        }
        Value::Object(_) => {
            path.push(out.len());
            let built = build_element(value, depth, path, handlers);
            match built {
                Ok(Some(node)) => {
                    if let Some(on_click) = value.get("on_click") {
                        handlers.push((path.clone(), on_click.clone()));
                    }
                    out.push(node);
                }
                Ok(None) => {}
                Err(e) => {
                    path.pop();
                    return Err(e);
                }
            }
            path.pop();
        }
    }
    Ok(())
}

fn build_element(
    obj: &Value,
    depth: usize,
    path: &mut Vec<usize>,
    handlers: &mut Handlers,
) -> Result<Option<Node>, LayoutError> {
    if depth >= MAX_DEPTH {
        return Err(LayoutError::MaxDepthExceeded(MAX_DEPTH));
    }

    let tag = obj
        .get("type")
        .and_then(Value::as_str)
        .ok_or(LayoutError::MissingTag)?;

    if VOID_TAGS.contains(&tag) {
        return Ok(None);
    }

    let node = match tag {
        // A line break is a text node holding one, which is what `white-space: pre`
        // on the `br` preset then keeps.
        "br" => Node::text("\n"),
        "svg" => return Err(LayoutError::InlineSvg),
        "img" => Node::image(image_data(obj)?),
        _ => {
            let mut children = Vec::new();
            if let Some(items) = obj.get("children") {
                push_node(items, depth + 1, &mut children, path, handlers)?;
            }
            Node::container(children)
        }
    };

    Ok(Some(apply_metadata(node, tag, obj)?))
}

fn image_data(obj: &Value) -> Result<ImageData, LayoutError> {
    let src = obj
        .get("src")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|src| !src.is_empty())
        .ok_or(LayoutError::MissingImageSrc)?;

    Ok(ImageData {
        src: ImageSourceInput::Url(src.into()),
        width: dimension(obj, "width"),
        height: dimension(obj, "height"),
    })
}

fn dimension(obj: &Value, key: &str) -> Option<f32> {
    obj.get(key).and_then(Value::as_f64).map(|n| n as f32)
}

fn apply_metadata(mut node: Node, tag: &str, obj: &Value) -> Result<Node, LayoutError> {
    node = node.with_tag_name(tag);

    if let Some(preset) = preset_for_tag(tag) {
        node = node.with_preset(preset.clone());
    }

    if let Some(class) = obj.get("class").and_then(Value::as_str) {
        // Recorded as the class name as well as read as utilities: one attribute, both
        // of HTML's jobs. See `docs/adr/0016`.
        node = node.with_class_name(class);
        if let Ok(tw) = TailwindValues::from_str(class) {
            node = node.with_tw(tw);
        }
    }

    if let Some(style) = obj.get("style") {
        let style: Style = serde_json::from_value(style.clone()).map_err(LayoutError::Style)?;
        node = node.with_style(style);
    }

    if let Some(id) = obj.get("id").and_then(Value::as_str) {
        node = node.with_id(id);
    }

    Ok(node)
}
