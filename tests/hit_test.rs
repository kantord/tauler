use tauler::{hit_test, MeasuredNode};

fn node(x: f32, y: f32, w: f32, h: f32, children: Vec<MeasuredNode>) -> MeasuredNode {
    MeasuredNode {
        width: w,
        height: h,
        transform: [1.0, 0.0, 0.0, 1.0, x, y],
        children,
        runs: vec![],
    }
}

#[test]
fn hit_within_root_with_on_click_returns_some() {
    let measured = node(0.0, 0.0, 100.0, 100.0, vec![]);
    let json =
        serde_json::json!({"type": "container", "on_click": {"action": "test"}, "children": []});
    let result = hit_test(&measured, &json, 50.0, 50.0);
    assert!(result.is_some());
    let (_, on_click) = result.unwrap();
    assert_eq!(on_click["action"], "test");
}

#[test]
fn miss_outside_bounds_returns_none() {
    let measured = node(0.0, 0.0, 100.0, 100.0, vec![]);
    let json = serde_json::json!({"on_click": {"action": "test"}, "children": []});
    assert!(hit_test(&measured, &json, 150.0, 50.0).is_none());
    assert!(hit_test(&measured, &json, 50.0, 150.0).is_none());
    assert!(hit_test(&measured, &json, -1.0, 50.0).is_none());
}

#[test]
fn node_without_on_click_returns_none() {
    let measured = node(0.0, 0.0, 100.0, 100.0, vec![]);
    let json = serde_json::json!({"type": "container", "children": []});
    assert!(hit_test(&measured, &json, 50.0, 50.0).is_none());
}

#[test]
fn hit_prefers_child_on_click_over_parent() {
    let child = node(10.0, 10.0, 30.0, 30.0, vec![]);
    let parent = node(0.0, 0.0, 100.0, 100.0, vec![child]);
    let json = serde_json::json!({
        "on_click": {"action": "parent"},
        "children": [
            {"on_click": {"action": "child"}, "children": []}
        ]
    });
    let (_, on_click) = hit_test(&parent, &json, 20.0, 20.0).unwrap();
    assert_eq!(on_click["action"], "child");
}

#[test]
fn hit_falls_back_to_parent_when_child_has_no_on_click() {
    let child = node(10.0, 10.0, 30.0, 30.0, vec![]);
    let parent = node(0.0, 0.0, 100.0, 100.0, vec![child]);
    let json = serde_json::json!({
        "on_click": {"action": "parent"},
        "children": [
            {"children": []}
        ]
    });
    let (_, on_click) = hit_test(&parent, &json, 20.0, 20.0).unwrap();
    assert_eq!(on_click["action"], "parent");
}

#[test]
fn child_uses_absolute_transform_for_hit_detection() {
    // Takumi stores absolute screen coords in transform[4/5].
    // Child at absolute (60, 60) — i.e. parent at (50,50) + child offset (10,10).
    let child = node(60.0, 60.0, 20.0, 20.0, vec![]);
    let parent = node(50.0, 50.0, 100.0, 100.0, vec![child]);
    let json = serde_json::json!({
        "children": [{"on_click": {"action": "child"}, "children": []}]
    });
    // Click at (65, 65) is inside child's absolute bounds (60..80, 60..80)
    let result = hit_test(&parent, &json, 65.0, 65.0);
    assert!(result.is_some());
    assert_eq!(result.unwrap().1["action"], "child");

    // Click at (55, 55) is inside parent but outside child
    assert!(hit_test(&parent, &json, 55.0, 55.0).is_none());
}

#[test]
fn path_reflects_tree_position() {
    let child = node(0.0, 0.0, 50.0, 50.0, vec![]);
    let parent = node(0.0, 0.0, 100.0, 100.0, vec![child]);
    let json = serde_json::json!({
        "children": [{"on_click": {"action": "x"}, "children": []}]
    });
    let (path, _) = hit_test(&parent, &json, 25.0, 25.0).unwrap();
    assert_eq!(path, "/children/0");
}
