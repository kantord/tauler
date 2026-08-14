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
