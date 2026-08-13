use takumi::prelude::Node;

/// Which screen edge a panel is anchored to. Drives window placement only — anchoring
/// reserves no space (see `docs/adr/0001`). Panels without an anchor are free-floating.
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
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            "top" => Some(Self::Top),
            "bottom" => Some(Self::Bottom),
            _ => None,
        }
    }
}

/// What kind of surface a spec describes.
///
/// Both kinds are rendered identically — takumi rasterizes the subtree into a
/// pixel buffer. They differ only in where that buffer is handed to: a `Panel`
/// gets its own window (X11 override-redirect window / Wayland layer surface),
/// a `Wallpaper` is painted straight into the desktop background of its output.
#[derive(Debug, PartialEq, Eq, Clone, Copy, Default)]
pub enum SurfaceKind {
    #[default]
    Panel,
    Wallpaper,
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

impl OutputInfo {
    /// The area this output covers on the root screen.
    pub fn rect(&self) -> Rect {
        Rect {
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
        }
    }
}

/// An absolute rect on the root screen, in physical pixels.
///
/// `Hash`/`Eq` because it doubles as a cache key: two panels over one wallpaper
/// differ only in which slice of it they cover, so the rect is what tells their
/// otherwise-identical frames apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rect {
    pub x: i16,
    pub y: i16,
    pub width: u32,
    pub height: u32,
}

/// Logical-pixel description of a `<panel>` or `<wallpaper>` node extracted from
/// the JSX root. All dimensions are in logical pixels; the display backend scales
/// to physical pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceSpec {
    pub id: String,
    pub kind: SurfaceKind,
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

impl std::fmt::Display for SurfaceSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.id)
    }
}

/// Where a surface's top-left corner lands on the root screen, in physical pixels.
///
/// Anchored surfaces sit flush against an edge of their output; unanchored ones
/// are offset from its origin by their logical `x`/`y`. `phys` is passed in
/// rather than derived from the spec because callers that already resized a
/// window know its real dimensions, which can differ from the spec mid-reconcile.
///
/// The pipeline needs this as much as the display backend does: cropping the
/// wallpaper behind a panel means knowing where the panel actually is, and for
/// an anchored panel `spec.x`/`spec.y` are both zero.
///
/// Takes the output's `Rect` rather than the whole [`OutputInfo`]: a caller that
/// only knows where a wallpaper sits — which is the same rect — should not have
/// to invent a name and a dpr it has no use for.
pub fn surface_origin(spec: &SurfaceSpec, phys: (u32, u32), output: Rect) -> (i16, i16) {
    let (phys_width, phys_height) = phys;
    let (mon_x, mon_y) = (output.x, output.y);
    match &spec.anchor {
        Some(PanelAnchor::Left) | Some(PanelAnchor::Top) => (mon_x, mon_y),
        Some(PanelAnchor::Right) => (mon_x + output.width as i16 - phys_width as i16, mon_y),
        Some(PanelAnchor::Bottom) => (mon_x, mon_y + output.height as i16 - phys_height as i16),
        None => (
            mon_x + (spec.x as f32 * spec.dpr).round() as i16,
            mon_y + (spec.y as f32 * spec.dpr).round() as i16,
        ),
    }
}

pub fn parse_layout(value: &serde_json::Value) -> Result<Node, serde_json::Error> {
    use serde::Deserialize;
    Node::deserialize(value)
}

/// Parse the JSX evaluator's output into a list of surface specs.
///
/// Expects the root value to be `{ type: "root", children: [...surfaces] }`. Each
/// `panel` child must have at minimum `id`, `width`, and `height`; each `wallpaper`
/// child needs only `id`, since its size is always the display's. Returns an error
/// string if the root type is wrong or a required field is missing.
///
/// `<surface type="panel">` and `<surface type="wallpaper">` are equivalent
/// long-hand spellings: a `type` prop overrides the tag name during JSX
/// flattening (see `jsx::flatten_passthrough`), so they arrive here already
/// indistinguishable from `<panel>` / `<wallpaper>`. A bare `<surface>` names no
/// kind and is rejected rather than silently ignored.
pub fn parse_root_node(root: &serde_json::Value) -> Result<Vec<SurfaceSpec>, String> {
    if root.get("type").and_then(|t| t.as_str()) != Some("root") {
        return Err(format!("expected root node, got {:?}", root.get("type")));
    }
    let children = root
        .get("children")
        .and_then(|c| c.as_array())
        .ok_or_else(|| "root node has no children array".to_string())?;

    children
        .iter()
        .enumerate()
        .filter_map(|(i, p)| match p.get("type").and_then(|t| t.as_str()) {
            Some("panel") => Some(parse_panel(i, p)),
            Some("wallpaper") => Some(parse_wallpaper(i, p)),
            Some("surface") => Some(Err(format!(
                "surface[{i}] needs type=\"panel\" or type=\"wallpaper\""
            ))),
            _ => None,
        })
        .collect()
}

fn required_str<'a>(obj: &'a serde_json::Value, key: &str, label: &str) -> Result<&'a str, String> {
    obj.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("{label} missing {key}"))
}

fn required_u64(obj: &serde_json::Value, key: &str, label: &str) -> Result<u64, String> {
    obj.get(key)
        .and_then(|v| v.as_u64())
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

/// A `<wallpaper>` node. It carries no geometry of its own: `width`/`height`/`x`/`y`
/// are filled in from the target output before reconciliation, so the rendered
/// buffer always matches the display exactly.
fn parse_wallpaper(i: usize, node: &serde_json::Value) -> Result<SurfaceSpec, String> {
    let id = required_str(node, "id", &format!("wallpaper[{i}]"))?.to_string();
    Ok(SurfaceSpec {
        id,
        kind: SurfaceKind::Wallpaper,
        width: 0,
        height: 0,
        anchor: None,
        x: 0,
        y: 0,
        outer_gap: 0,
        output: optional_str(node, "output").map(str::to_string),
        above: false,
        content: first_child(node),
        dpr: 1.0,
    })
}

fn parse_panel(i: usize, panel: &serde_json::Value) -> Result<SurfaceSpec, String> {
    let id = required_str(panel, "id", &format!("panel[{i}]"))?.to_string();
    let label = format!("panel '{id}'");
    Ok(SurfaceSpec {
        id,
        kind: SurfaceKind::Panel,
        width: required_u64(panel, "width", &label)? as u32,
        height: required_u64(panel, "height", &label)? as u32,
        anchor: optional_str(panel, "anchor").and_then(PanelAnchor::parse),
        x: optional_i64(panel, "x", 0) as i32,
        y: optional_i64(panel, "y", 0) as i32,
        outer_gap: optional_u64(panel, "outer_gap", 0) as u32,
        output: optional_str(panel, "output").map(str::to_string),
        above: optional_bool(panel, "above", false),
        content: first_child(panel),
        dpr: 1.0,
    })
}

#[cfg(test)]
mod origin_tests {
    use super::*;

    fn out() -> OutputInfo {
        // A secondary monitor at a non-zero origin, so a bug that ignores the
        // output offset can't pass by accident.
        OutputInfo {
            name: "DP-4".into(),
            x: 1080,
            y: 748,
            width: 3840,
            height: 2160,
            dpr: 1.0,
        }
    }

    fn spec(anchor: Option<PanelAnchor>, x: i32, y: i32, dpr: f32) -> SurfaceSpec {
        SurfaceSpec {
            id: "s".into(),
            kind: SurfaceKind::Panel,
            anchor,
            width: 272,
            height: 2160,
            x,
            y,
            outer_gap: 0,
            output: None,
            above: false,
            content: serde_json::Value::Null,
            dpr,
        }
    }

    #[test]
    fn left_and_top_sit_at_the_output_origin() {
        let o = out();
        let at = |a| surface_origin(&spec(Some(a), 0, 0, 1.0), (272, 2160), o.rect());
        assert_eq!(at(PanelAnchor::Left), (1080, 748));
        assert_eq!(at(PanelAnchor::Top), (1080, 748));
    }

    #[test]
    fn right_is_flush_with_the_outputs_right_edge() {
        let (x, y) = surface_origin(
            &spec(Some(PanelAnchor::Right), 0, 0, 1.0),
            (60, 2160),
            out().rect(),
        );
        assert_eq!((x, y), (1080 + 3840 - 60, 748));
    }

    #[test]
    fn bottom_is_flush_with_the_outputs_bottom_edge() {
        let (x, y) = surface_origin(
            &spec(Some(PanelAnchor::Bottom), 0, 0, 1.0),
            (3840, 32),
            out().rect(),
        );
        assert_eq!((x, y), (1080, 748 + 2160 - 32));
    }

    #[test]
    fn unanchored_offsets_from_the_output_origin_in_physical_pixels() {
        // Logical 10,20 at dpr 2.0 is 20,40 physical from the output's corner.
        let (x, y) = surface_origin(&spec(None, 10, 20, 2.0), (100, 100), out().rect());
        assert_eq!((x, y), (1080 + 20, 748 + 40));
    }
}
