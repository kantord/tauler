use costae::{init_global_ctx, render_frame};
use costae::config::FontConfig;

#[test]
fn render_frame_output_matches_physical_dimensions() {
    init_global_ctx(FontConfig::default());
    let out1x = render_frame(&serde_json::Value::Null, 10, 10, 1.0);
    assert_eq!(out1x.len(), 10 * 10 * 4);

    let out2x = render_frame(&serde_json::Value::Null, 20, 20, 2.0);
    assert_eq!(out2x.len(), 20 * 20 * 4);
}
