use costae::config::FontConfig;
use costae::{init_global_ctx, preload_layout_images, with_global_ctx};

fn write_temp_png() -> std::path::PathBuf {
    let path = std::env::temp_dir().join("costae_test_1x1.png");
    let img = image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 0, 0, 255]));
    img.save(&path).unwrap();
    path
}

#[test]
fn preload_layout_images_loads_local_file_into_store() {
    init_global_ctx(FontConfig::default());
    let path = write_temp_png();
    let src = path.to_str().unwrap().to_string();
    let layout = serde_json::json!({
        "type": "container",
        "children": [{"type": "image", "src": src.clone()}]
    });
    preload_layout_images(&layout);
    with_global_ctx(|global| {
        assert!(global.persistent_image_store.get(&src).is_some());
    });
}

#[test]
fn preload_layout_images_ignores_missing_files() {
    init_global_ctx(FontConfig::default());
    let layout = serde_json::json!({"type": "image", "src": "/nonexistent/image.png"});
    preload_layout_images(&layout);
    with_global_ctx(|global| {
        assert!(global
            .persistent_image_store
            .get("/nonexistent/image.png")
            .is_none());
    });
}

#[test]
fn preload_layout_images_skips_http_urls() {
    init_global_ctx(FontConfig::default());
    let layout = serde_json::json!({"type": "image", "src": "https://example.com/img.png"});
    preload_layout_images(&layout);
    with_global_ctx(|global| {
        assert!(global
            .persistent_image_store
            .get("https://example.com/img.png")
            .is_none());
    });
}
