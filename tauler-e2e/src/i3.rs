//! The half of the harness that knows what i3 is.
//!
//! Kept apart from [`crate`] so adding sway means a second module and a second
//! image, not a rewrite. No trait yet: an abstraction designed against exactly
//! one implementation tends to be the wrong one.

use anyhow::{anyhow, bail, Result};
use serde_json::Value;

use crate::{Desktop, Rect};

/// What a workspace reserves on each edge, in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Gaps {
    pub left: u32,
    pub right: u32,
    pub top: u32,
    pub bottom: u32,
}

impl std::fmt::Display for Gaps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "left={} right={} top={} bottom={}",
            self.left, self.right, self.top, self.bottom
        )
    }
}

/// The gaps on the focused workspace.
///
/// tauler-i3 writes with `gaps ... current set`, so the focused workspace is
/// the one that carries them — asking i3 for anything else would report a
/// workspace nobody wrote to.
pub fn focused_workspace_gaps(desktop: &Desktop) -> Result<Gaps> {
    let tree = get_tree(desktop)?;
    let workspace =
        focused_workspace(&tree).ok_or_else(|| anyhow!("no focused workspace in the i3 tree"))?;

    let side = |name: &str| workspace["gaps"][name].as_u64().unwrap_or(0) as u32;
    Ok(Gaps {
        left: side("left"),
        right: side("right"),
        top: side("top"),
        bottom: side("bottom"),
    })
}

/// Every window i3 manages, which is every window except tauler's panels.
pub fn client_rects(desktop: &Desktop) -> Result<Vec<Rect>> {
    let tree = get_tree(desktop)?;
    let mut out = Vec::new();
    collect_clients(&tree, &mut out);
    Ok(out)
}

fn get_tree(desktop: &Desktop) -> Result<Value> {
    let raw = desktop.exec(&["i3-msg", "-t", "get_tree"])?;
    serde_json::from_str(&raw).map_err(|e| anyhow!("i3 get_tree returned unparseable JSON: {e}"))
}

fn focused_workspace(node: &Value) -> Option<&Value> {
    if node["type"] == "workspace" && contains_focus(node) {
        return Some(node);
    }
    children(node).into_iter().find_map(focused_workspace)
}

/// i3 marks only the focused *leaf* with `focused: true`; a workspace owns the
/// focus when the focused node is somewhere beneath it. An empty workspace is
/// itself the focused node.
fn contains_focus(node: &Value) -> bool {
    if node["focused"] == Value::Bool(true) {
        return true;
    }
    children(node).into_iter().any(contains_focus)
}

fn collect_clients(node: &Value, out: &mut Vec<Rect>) {
    if node["window"].is_number() {
        if let Some(rect) = rect_of(&node["rect"]) {
            out.push(rect);
        }
    }
    for child in children(node) {
        collect_clients(child, out);
    }
}

fn children(node: &Value) -> Vec<&Value> {
    ["nodes", "floating_nodes"]
        .iter()
        .filter_map(|key| node[*key].as_array())
        .flatten()
        .collect()
}

fn rect_of(value: &Value) -> Option<Rect> {
    Some(Rect {
        x: value["x"].as_i64()? as i32,
        y: value["y"].as_i64()? as i32,
        width: value["width"].as_u64()? as u32,
        height: value["height"].as_u64()? as u32,
    })
}

/// Fail unless every managed window sits inside what the gaps leave free.
///
/// This is the reservation contract from the reader's side: gaps that i3
/// accepted but did not act on look identical in `get_tree` to gaps that
/// worked.
pub fn assert_clients_respect(gaps: Gaps, screen: crate::Screen, clients: &[Rect]) -> Result<()> {
    let free = Rect {
        x: gaps.left as i32,
        y: gaps.top as i32,
        width: screen.width.saturating_sub(gaps.left + gaps.right),
        height: screen.height.saturating_sub(gaps.top + gaps.bottom),
    };

    for client in clients {
        let inside = client.x >= free.x
            && client.y >= free.y
            && client.x + client.width as i32 <= free.x + free.width as i32
            && client.y + client.height as i32 <= free.y + free.height as i32;
        if !inside {
            bail!("client at {client} escapes the free area {free} (gaps: {gaps})");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> Value {
        serde_json::json!({
            "type": "root",
            "nodes": [{
                "type": "output",
                "nodes": [{
                    "type": "con",
                    "nodes": [{
                        "type": "workspace",
                        "gaps": { "left": 272, "right": 0, "top": 26, "bottom": 26 },
                        "nodes": [{
                            "type": "con",
                            "window": 4194307,
                            "focused": true,
                            "rect": { "x": 272, "y": 26, "width": 1648, "height": 1028 }
                        }]
                    }]
                }]
            }]
        })
    }

    #[test]
    fn finds_the_workspace_holding_the_focused_leaf() {
        let tree = tree();
        let workspace = focused_workspace(&tree).expect("focused workspace");
        assert_eq!(workspace["gaps"]["left"], 272);
    }

    #[test]
    fn collects_only_managed_windows() {
        let mut clients = Vec::new();
        collect_clients(&tree(), &mut clients);
        assert_eq!(
            clients,
            vec![Rect {
                x: 272,
                y: 26,
                width: 1648,
                height: 1028
            }]
        );
    }

    #[test]
    fn a_client_over_the_sidebar_is_a_failure() {
        let screen = crate::Screen::default();
        let gaps = Gaps {
            left: 272,
            ..Gaps::default()
        };
        let overlapping = Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        };
        assert!(assert_clients_respect(gaps, screen, &[overlapping]).is_err());
    }
}
