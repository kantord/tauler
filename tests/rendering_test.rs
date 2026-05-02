use costae::config::FontConfig;
use costae::{init_global_ctx, parse_layout, render_frame, render_frame_rgba};

#[test]
fn render_frame_respects_width_parameter() {
    init_global_ctx(FontConfig::default());
    let bgrx = render_frame(&serde_json::Value::Null, 100, 50, 1.0);
    assert_eq!(bgrx.len(), (100 * 50 * 4) as usize);
}

#[test]
fn render_frame_respects_different_width() {
    init_global_ctx(FontConfig::default());
    let bgrx_200 = render_frame(&serde_json::Value::Null, 200, 50, 1.0);
    let bgrx_400 = render_frame(&serde_json::Value::Null, 400, 50, 1.0);
    assert_eq!(bgrx_200.len(), 200 * 50 * 4);
    assert_eq!(bgrx_400.len(), 400 * 50 * 4);
}

#[test]
fn parse_layout_succeeds_for_valid_node_json() {
    let json = serde_json::json!({"type": "container", "children": []});
    assert!(parse_layout(&json).is_ok());
}

#[test]
fn render_frame_with_layout_returns_correct_size() {
    init_global_ctx(FontConfig::default());
    let content = serde_json::json!({
        "type": "container",
        "children": [{"type": "text", "text": "from layout"}]
    });
    let bgrx = render_frame(&content, 100, 200, 1.0);
    assert_eq!(bgrx.len(), 100 * 200 * 4);
}

#[test]
fn render_frame_rgba_transparent_pixels_have_alpha_zero() {
    init_global_ctx(FontConfig::default());
    let content = serde_json::json!({
        "type": "container",
        "tw": "w-full h-full",
        "children": []
    });
    let rgba = render_frame_rgba(&content, 10, 10, 1.0);
    assert_eq!(rgba.len(), 10 * 10 * 4, "expected 400 bytes for 10x10 RGBA");
    for i in 0..(10 * 10) {
        assert_eq!(
            rgba[i * 4 + 3],
            0,
            "pixel {} alpha should be 0 (transparent), got {}",
            i,
            rgba[i * 4 + 3]
        );
    }
}
