use costae::{strut_partial_values_for_anchor, PanelAnchor};

#[test]
fn left_bar_primary_monitor() {
    let v = strut_partial_values_for_anchor(PanelAnchor::Left, 0, 0, 1920, 1080, 200, 1080);
    assert_eq!(v[0], 200); // left
    assert_eq!(v[1], 0); // right
    assert_eq!(v[2], 0); // top
    assert_eq!(v[3], 0); // bottom
    assert_eq!(v[4], 0); // left_start_y
    assert_eq!(v[5], 1079); // left_end_y
                            // all remaining strut fields are 0
    assert_eq!(&v[6..], &[0u32; 6]);
}

#[test]
fn left_bar_offset_monitor() {
    // secondary monitor at y=100
    let v = strut_partial_values_for_anchor(PanelAnchor::Left, 0, 100, 1920, 900, 200, 900);
    assert_eq!(v[0], 200); // left
    assert_eq!(v[4], 100); // left_start_y
    assert_eq!(v[5], 999); // left_end_y = mon_y + mon_height - 1
}
