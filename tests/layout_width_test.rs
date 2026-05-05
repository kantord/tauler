use takumi::{
    layout::Viewport,
    rendering::{measure_layout, RenderOptions},
};
use tauler::config::FontConfig;
use tauler::{init_global_ctx, parse_layout, render_frame, with_global_ctx};

fn measure_node(node: &serde_json::Value, width: u32, dpr: f32) -> takumi::rendering::MeasuredNode {
    init_global_ctx(FontConfig::default());
    let layout = parse_layout(node).unwrap();
    with_global_ctx(|global| {
        let options = RenderOptions::builder()
            .global(global)
            .viewport(Viewport::new((Some(width), Some(800u32))).with_device_pixel_ratio(dpr))
            .node(layout)
            .build();
        measure_layout(options).unwrap()
    })
}

fn measure(tw: &str, width: u32, dpr: f32) -> f32 {
    measure_node(
        &serde_json::json!({"type": "container", "tw": tw, "children": []}),
        width,
        dpr,
    )
    .width
}

#[test]
fn flex_col_without_w_full_shrinks_to_content() {
    let w = measure("flex flex-col h-full", 146, 1.46);
    assert_eq!(w, 0.0);
}

#[test]
fn w_full_fills_viewport_at_dpr_1() {
    let w = measure("flex flex-col w-full h-full", 100, 1.0);
    assert_eq!(w, 100.0);
}

#[test]
fn w_full_fills_viewport_at_dpr_1_46() {
    let w = measure("flex flex-col w-full h-full", 146, 1.46);
    assert_eq!(w, 146.0);
}

#[test]
fn padded_root_children_with_w_full_fill_content_box() {
    let root = measure_node(
        &serde_json::json!({
            "type": "container",
            "tw": "flex flex-col w-full h-full px-3",
            "children": [{"type": "container", "tw": "w-full", "children": []}]
        }),
        146,
        1.46,
    );
    let child_w = root.children[0].width;
    let expected_padding = 0.75 * 16.0 * 1.46 * 2.0;
    let expected = 146.0 - expected_padding;
    assert!(
        (child_w - expected).abs() < 2.0,
        "child w={child_w} should ≈ content box {expected}"
    );
}

#[test]
fn rendered_child_width_matches_layout_at_dpr_1_46() {
    init_global_ctx(FontConfig::default());
    let content = serde_json::json!({
        "type": "container",
        "tw": "flex flex-col h-full w-full",
        "style": {"backgroundColor": "red"},
        "children": [{
            "type": "container",
            "tw": "flex-1 w-full",
            "style": {"backgroundColor": "blue"},
            "children": []
        }]
    });
    let bgrx = render_frame(&content, 146, 50, 1.458);

    let row0 = &bgrx[..146 * 4];
    let mut first_blue = None;
    let mut last_blue = None;
    for (i, px) in row0.chunks_exact(4).enumerate() {
        if px[0] > 200 && px[2] < 50 {
            if first_blue.is_none() {
                first_blue = Some(i);
            }
            last_blue = Some(i);
        }
    }

    let first = first_blue.expect("no blue pixels found");
    let last = last_blue.unwrap();
    let blue_width = last - first + 1;

    assert!(
        blue_width >= 140,
        "Blue child width {blue_width} should fill most of the 146px viewport"
    );
}
