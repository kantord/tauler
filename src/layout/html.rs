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

/// Cap on element nesting, guarding this recursive walk against a layout file that
/// nests without bound.
///
/// Deliberately *not* Blink's 512, which takumi-html uses: measured against a debug
/// build on a 2 MiB test thread, this walk overflows the stack at a nesting depth of
/// 34, so a 512 cap could never fire and deep nesting aborted the process instead of
/// returning an error. The frames are large because each level holds a `Node` and its
/// `Vec<Node>` children by value.
///
/// 32 leaves roughly twice the depth any real bar uses. Raising it means making the
/// walk iterative first — the constant is a symptom, not the fix.
pub const MAX_DEPTH: usize = 32;

/// Tags whose entire subtree is dropped rather than laid out.
///
/// Not the same set as the tags whose preset is `display: none`, though every tag here
/// carries that preset too: `title`, `noscript`, `datalist` and `template` are hidden
/// but still measured, while these five never reach the tree at all. Document metadata
/// has no content to lay out, and a `<style>` body would otherwise render as text.
///
// ─── vendored from takumi-html ─────────────────────────────────────────────────────
// takumi-html 0.2.0 `VOID_TAGS` — Copyright (c) 2025 Kane Wang — MIT OR Apache-2.0
// https://github.com/kane50613/takumi/blob/6d31b7c5feeefafc360e5b09500ebc4d849f6f27/takumi-html/src/lib.rs#L39
pub(crate) const DROPPED_TAGS: [&str; 5] = ["head", "meta", "link", "style", "script"];
// ─── end vendored ──────────────────────────────────────────────────────────────────
/// Why a layout tree could not be turned into takumi nodes.
#[derive(Debug, thiserror::Error)]
pub enum LayoutError {
    #[error("<img> needs a non-empty src")]
    MissingImageSrc,
    #[error("element nesting exceeded the maximum depth of {0}")]
    MaxDepthExceeded(usize),
    #[error("inline <svg> is not supported — put the SVG in a data URI on an <img src>")]
    InlineSvg,
    #[error("element has no tag name")]
    MissingTag,
    #[error("invalid style: {0}")]
    Style(serde_json::Error),
}

/// One node's handlers, bound to the render path they were found at.
///
/// Named for what it does rather than what it holds: `Handler` is the glossary's word
/// for the intents-or-function a node carries, which is `pointer::Handler`.
///
/// Recorded during the walk rather than recovered later, because there is no way to
/// recover it: the tree takumi lays out is not index-comparable with the tree the
/// layout file wrote (`docs/adr/0018`). The walk knows both at once, so it is the only
/// place the two can be tied together without guessing.
#[derive(Debug, Clone)]
pub struct Binding {
    /// Child-index path from the root of the finished node tree.
    pub path: Vec<usize>,
    /// Fires on a press. Either an array of intents or `{"$handler": n}`, verbatim
    /// from the layout file (`docs/adr/0021`).
    pub on_click: Option<Value>,
    /// Fires on a press and on every motion until release, and captures the pointer
    /// while it does (`docs/adr/0020`). Same two shapes as `on_click`.
    pub on_drag: Option<Value>,
    /// How the node was written, for diagnostics — `<span id="close" class="…">`.
    ///
    /// A path of child indices is useless in a warning: nobody can map `[0, 3, 1]` back
    /// to a line of JSX. The tag, `id` and `class` are all in hand here and cost one
    /// string per handler, of which a bar has a handful.
    pub label: String,
}

pub type Bindings = Vec<Binding>;

/// Describe a node the way its author wrote it, for a diagnostic.
fn describe(tag: &str, obj: &Value) -> String {
    let attr = |key: &str| obj.get(key).and_then(Value::as_str);
    let mut out = format!("<{tag}");
    if let Some(id) = attr("id") {
        out.push_str(&format!(" id=\"{id}\""));
    }
    if let Some(class) = attr("class") {
        out.push_str(&format!(" class=\"{class}\""));
    }
    out.push('>');
    out
}

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
pub fn build_tree(value: &Value) -> Result<(Node, Bindings), LayoutError> {
    let mut nodes = Vec::new();
    let mut handlers = Bindings::new();
    let mut path = Vec::new();
    push_node(value, 0, &mut nodes, &mut path, &mut handlers)?;

    if nodes.len() == 1 {
        for handler in handlers.iter_mut() {
            handler.path.remove(0);
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
    handlers: &mut Bindings,
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
                    let on_click = value.get("on_click").cloned();
                    let on_drag = value.get("on_drag").cloned();
                    if on_click.is_some() || on_drag.is_some() {
                        let tag = value.get("type").and_then(Value::as_str).unwrap_or("?");
                        handlers.push(Binding {
                            path: path.clone(),
                            on_click,
                            on_drag,
                            label: describe(tag, value),
                        });
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
    handlers: &mut Bindings,
) -> Result<Option<Node>, LayoutError> {
    if depth >= MAX_DEPTH {
        return Err(LayoutError::MaxDepthExceeded(MAX_DEPTH));
    }

    let tag = obj
        .get("type")
        .and_then(Value::as_str)
        .ok_or(LayoutError::MissingTag)?;

    if DROPPED_TAGS.contains(&tag) {
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
