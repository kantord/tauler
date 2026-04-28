mod common;

fn eval_progress(inner_jsx: &str) -> serde_json::Value {
    let source = format!(
        "import {{ Progress }} from '@ui/progress';\nexport default function render() {{ return {inner_jsx}; }}"
    );
    common::eval_jsx(&source).layout
}

// --- node shape ---

#[test]
fn progress_renders_as_container_node() {
    assert_eq!(eval_progress("<Progress value={50} />")["type"], "container");
}

#[test]
fn progress_has_fill_and_remainder_children() {
    let node = eval_progress("<Progress value={50} />");
    let children = node["children"].as_array().expect("children");
    assert_eq!(children.len(), 2);
    assert_eq!(children[0]["type"], "container");
    assert_eq!(children[1]["type"], "container");
}

// --- value → fill flex ---

#[test]
fn progress_fill_flex_reflects_value() {
    let node = eval_progress("<Progress value={72} />");
    let flex = node["children"][0]["style"]["flex"].as_f64().expect("fill style.flex");
    assert!((flex - 72.0).abs() < 0.01);
}

#[test]
fn progress_remainder_flex_is_complement() {
    let node = eval_progress("<Progress value={72} />");
    let flex = node["children"][1]["style"]["flex"].as_f64().expect("remainder flex");
    assert!((flex - 28.0).abs() < 0.01);
}

#[test]
fn progress_clamps_value_above_100() {
    let node = eval_progress("<Progress value={150} />");
    let flex = node["children"][0]["style"]["flex"].as_f64().expect("fill style.flex");
    assert!((flex - 100.0).abs() < 0.01);
}

#[test]
fn progress_clamps_value_below_0() {
    let node = eval_progress("<Progress value={-10} />");
    let flex = node["children"][0]["style"]["flex"].as_f64().expect("fill style.flex");
    assert!((flex - 0.0).abs() < 0.01);
}

// --- color prop ---

#[test]
fn progress_fill_uses_primary_tw_by_default() {
    let node = eval_progress("<Progress value={50} />");
    let fill_tw = node["children"][0]["tw"].as_str().expect("fill tw");
    assert!(fill_tw.contains("bg-primary"), "expected bg-primary in '{fill_tw}'");
}

#[test]
fn progress_color_prop_sets_background_color_style() {
    let node = eval_progress(r##"<Progress value={50} color="#f38ba8" />"##);
    assert_eq!(node["children"][0]["style"]["backgroundColor"], "#f38ba8");
    let fill_tw = node["children"][0]["tw"].as_str().unwrap_or("");
    assert!(!fill_tw.contains("bg-primary"));
}

// --- track tw ---

#[test]
fn progress_track_includes_muted_background() {
    let node = eval_progress("<Progress value={50} />");
    let track_tw = node["tw"].as_str().expect("track tw");
    assert!(track_tw.contains("bg-muted"), "expected bg-muted in '{track_tw}'");
}

#[test]
fn progress_tw_prop_extends_track() {
    let node = eval_progress(r#"<Progress value={50} tw="mt-2" />"#);
    let track_tw = node["tw"].as_str().expect("track tw");
    assert!(track_tw.contains("mt-2"), "expected mt-2 in '{track_tw}'");
    assert!(track_tw.contains("bg-muted"), "base track tw should still be present");
}
