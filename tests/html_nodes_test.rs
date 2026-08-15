//! Layout nodes are HTML elements (ADR 0016).
//!
//! These assert *behaviour*, not spelling: a `<div>` is only really a `<div>` if it
//! stacks its children the way `display: block` does. Asserting the tag name alone
//! would pass against a walker that recorded the name and applied no preset.

use takumi::measure as measure_layout;
use takumi::prelude::{MeasuredNode, RenderOptions, Viewport};
use tauler::config::FontConfig;
use tauler::{init_global_ctx, parse_layout, with_global_ctx};

fn measure_node(node: &serde_json::Value) -> MeasuredNode {
    init_global_ctx(FontConfig::default());
    let layout = parse_layout(node).expect("layout should parse");
    with_global_ctx(|global| {
        let options = RenderOptions::builder()
            .fonts(&global.fonts)
            .viewport(Viewport::new((Some(100u32), Some(800u32))).with_device_pixel_ratio(1.0))
            .node(layout)
            .build();
        measure_layout(options).expect("layout should measure")
    })
}

fn box_of(width: u32, height: u32) -> serde_json::Value {
    serde_json::json!({"type": "div", "style": {"width": width, "height": height}})
}

#[test]
fn div_is_accepted() {
    assert!(parse_layout(&serde_json::json!({"type": "div"})).is_ok());
}

#[test]
fn div_stacks_its_children_as_block() {
    let root = measure_node(&serde_json::json!({
        "type": "div",
        "children": [box_of(10, 10), box_of(10, 10)],
    }));

    // Block flow puts one under the other. Under takumi's bare default — `inline`,
    // which is what the retired `container` node used — they would sit side by side
    // and this would be 10.
    assert_eq!(root.height, 20.0, "two block children should stack");
}

#[test]
fn a_bare_string_becomes_text() {
    let root = measure_node(&serde_json::json!({
        "type": "div",
        "style": {"width": 100},
        "children": ["hello"],
    }));
    assert!(root.height > 0.0, "text should give the div a height");
}

#[test]
fn spans_flow_inline() {
    let inline = measure_node(&serde_json::json!({
        "type": "div",
        "style": {"width": 100},
        "children": [
            {"type": "span", "children": ["a"]},
            {"type": "span", "children": ["b"]},
        ],
    }));
    let block = measure_node(&serde_json::json!({
        "type": "div",
        "style": {"width": 100},
        "children": [
            {"type": "div", "children": ["a"]},
            {"type": "div", "children": ["b"]},
        ],
    }));
    assert!(
        inline.height < block.height,
        "two spans share a line ({}) where two divs stack ({})",
        inline.height,
        block.height
    );
}

#[test]
fn an_unknown_tag_is_inline_rather_than_an_error() {
    let root = measure_node(&serde_json::json!({
        "type": "div",
        "style": {"width": 100},
        "children": [
            {"type": "wat", "children": ["a"]},
            {"type": "wat", "children": ["b"]},
        ],
    }));
    let one_line = measure_node(&serde_json::json!({
        "type": "div",
        "style": {"width": 100},
        "children": [{"type": "span", "children": ["ab"]}],
    }));
    assert_eq!(root.height, one_line.height, "unknown tags share a line");
}

#[test]
fn void_tags_are_dropped_with_their_contents() {
    let root = measure_node(&serde_json::json!({
        "type": "div",
        "style": {"width": 100},
        "children": [{"type": "style", "children": [".a { color: red }"]}],
    }));
    assert_eq!(root.height, 0.0, "a <style> block renders nothing");
}

#[test]
fn an_img_without_a_src_is_a_parse_error() {
    let err = parse_layout(&serde_json::json!({"type": "img"})).unwrap_err();
    assert!(
        err.to_string().contains("src"),
        "the error should name the missing attribute, got: {err}"
    );
}

#[test]
fn an_inline_svg_is_rejected_with_a_way_out() {
    let err = parse_layout(&serde_json::json!({"type": "svg"})).unwrap_err();
    assert!(
        err.to_string().contains("data URI"),
        "the error should point at the supported alternative, got: {err}"
    );
}

#[test]
fn class_carries_tailwind_utilities() {
    let root = measure_node(&serde_json::json!({"type": "div", "class": "w-[64px] h-[32px]"}));
    assert_eq!((root.width, root.height), (64.0, 32.0));
}

#[test]
fn nesting_past_the_depth_limit_is_an_error() {
    // Past the cap. The walk must stop here rather than recurse until the stack dies,
    // which is what a cap above the real stack budget used to do.
    let mut node = serde_json::json!({"type": "div"});
    for _ in 0..40 {
        node = serde_json::json!({"type": "div", "children": [node]});
    }
    let err = parse_layout(&node).unwrap_err();
    assert!(
        err.to_string().contains("nesting"),
        "the error should name the depth limit, got: {err}"
    );
}
