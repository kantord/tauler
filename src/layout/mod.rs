use takumi::layout::node::Node;

/// Which screen edge a panel is anchored to. Drives both window placement and EWMH strut
/// reservation. Panels without an anchor are free-floating (no strut).
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum PanelAnchor {
    Left,
    Right,
    Top,
    Bottom,
}

impl PanelAnchor {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "left"   => Some(Self::Left),
            "right"  => Some(Self::Right),
            "top"    => Some(Self::Top),
            "bottom" => Some(Self::Bottom),
            _        => None,
        }
    }
}

/// Per-monitor metadata, including physical pixel dimensions and device pixel ratio.
#[derive(Debug, Clone)]
pub struct OutputInfo {
    pub name: String,
    pub x: i16,
    pub y: i16,
    pub width: u32,
    pub height: u32,
    pub dpr: f32,
}

/// Logical-pixel description of a `<panel>` node extracted from the JSX root.
/// All dimensions are in logical pixels; the display backend scales to physical pixels.
#[derive(Debug, Clone)]
pub struct PanelSpecData {
    pub id: String,
    pub anchor: Option<PanelAnchor>,
    /// Logical width in CSS px (same unit as i3 config / Tailwind values).
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
    /// i3-specific gap to reserve around the screen edges. Temporary until a
    /// cleaner per-WM mechanism exists.
    pub outer_gap: u32,
    /// RandR output name to place this panel on (e.g. "DP-2"). None = primary output.
    pub output: Option<String>,
    /// When true the window is stacked above other windows (for floating overlays like
    /// notifications). When false (default) the window sits below tiled content.
    pub above: bool,
    /// The layout subtree that lives inside this panel (first child of the panel node).
    pub content: serde_json::Value,
    /// Device pixel ratio for this panel's output. Set by the app after parsing.
    pub dpr: f32,
}

impl std::fmt::Display for PanelSpecData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.id)
    }
}

pub fn parse_layout(value: &serde_json::Value) -> Result<Node, serde_json::Error> {
    use serde::Deserialize;
    Node::deserialize(value)
}

/// Parse the JSX evaluator's output into a list of panel specs.
///
/// Expects the root value to be `{ type: "root", children: [...panels] }`. Each panel
/// child must have at minimum `id`, `width`, and `height`. Returns an error string if
/// the root type is wrong or a required field is missing.
pub fn parse_root_node(root: &serde_json::Value) -> Result<Vec<PanelSpecData>, String> {
    if root.get("type").and_then(|t| t.as_str()) != Some("root") {
        return Err(format!("expected root node, got {:?}", root.get("type")));
    }
    let children = root.get("children")
        .and_then(|c| c.as_array())
        .ok_or_else(|| "root node has no children array".to_string())?;

    children.iter().enumerate()
        .filter(|(_, p)| p.get("type").and_then(|t| t.as_str()) == Some("panel"))
        .map(|(i, p)| parse_panel_spec(i, p))
        .collect()
}

fn required_str<'a>(obj: &'a serde_json::Value, key: &str, label: &str) -> Result<&'a str, String> {
    obj.get(key).and_then(|v| v.as_str())
        .ok_or_else(|| format!("{label} missing {key}"))
}

fn required_u64(obj: &serde_json::Value, key: &str, label: &str) -> Result<u64, String> {
    obj.get(key).and_then(|v| v.as_u64())
        .ok_or_else(|| format!("{label} missing {key}"))
}

fn optional_str<'a>(obj: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    obj.get(key).and_then(|v| v.as_str())
}

fn optional_i64(obj: &serde_json::Value, key: &str, default: i64) -> i64 {
    obj.get(key).and_then(|v| v.as_i64()).unwrap_or(default)
}

fn optional_u64(obj: &serde_json::Value, key: &str, default: u64) -> u64 {
    obj.get(key).and_then(|v| v.as_u64()).unwrap_or(default)
}

fn optional_bool(obj: &serde_json::Value, key: &str, default: bool) -> bool {
    obj.get(key).and_then(|v| v.as_bool()).unwrap_or(default)
}

fn first_child(node: &serde_json::Value) -> serde_json::Value {
    node.get("children")
        .and_then(|c| c.as_array())
        .and_then(|c| c.first())
        .cloned()
        .unwrap_or(serde_json::Value::Null)
}

fn parse_panel_spec(i: usize, panel: &serde_json::Value) -> Result<PanelSpecData, String> {
    let id = required_str(panel, "id", &format!("panel[{i}]"))?.to_string();
    let label = format!("panel '{id}'");
    Ok(PanelSpecData {
        id,
        width:     required_u64(panel, "width",  &label)? as u32,
        height:    required_u64(panel, "height", &label)? as u32,
        anchor:    optional_str(panel, "anchor").and_then(PanelAnchor::parse),
        x:         optional_i64(panel, "x",         0) as i32,
        y:         optional_i64(panel, "y",         0) as i32,
        outer_gap: optional_u64(panel, "outer_gap", 0) as u32,
        output:    optional_str(panel, "output").map(str::to_string),
        above:     optional_bool(panel, "above", false),
        content:   first_child(panel),
        dpr:       1.0,
    })
}

