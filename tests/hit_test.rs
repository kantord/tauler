//! Clicks bind by render path (ADR 0018).
//!
//! Every test here goes through the real layout pipeline. The previous suite built
//! `MeasuredNode` trees by hand, which let the walk agree with a tree takumi would
//! never produce — and hid the fact that takumi replaces a node's measured children
//! with inline boxes the moment it holds text.

use tauler::config::FontConfig;
use tauler::{hit_test, init_global_ctx};

const W: u32 = 200;
const H: u32 = 100;

fn hit(layout: &serde_json::Value, x: f32, y: f32) -> Option<serde_json::Value> {
    init_global_ctx(FontConfig::default());
    hit_test(layout, W, H, 1.0, x, y)
}

fn intent(name: &str) -> serde_json::Value {
    serde_json::json!([{"channel": "test", "event": {"type": name}}])
}

#[test]
fn a_click_inside_a_handler_box_finds_it() {
    let layout = serde_json::json!({
        "type": "div",
        "style": {"width": W, "height": H},
        "on_click": intent("root"),
    });
    assert_eq!(hit(&layout, 50.0, 50.0), Some(intent("root")));
}

#[test]
fn a_click_outside_every_box_finds_nothing() {
    let layout = serde_json::json!({
        "type": "div",
        "style": {"width": 20, "height": 20},
        "on_click": intent("root"),
    });
    assert!(hit(&layout, 150.0, 80.0).is_none());
}

#[test]
fn a_node_without_a_handler_finds_nothing() {
    let layout = serde_json::json!({
        "type": "div",
        "style": {"width": W, "height": H},
    });
    assert!(hit(&layout, 50.0, 50.0).is_none());
}

#[test]
fn the_innermost_handler_under_the_point_wins() {
    let layout = serde_json::json!({
        "type": "div",
        "style": {"width": W, "height": H},
        "on_click": intent("parent"),
        "children": [{
            "type": "div",
            "style": {"width": 40, "height": 40},
            "on_click": intent("child"),
        }],
    });
    assert_eq!(hit(&layout, 20.0, 20.0), Some(intent("child")));
    assert_eq!(hit(&layout, 150.0, 80.0), Some(intent("parent")));
}

#[test]
fn a_child_without_a_handler_falls_back_to_its_parent() {
    let layout = serde_json::json!({
        "type": "div",
        "style": {"width": W, "height": H},
        "on_click": intent("parent"),
        "children": [{"type": "div", "style": {"width": 40, "height": 40}}],
    });
    assert_eq!(hit(&layout, 20.0, 20.0), Some(intent("parent")));
}

/// The regression ADR 0018 is about. A handler on a sibling of text used to be
/// unreachable: the row holds inline content, so takumi replaced its measured
/// children with inline boxes and the old index-paired walk compared a source node
/// against a text box.
#[test]
fn a_handler_survives_a_sibling_that_holds_text() {
    let layout = serde_json::json!({
        "type": "div",
        "class": "flex flex-row items-center",
        "style": {"width": W, "height": H},
        "children": [
            {
                "type": "div",
                "class": "flex flex-col",
                "style": {"width": 150, "height": H},
                "children": ["summary", "body"],
            },
            {
                "type": "div",
                "style": {"width": 50, "height": H},
                "on_click": intent("dismiss"),
                "children": ["x"],
            },
        ],
    });

    assert_eq!(
        hit(&layout, 175.0, 50.0),
        Some(intent("dismiss")),
        "clicking the button must find its handler"
    );
    assert!(
        hit(&layout, 40.0, 50.0).is_none(),
        "clicking the text column must not find the button's handler"
    );
}

/// Text is inline content, so a handler on a `<span>` has no layout node to bind to.
/// It finds nothing rather than finding the wrong thing.
#[test]
fn a_handler_on_an_inline_element_is_not_found() {
    let layout = serde_json::json!({
        "type": "div",
        "style": {"width": W, "height": H},
        "children": [{
            "type": "span",
            "on_click": intent("inline"),
            "children": ["click me"],
        }],
    });
    assert!(hit(&layout, 20.0, 10.0).is_none());
}
