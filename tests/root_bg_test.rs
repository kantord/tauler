use costae::{solid_color_rgba, x11_bgrx_to_rgba};

#[test]
fn converts_bgrx_pixel_to_rgba() {
    let bgrx = vec![0x11, 0x22, 0x33, 0x00];
    let rgba = x11_bgrx_to_rgba(&bgrx);
    assert_eq!(rgba, vec![0x33, 0x22, 0x11, 0xFF]);
}

#[test]
fn converts_multiple_pixels() {
    let bgrx = vec![0x11, 0x22, 0x33, 0x00, 0xAA, 0xBB, 0xCC, 0x00];
    let rgba = x11_bgrx_to_rgba(&bgrx);
    assert_eq!(rgba, vec![0x33, 0x22, 0x11, 0xFF, 0xCC, 0xBB, 0xAA, 0xFF,]);
}

#[test]
fn always_sets_alpha_to_255() {
    let bgrx = vec![0x00, 0x00, 0x00, 0xFF]; // X byte is ignored
    let rgba = x11_bgrx_to_rgba(&bgrx);
    assert_eq!(rgba[3], 0xFF);
}

#[test]
fn empty_input_returns_empty() {
    assert_eq!(x11_bgrx_to_rgba(&[]), Vec::<u8>::new());
}

#[test]
fn solid_color_rgba_fills_correctly() {
    // i3 client.background #0e101a → TrueColor pixel 0x0e101a
    let rgba = solid_color_rgba(0x0e_10_1a, 2, 1);
    assert_eq!(rgba, vec![0x0e, 0x10, 0x1a, 0xFF, 0x0e, 0x10, 0x1a, 0xFF]);
}

#[test]
fn solid_color_rgba_black() {
    let rgba = solid_color_rgba(0x000000, 1, 1);
    assert_eq!(rgba, vec![0x00, 0x00, 0x00, 0xFF]);
}

#[test]
fn solid_color_rgba_white() {
    let rgba = solid_color_rgba(0xFFFFFF, 1, 2);
    assert_eq!(rgba, vec![0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
}
