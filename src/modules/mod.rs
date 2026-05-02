use takumi::rendering::MeasuredNode;

/// Find the deepest node under `(click_x, click_y)` that carries an `on_click` field.
///
/// Walks the `MeasuredNode` and JSON trees in lockstep.
/// Returns `(json_path, on_click_value)` on hit, `None` otherwise.
pub fn hit_test(
    measured: &MeasuredNode,
    json: &serde_json::Value,
    click_x: f32,
    click_y: f32,
) -> Option<(String, serde_json::Value)> {
    hit_test_inner(measured, json, click_x, click_y, "")
}

fn hit_test_inner(
    measured: &MeasuredNode,
    json: &serde_json::Value,
    click_x: f32,
    click_y: f32,
    path: &str,
) -> Option<(String, serde_json::Value)> {
    // Takumi stores absolute screen coordinates in transform[4/5]
    let node_x = measured.transform[4];
    let node_y = measured.transform[5];

    if click_x < node_x
        || click_x > node_x + measured.width
        || click_y < node_y
        || click_y > node_y + measured.height
    {
        return None;
    }

    // Prefer deepest child hit first
    if let Some(children_json) = json.get("children").and_then(|c| c.as_array()) {
        for (i, (child_m, child_j)) in measured.children.iter().zip(children_json).enumerate() {
            let child_path = format!("{}/children/{}", path, i);
            if let Some(result) = hit_test_inner(child_m, child_j, click_x, click_y, &child_path) {
                return Some(result);
            }
        }
    }

    // This node is the deepest hit — return it if it has on_click
    json.get("on_click").map(|v| (path.to_string(), v.clone()))
}
