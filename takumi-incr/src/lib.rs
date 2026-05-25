use std::collections::{BTreeSet, HashMap, HashSet};
use std::num::NonZeroUsize;
use tracing::debug;

use lru::LruCache;
use serde::Deserialize;

use anyhow::Result;
use takumi::{
    layout::{node::Node, Viewport},
    rendering::{
        measure_layout as takumi_measure_layout, render as takumi_render, MeasuredNode,
        RenderOptions,
    },
};

use optative::reconcile::Reconcile;
use optative::{Lifecycle, OptativeSet};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

pub const TILE_SIZE: u32 = 24;
pub const SHADOW_BUF: u32 = 32;
pub const TILE_CACHE_MB: usize = 10;
pub const BAILOUT_MIN_PIXELS: u64 = 50_000;
pub const BAILOUT_DIRTY_RATIO: f32 = 0.70;

#[allow(dead_code)]
pub const MERGE_THRESHOLD: u32 = 2 * SHADOW_BUF / TILE_SIZE;

pub const O_FIXED_MS: f64 = 3.48;
pub const K_AREA: f64 = 1.39e-4;
pub const K_NODES: f64 = 0.001;

// ---------------------------------------------------------------------------
// Shared helper: parse a JSON value into a takumi Node
// ---------------------------------------------------------------------------

pub fn parse_layout(value: &serde_json::Value) -> Result<Node, serde_json::Error> {
    Node::deserialize(value)
}

// ---------------------------------------------------------------------------
// IncrNode
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub enum IncrNode {
    Text {
        id: String,
        text: String,
        tw: String,
        style: Option<serde_json::Value>,
    },
    Image {
        id: String,
        src: String,
        width: Option<f32>,
        height: Option<f32>,
        tw: String,
        style: Option<serde_json::Value>,
    },
    Container {
        id: String,
        tw: String,
        style: Option<serde_json::Value>,
        children: Vec<IncrNode>,
    },
}

impl IncrNode {
    pub fn id(&self) -> &str {
        match self {
            Self::Text { id, .. } | Self::Image { id, .. } | Self::Container { id, .. } => id,
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        match self {
            Self::Text {
                text, tw, style, ..
            } => {
                let mut v = serde_json::json!({"type":"text","text":text,"tw":tw});
                if let Some(s) = style {
                    v["style"] = s.clone();
                }
                v
            }
            Self::Image {
                src,
                width,
                height,
                tw,
                style,
                ..
            } => {
                let mut v = serde_json::json!({"type":"image","src":src});
                if let Some(w) = width {
                    v["width"] = serde_json::json!(w);
                }
                if let Some(h) = height {
                    v["height"] = serde_json::json!(h);
                }
                if !tw.is_empty() {
                    v["tw"] = serde_json::json!(tw);
                }
                if let Some(s) = style {
                    v["style"] = s.clone();
                }
                v
            }
            Self::Container {
                tw,
                style,
                children,
                ..
            } => {
                let ch: Vec<_> = children.iter().map(|c| c.to_json()).collect();
                let mut v = serde_json::json!({"type":"container","tw":tw,"children":ch});
                if let Some(s) = style {
                    v["style"] = s.clone();
                }
                v
            }
        }
    }

    pub fn from_json(v: &serde_json::Value) -> Option<Self> {
        Self::from_json_at(v, "r")
    }

    fn from_json_at(v: &serde_json::Value, path: &str) -> Option<Self> {
        let obj = v.as_object()?;
        let ty = obj
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or("container");
        let id = obj
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| path.to_string());
        let tw = obj
            .get("tw")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let style = obj.get("style").cloned();

        match ty {
            "text" => {
                let text = obj
                    .get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                Some(Self::Text {
                    id,
                    text,
                    tw,
                    style,
                })
            }
            "image" => {
                let src = obj
                    .get("src")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let width = obj.get("width").and_then(|v| v.as_f64()).map(|f| f as f32);
                let height = obj.get("height").and_then(|v| v.as_f64()).map(|f| f as f32);
                Some(Self::Image {
                    id,
                    src,
                    width,
                    height,
                    tw,
                    style,
                })
            }
            _ => {
                let children = obj
                    .get("children")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .enumerate()
                            .filter_map(|(i, child)| {
                                let child_path = format!("{path}/{i}");
                                Self::from_json_at(child, &child_path)
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                Some(Self::Container {
                    id,
                    tw,
                    style,
                    children,
                })
            }
        }
    }

    pub fn leaf_hash(&self) -> u64 {
        let mut h: u64 = 14695981039346656037;
        match self {
            Self::Text {
                text, tw, style, ..
            } => {
                h = fnv_mix(h, b"text");
                h = fnv_mix(h, tw.as_bytes());
                h = fnv_mix(h, text.as_bytes());
                if let Some(s) = style {
                    h = fnv_mix(h, s.to_string().as_bytes());
                }
            }
            Self::Image {
                src,
                width,
                height,
                tw,
                style,
                ..
            } => {
                h = fnv_mix(h, b"image");
                h = fnv_mix(h, tw.as_bytes());
                h = fnv_mix(h, src.as_bytes());
                if let Some(w) = width {
                    h = fnv_mix(h, &w.to_bits().to_le_bytes());
                }
                if let Some(ht) = height {
                    h = fnv_mix(h, &ht.to_bits().to_le_bytes());
                }
                if let Some(s) = style {
                    h = fnv_mix(h, s.to_string().as_bytes());
                }
            }
            Self::Container { tw, style, .. } => {
                h = fnv_mix(h, b"container");
                h = fnv_mix(h, tw.as_bytes());
                if let Some(s) = style {
                    h = fnv_mix(h, s.to_string().as_bytes());
                }
            }
        }
        h
    }
}

impl std::fmt::Display for IncrNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.id())
    }
}

// ---------------------------------------------------------------------------
// IncrNodeState
// ---------------------------------------------------------------------------

pub struct IncrNodeState {
    pub id: String,
    pub leaf_hash: u64,
    pub children: OptativeSet<IncrNode>,
}

// ---------------------------------------------------------------------------
// Ctx
// ---------------------------------------------------------------------------

pub struct Ctx {
    pub changed_ids: Vec<String>,
    pub node_dims: HashMap<String, (f32, f32)>,
}

// ---------------------------------------------------------------------------
// Lifecycle
// ---------------------------------------------------------------------------

impl Lifecycle for IncrNode {
    type Key = String;
    type State = IncrNodeState;
    type Context = Ctx;
    type Output = ();
    type Error = anyhow::Error;

    fn key(&self) -> String {
        self.id().to_string()
    }

    fn display_name(&self) -> String {
        self.id().to_string()
    }

    fn enter(self, ctx: &mut Ctx, _: &mut ()) -> Result<IncrNodeState> {
        ctx.changed_ids.push(self.id().to_string());
        let id = self.id().to_string();
        let hash = self.leaf_hash();
        let child_list = match &self {
            IncrNode::Container { children, .. } => children.clone(),
            _ => vec![],
        };
        let mut children: OptativeSet<IncrNode> = OptativeSet::new();
        children.reconcile(child_list, ctx, &mut ());
        Ok(IncrNodeState {
            id,
            leaf_hash: hash,
            children,
        })
    }

    fn reconcile_self(self, state: &mut IncrNodeState, ctx: &mut Ctx, _: &mut ()) -> Result<()> {
        let new_hash = self.leaf_hash();
        if new_hash != state.leaf_hash {
            ctx.changed_ids.push(self.id().to_string());
            state.leaf_hash = new_hash;
        }
        let child_list = match &self {
            IncrNode::Container { children, .. } => children.clone(),
            _ => vec![],
        };
        state.children.reconcile(child_list, ctx, &mut ());
        Ok(())
    }

    fn exit(mut state: IncrNodeState, ctx: &mut Ctx, _: &mut ()) -> Result<()> {
        ctx.changed_ids.push(state.id.clone());
        state.children.reconcile(vec![], ctx, &mut ());
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Rect
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

// ---------------------------------------------------------------------------
// TileConfig
// ---------------------------------------------------------------------------

pub struct TileConfig {
    pub tile_size: u32,
    pub shadow_buf: u32,
    pub merge_threshold: u32,
    pub cache_cap: NonZeroUsize,
}

impl TileConfig {
    pub fn new(tile_size: u32) -> Self {
        let shadow_buf = SHADOW_BUF;
        let tile_bytes = (tile_size * tile_size * 4) as usize;
        let cache_cap =
            NonZeroUsize::new((TILE_CACHE_MB * 1024 * 1024).div_ceil(tile_bytes)).unwrap();
        Self {
            tile_size,
            shadow_buf,
            merge_threshold: 2 * shadow_buf / tile_size,
            cache_cap,
        }
    }
}

// ---------------------------------------------------------------------------
// CostModel
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct CostModel {
    pub o_fixed: f64,
    pub k_area: f64,
    pub k_nodes: f64,
}

impl Default for CostModel {
    fn default() -> Self {
        Self {
            o_fixed: O_FIXED_MS,
            k_area: K_AREA,
            k_nodes: K_NODES,
        }
    }
}

// ---------------------------------------------------------------------------
// RenderBand
// ---------------------------------------------------------------------------

pub struct RenderBand {
    pub min_tx: u32,
    pub max_tx: u32,
    pub min_ty: u32,
    pub max_ty: u32,
    pub tiles: Vec<(u32, u32)>,
}

// ---------------------------------------------------------------------------
// RenderCandidate
// ---------------------------------------------------------------------------

pub struct RenderCandidate {
    pub min_tx: u32,
    pub max_tx: u32,
    pub min_ty: u32,
    pub max_ty: u32,
    pub tiles: Vec<(u32, u32)>,
    pub node_set: BTreeSet<String>,
}

// ---------------------------------------------------------------------------
// Pipeline helper functions
// ---------------------------------------------------------------------------

pub fn fnv_mix(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    h
}

pub fn layout_tw(tw: &str) -> String {
    tw.split_whitespace()
        .filter(|c| {
            matches!(
                *c,
                "ml-auto" | "mr-auto" | "mx-auto" | "mt-auto" | "mb-auto" | "my-auto"
            ) || c.starts_with("flex-")
                || c.starts_with("grow")
                || c.starts_with("shrink")
                || c.starts_with("self-")
                || c.starts_with("justify-self-")
                || c.starts_with("order-")
                || c.starts_with("w-")
                || c.starts_with("h-")
                || c.starts_with("min-w-")
                || c.starts_with("max-w-")
                || c.starts_with("min-h-")
                || c.starts_with("max-h-")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn visual_tw(tw: &str) -> String {
    tw.split_whitespace()
        .filter(|c| {
            c.starts_with("bg-")
                || c.starts_with("shadow")
                || c.starts_with("rounded")
                || c.starts_with("border")
                || c.starts_with("ring")
                || c.starts_with("opacity")
                || c.starts_with("blur")
                || c.starts_with("backdrop")
                || c.starts_with("brightness")
                || c.starts_with("contrast")
                || c.starts_with("saturate")
                || c.starts_with("grayscale")
                || c.starts_with("invert")
                || c.starts_with("hue-rotate")
                || c.starts_with("drop-shadow")
                || c.starts_with("mix-blend")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn has_overflow_clip(tw: &str) -> bool {
    tw.split_whitespace().any(|c| c.starts_with("overflow-"))
}

pub fn stub_scene_json(node: &IncrNode, dims: &HashMap<String, (f32, f32)>) -> serde_json::Value {
    match node {
        IncrNode::Text { id, tw, .. } => {
            let (w, h) = dims.get(id.as_str()).copied().unwrap_or((0.0, 0.0));
            let ltw = layout_tw(tw);
            if ltw.is_empty() {
                serde_json::json!({"type":"container","style":{"width":w,"height":h}})
            } else {
                serde_json::json!({"type":"container","tw":ltw,"style":{"width":w,"height":h}})
            }
        }
        IncrNode::Image { .. } => node.to_json(),
        IncrNode::Container { tw, children, .. } => {
            if children.is_empty() {
                node.to_json()
            } else {
                let ch: Vec<_> = children.iter().map(|c| stub_scene_json(c, dims)).collect();
                serde_json::json!({"type":"container","tw":tw,"children":ch})
            }
        }
    }
}

pub fn measure_natural(node: &IncrNode, global: &takumi::GlobalContext) -> (f32, f32) {
    let json = node.to_json();
    let n = parse_layout(&json).unwrap_or_else(|_| Node::container(vec![]));
    let m = takumi_measure_layout(
        RenderOptions::builder()
            .global(global)
            .viewport(Viewport::new((None, None)))
            .node(n)
            .build(),
    )
    .expect("measure natural");
    (m.width, m.height)
}

pub fn collect_bboxes(
    measured: &MeasuredNode,
    node: &IncrNode,
    bboxes: &mut HashMap<String, Rect>,
) {
    bboxes.insert(
        node.id().to_string(),
        Rect {
            x: measured.transform[4],
            y: measured.transform[5],
            w: measured.width,
            h: measured.height,
        },
    );
    if let IncrNode::Container { children, .. } = node {
        let is_absolute = |c: &IncrNode| -> bool {
            let tw = match c {
                IncrNode::Text { tw, .. }
                | IncrNode::Image { tw, .. }
                | IncrNode::Container { tw, .. } => tw.as_str(),
            };
            tw.split_whitespace().any(|t| t == "absolute")
        };
        let in_flow_f: Vec<&IncrNode> = children.iter().filter(|c| !is_absolute(c)).collect();
        let abs_f: Vec<&IncrNode> = children.iter().filter(|c| is_absolute(c)).collect();
        if abs_f.is_empty() {
            for (m, f) in measured.children.iter().zip(in_flow_f.iter()) {
                collect_bboxes(m, f, bboxes);
            }
        } else {
            let n_if = in_flow_f.len();
            let n_ab = abs_f.len();
            let n_m = measured.children.len();
            let (in_flow_m, abs_m): (Vec<&MeasuredNode>, Vec<&MeasuredNode>) =
                if n_m == n_if + 1 && !measured.children[n_if].children.is_empty() {
                    let ph = &measured.children[n_if];
                    (
                        measured.children[..n_if].iter().collect(),
                        ph.children.iter().collect(),
                    )
                } else {
                    let split = n_if.min(n_m);
                    (
                        measured.children[..split].iter().collect(),
                        measured.children[split..split + n_ab.min(n_m - split)]
                            .iter()
                            .collect(),
                    )
                };
            for (m, f) in in_flow_m.iter().zip(in_flow_f.iter()) {
                collect_bboxes(m, f, bboxes);
            }
            for (m, f) in abs_m.iter().zip(abs_f.iter()) {
                collect_bboxes(m, f, bboxes);
            }
        }
    }
}

pub fn collect_nested_whitelist(
    node: &IncrNode,
    bboxes: &HashMap<String, Rect>,
    node_set: &BTreeSet<String>,
    parent_x: f32,
    parent_y: f32,
    out: &mut Vec<serde_json::Value>,
) {
    let id = node.id();
    let Some(r) = bboxes.get(id) else {
        return;
    };
    let lx = r.x - parent_x;
    let ly = r.y - parent_y;
    let in_set = node_set.contains(id);
    match node {
        IncrNode::Text { text, tw, .. } => {
            if in_set {
                out.push(serde_json::json!({"type":"text","text":text,"tw":tw,
                    "style":{"position":"absolute","left":lx,"top":ly,"width":r.w}}));
            }
        }
        IncrNode::Image {
            src, width, height, ..
        } => {
            if in_set {
                let w = width.unwrap_or(0.0);
                let h = height.unwrap_or(0.0);
                out.push(serde_json::json!({"type":"image","src":src,
                    "style":{"display":"block","position":"absolute","left":lx,"top":ly,
                             "width":w,"height":h}}));
            }
        }
        IncrNode::Container {
            tw,
            style,
            children,
            ..
        } => {
            if has_overflow_clip(tw) {
                let mut ch = Vec::new();
                for child in children {
                    collect_nested_whitelist(child, bboxes, node_set, r.x, r.y, &mut ch);
                }
                if in_set {
                    let s = container_tile_style(
                        style,
                        lx,
                        ly,
                        r.w,
                        r.h,
                        Some(("overflow", serde_json::json!("hidden"))),
                    );
                    out.push(serde_json::json!({"type":"container","tw":visual_tw(tw),
                        "style":s,"children":ch}));
                } else {
                    out.extend(ch);
                }
            } else {
                if in_set {
                    let s = container_tile_style(style, lx, ly, r.w, r.h, None);
                    out.push(serde_json::json!({"type":"container","tw":visual_tw(tw),"style":s}));
                }
                for child in children {
                    collect_nested_whitelist(child, bboxes, node_set, parent_x, parent_y, out);
                }
            }
        }
    }
}

/// Layout properties that we always override with bbox-computed values.
const LAYOUT_KEYS: &[&str] = &[
    "position",
    "left",
    "right",
    "top",
    "bottom",
    "width",
    "height",
    "display",
    "flex",
    "flexDirection",
    "flexWrap",
    "flexGrow",
    "flexShrink",
    "flexBasis",
    "alignSelf",
    "justifySelf",
    "margin",
    "marginTop",
    "marginBottom",
    "marginLeft",
    "marginRight",
    "padding",
    "paddingTop",
    "paddingBottom",
    "paddingLeft",
    "paddingRight",
    "overflow",
    "overflowX",
    "overflowY",
];

/// Build a style object for a tile-scene container: visual inline styles from the
/// original node merged with explicit bbox position/size (bbox values take precedence).
fn container_tile_style(
    node_style: &Option<serde_json::Value>,
    lx: f32,
    ly: f32,
    w: f32,
    h: f32,
    extra: Option<(&str, serde_json::Value)>,
) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    if let Some(serde_json::Value::Object(ns)) = node_style {
        for (k, v) in ns {
            if !LAYOUT_KEYS.contains(&k.as_str()) {
                map.insert(k.clone(), v.clone());
            }
        }
    }
    map.insert("position".to_string(), serde_json::json!("absolute"));
    map.insert("left".to_string(), serde_json::json!(lx));
    map.insert("top".to_string(), serde_json::json!(ly));
    map.insert("width".to_string(), serde_json::json!(w));
    map.insert("height".to_string(), serde_json::json!(h));
    if let Some((k, v)) = extra {
        map.insert(k.to_string(), v);
    }
    serde_json::Value::Object(map)
}

#[allow(clippy::too_many_arguments)]
fn is_text_color_class(c: &str) -> bool {
    let Some(suffix) = c.strip_prefix("text-") else {
        return false;
    };
    if matches!(
        suffix,
        "xs" | "sm"
            | "base"
            | "lg"
            | "xl"
            | "2xl"
            | "3xl"
            | "4xl"
            | "5xl"
            | "6xl"
            | "7xl"
            | "8xl"
            | "9xl"
    ) {
        return false;
    }
    if suffix.starts_with('[') {
        // Arbitrary value — color if not a CSS length (px/rem/em/%)
        let inner = suffix.trim_start_matches('[').trim_end_matches(']');
        return !inner.ends_with("px")
            && !inner.ends_with("rem")
            && !inner.ends_with("em")
            && !inner.ends_with('%')
            && !inner.chars().next().is_some_and(|ch| ch.is_ascii_digit());
    }
    !suffix.chars().next().is_some_and(|ch| ch.is_ascii_digit())
}

fn inheritable_tw(tw: &str) -> String {
    tw.split_whitespace()
        .filter(|c| is_text_color_class(c))
        .collect::<Vec<_>>()
        .join(" ")
}

#[allow(clippy::too_many_arguments)]
pub fn collect_flat_whitelist(
    node: &IncrNode,
    bboxes: &HashMap<String, Rect>,
    node_set: &BTreeSet<String>,
    qx: f32,
    qy: f32,
    qw: f32,
    qh: f32,
    out: &mut Vec<serde_json::Value>,
    tc: &TileConfig,
) {
    collect_flat_whitelist_inner(node, bboxes, node_set, qx, qy, qw, qh, out, tc, "");
}

#[allow(clippy::too_many_arguments)]
fn collect_flat_whitelist_inner(
    node: &IncrNode,
    bboxes: &HashMap<String, Rect>,
    node_set: &BTreeSet<String>,
    qx: f32,
    qy: f32,
    qw: f32,
    qh: f32,
    out: &mut Vec<serde_json::Value>,
    tc: &TileConfig,
    inherited: &str,
) {
    let id = node.id();
    let Some(r) = bboxes.get(id) else {
        return;
    };
    let buf = tc.shadow_buf as f32;
    if r.x + r.w + buf <= qx
        || r.x - buf >= qx + qw
        || r.y + r.h + buf <= qy
        || r.y - buf >= qy + qh
    {
        return;
    }

    let in_set = node_set.contains(id);
    let lx = r.x - qx;
    let ly = r.y - qy;
    match node {
        IncrNode::Text { text, tw, .. } => {
            if in_set {
                // Apply inherited color/font if the node has no own text-color class
                let has_own_color = tw.split_whitespace().any(is_text_color_class);
                let effective_tw = if !has_own_color && !inherited.is_empty() {
                    format!("{} {}", inherited, tw)
                } else {
                    tw.clone()
                };
                out.push(
                    serde_json::json!({"type":"text","text":text,"tw":effective_tw,
                    "style":{"position":"absolute","left":lx,"top":ly,"width":r.w}}),
                );
            }
        }
        IncrNode::Image {
            src, width, height, ..
        } => {
            if in_set {
                let w = width.unwrap_or(0.0);
                let h = height.unwrap_or(0.0);
                out.push(serde_json::json!({"type":"image","src":src,
                    "style":{"display":"block","position":"absolute","left":lx,"top":ly,
                             "width":w,"height":h}}));
            }
        }
        IncrNode::Container {
            tw,
            style,
            children,
            ..
        } => {
            // Accumulate inheritable classes for children
            let own_inheritable = inheritable_tw(tw);
            let child_inherited = if own_inheritable.is_empty() {
                inherited.to_string()
            } else if inherited.is_empty() {
                own_inheritable
            } else {
                format!("{} {}", inherited, own_inheritable)
            };

            if has_overflow_clip(tw) {
                let mut ch = Vec::new();
                for child in children {
                    collect_nested_whitelist(child, bboxes, node_set, r.x, r.y, &mut ch);
                }
                if in_set {
                    let s = container_tile_style(
                        style,
                        lx,
                        ly,
                        r.w,
                        r.h,
                        Some(("overflow", serde_json::json!("hidden"))),
                    );
                    out.push(serde_json::json!({"type":"container","tw":visual_tw(tw),
                        "style":s,"children":ch}));
                } else {
                    out.extend(ch);
                }
            } else {
                if in_set {
                    if children.is_empty()
                        && style.as_ref().and_then(|s| s["display"].as_str())
                            == Some("inline-block")
                    {
                        let bg_tw = tw
                            .split_whitespace()
                            .find(|t| t.starts_with("bg-"))
                            .unwrap_or("");
                        let s = container_tile_style(
                            style,
                            lx,
                            ly,
                            r.w,
                            r.h,
                            Some(("display", serde_json::json!("block"))),
                        );
                        out.push(serde_json::json!({"type":"container","tw":bg_tw,"style":s}));
                    } else {
                        let s = container_tile_style(style, lx, ly, r.w, r.h, None);
                        out.push(
                            serde_json::json!({"type":"container","tw":visual_tw(tw),"style":s}),
                        );
                    }
                }
                for child in children {
                    collect_flat_whitelist_inner(
                        child,
                        bboxes,
                        node_set,
                        qx,
                        qy,
                        qw,
                        qh,
                        out,
                        tc,
                        &child_inherited,
                    );
                }
            }
        }
    }
}

pub fn mark_dirty(
    r: &Rect,
    tile: u32,
    scene_w: u32,
    scene_h: u32,
    dirty: &mut HashSet<(u32, u32)>,
    tc: &TileConfig,
) {
    let t = tile as f32;
    let buf = tc.shadow_buf as f32;
    let col0 = ((r.x - buf) / t).floor() as i32;
    let row0 = ((r.y - buf) / t).floor() as i32;
    let col1 = ((r.x + r.w + buf) / t).ceil() as i32;
    let row1 = ((r.y + r.h + buf) / t).ceil() as i32;
    let max_col = scene_w.div_ceil(tile) as i32;
    let max_row = scene_h.div_ceil(tile) as i32;
    for row in row0.max(0)..row1.min(max_row) {
        for col in col0.max(0)..col1.min(max_col) {
            dirty.insert((col as u32, row as u32));
        }
    }
}

pub fn tiles_for_bbox(r: &Rect, cols: u32, rows: u32, tc: &TileConfig) -> Vec<(u32, u32)> {
    let buf = tc.shadow_buf as f32;
    let c0 = ((r.x - buf) / tc.tile_size as f32).floor().max(0.0) as u32;
    let r0 = ((r.y - buf) / tc.tile_size as f32).floor().max(0.0) as u32;
    let c1 = ((r.x + r.w + buf) / tc.tile_size as f32)
        .ceil()
        .min(cols as f32) as u32;
    let r1 = ((r.y + r.h + buf) / tc.tile_size as f32)
        .ceil()
        .min(rows as f32) as u32;
    let mut out = Vec::new();
    for ty in r0..r1 {
        for tx in c0..c1 {
            out.push((tx, ty));
        }
    }
    out
}

pub fn stitch(
    frame: &mut [u8],
    frame_w: u32,
    frame_h: u32,
    tile_px: &[u8],
    tile: u32,
    px_x: u32,
    px_y: u32,
) {
    let copy_w = tile.min(frame_w.saturating_sub(px_x)) as usize;
    let copy_h = tile.min(frame_h.saturating_sub(px_y));
    for row in 0..copy_h {
        let src_row = (row * tile * 4) as usize;
        let dst_row = (((px_y + row) * frame_w + px_x) * 4) as usize;
        for col in 0..copy_w {
            let s = src_row + col * 4;
            let d = dst_row + col * 4;
            let a = tile_px[s + 3] as u32;
            if a == 255 {
                frame[d..d + 4].copy_from_slice(&tile_px[s..s + 4]);
            } else if a > 0 {
                let inv_a = 255 - a;
                frame[d] = ((tile_px[s] as u32 * a + frame[d] as u32 * inv_a + 127) / 255) as u8;
                frame[d + 1] =
                    ((tile_px[s + 1] as u32 * a + frame[d + 1] as u32 * inv_a + 127) / 255) as u8;
                frame[d + 2] =
                    ((tile_px[s + 2] as u32 * a + frame[d + 2] as u32 * inv_a + 127) / 255) as u8;
                frame[d + 3] = 255;
            }
            // a == 0: no-op — transparent tile pixels preserve existing frame content
        }
    }
}

pub fn tile_fingerprint(
    tx: u32,
    ty: u32,
    tile_node_map: &HashMap<(u32, u32), BTreeSet<String>>,
    bboxes: &HashMap<String, Rect>,
    node_map: &HashMap<&str, &IncrNode>,
) -> u64 {
    let empty = BTreeSet::new();
    let node_set = tile_node_map.get(&(tx, ty)).unwrap_or(&empty);
    let mut h: u64 = 14695981039346656037;
    h = fnv_mix(h, &tx.to_le_bytes());
    h = fnv_mix(h, &ty.to_le_bytes());
    for id in node_set {
        h = fnv_mix(h, id.as_bytes());
        if let Some(r) = bboxes.get(id.as_str()) {
            h = fnv_mix(h, &r.x.to_bits().to_le_bytes());
            h = fnv_mix(h, &r.y.to_bits().to_le_bytes());
            h = fnv_mix(h, &r.w.to_bits().to_le_bytes());
            h = fnv_mix(h, &r.h.to_bits().to_le_bytes());
        }
        if let Some(&node) = node_map.get(id.as_str()) {
            match node {
                IncrNode::Text {
                    text, tw, style, ..
                } => {
                    h = fnv_mix(h, b"text|");
                    h = fnv_mix(h, tw.as_bytes());
                    h = fnv_mix(h, b"|");
                    h = fnv_mix(h, text.as_bytes());
                    h = fnv_mix(h, b"|");
                    if let Some(s) = style {
                        h = fnv_mix(h, s.to_string().as_bytes());
                    }
                }
                IncrNode::Image {
                    src,
                    width,
                    height,
                    tw,
                    style,
                    ..
                } => {
                    h = fnv_mix(h, b"image|");
                    h = fnv_mix(h, tw.as_bytes());
                    h = fnv_mix(h, b"|");
                    h = fnv_mix(h, src.as_bytes());
                    h = fnv_mix(h, b"|");
                    if let Some(w) = width {
                        h = fnv_mix(h, &w.to_bits().to_le_bytes());
                    }
                    if let Some(ht) = height {
                        h = fnv_mix(h, &ht.to_bits().to_le_bytes());
                    }
                    if let Some(s) = style {
                        h = fnv_mix(h, s.to_string().as_bytes());
                    }
                }
                IncrNode::Container { tw, style, .. } => {
                    h = fnv_mix(h, b"container|");
                    h = fnv_mix(h, tw.as_bytes());
                    h = fnv_mix(h, b"|");
                    if let Some(s) = style {
                        h = fnv_mix(h, s.to_string().as_bytes());
                    }
                }
            }
        }
        h = fnv_mix(h, b"\0");
    }
    h
}

pub fn build_node_map(root: &IncrNode) -> HashMap<&str, &IncrNode> {
    let mut map = HashMap::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        map.insert(node.id(), node);
        if let IncrNode::Container { children, .. } = node {
            stack.extend(children.iter());
        }
    }
    map
}

pub fn build_tile_node_map(
    bboxes: &HashMap<String, Rect>,
    cols: u32,
    rows: u32,
    tc: &TileConfig,
) -> HashMap<(u32, u32), BTreeSet<String>> {
    let mut map: HashMap<(u32, u32), BTreeSet<String>> = HashMap::new();
    for (id, r) in bboxes {
        for tile in tiles_for_bbox(r, cols, rows, tc) {
            map.entry(tile).or_default().insert(id.clone());
        }
    }
    map
}

pub fn candidate_cost(c: &RenderCandidate, cm: &CostModel, tc: &TileConfig) -> f64 {
    let w = ((c.max_tx - c.min_tx + 1) * tc.tile_size + 2 * tc.shadow_buf) as f64;
    let h = ((c.max_ty - c.min_ty + 1) * tc.tile_size + 2 * tc.shadow_buf) as f64;
    cm.o_fixed + cm.k_area * w * h + cm.k_nodes * c.node_set.len() as f64
}

pub fn merge_candidates(a: &RenderCandidate, b: &RenderCandidate) -> RenderCandidate {
    let mut tiles = a.tiles.clone();
    tiles.extend_from_slice(&b.tiles);
    let mut node_set = a.node_set.clone();
    for id in &b.node_set {
        node_set.insert(id.clone());
    }
    RenderCandidate {
        min_tx: a.min_tx.min(b.min_tx),
        max_tx: a.max_tx.max(b.max_tx),
        min_ty: a.min_ty.min(b.min_ty),
        max_ty: a.max_ty.max(b.max_ty),
        tiles,
        node_set,
    }
}

pub fn estimated_area(bands: &[RenderBand], tc: &TileConfig) -> u64 {
    bands
        .iter()
        .map(|b| {
            let w = ((b.max_tx - b.min_tx + 1) * tc.tile_size + 2 * tc.shadow_buf) as u64;
            let h = ((b.max_ty - b.min_ty + 1) * tc.tile_size + 2 * tc.shadow_buf) as u64;
            w * h
        })
        .sum()
}

pub fn compute_bands_y(dirty: &HashSet<(u32, u32)>, tc: &TileConfig) -> Vec<RenderBand> {
    let mut tiles: Vec<(u32, u32)> = dirty.iter().copied().collect();
    tiles.sort_by_key(|&(_, ty)| ty);
    let mut bands: Vec<RenderBand> = Vec::new();
    for (tx, ty) in tiles {
        if let Some(b) = bands.last_mut() {
            if ty - b.max_ty <= tc.merge_threshold {
                b.max_ty = b.max_ty.max(ty);
                b.min_tx = b.min_tx.min(tx);
                b.max_tx = b.max_tx.max(tx);
                b.tiles.push((tx, ty));
                continue;
            }
        }
        bands.push(RenderBand {
            min_tx: tx,
            max_tx: tx,
            min_ty: ty,
            max_ty: ty,
            tiles: vec![(tx, ty)],
        });
    }
    bands
}

pub fn compute_bands_x(dirty: &HashSet<(u32, u32)>, tc: &TileConfig) -> Vec<RenderBand> {
    let mut tiles: Vec<(u32, u32)> = dirty.iter().copied().collect();
    tiles.sort_by_key(|&(tx, _)| tx);
    let mut bands: Vec<RenderBand> = Vec::new();
    for (tx, ty) in tiles {
        if let Some(b) = bands.last_mut() {
            if tx - b.max_tx <= tc.merge_threshold {
                b.max_tx = b.max_tx.max(tx);
                b.min_ty = b.min_ty.min(ty);
                b.max_ty = b.max_ty.max(ty);
                b.tiles.push((tx, ty));
                continue;
            }
        }
        bands.push(RenderBand {
            min_tx: tx,
            max_tx: tx,
            min_ty: ty,
            max_ty: ty,
            tiles: vec![(tx, ty)],
        });
    }
    bands
}

pub fn compute_candidates(
    dirty: &HashSet<(u32, u32)>,
    tile_node_map: &HashMap<(u32, u32), BTreeSet<String>>,
    tc: &TileConfig,
) -> Vec<RenderCandidate> {
    let mut groups: HashMap<Vec<String>, HashSet<(u32, u32)>> = HashMap::new();
    for &t in dirty {
        let key: Vec<String> = tile_node_map
            .get(&t)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default();
        groups.entry(key).or_default().insert(t);
    }
    let mut candidates = Vec::new();
    for (node_vec, tiles) in groups {
        let node_set: BTreeSet<String> = node_vec.into_iter().collect();
        let by = compute_bands_y(&tiles, tc);
        let bx = compute_bands_x(&tiles, tc);
        let bands = if estimated_area(&by, tc) <= estimated_area(&bx, tc) {
            by
        } else {
            bx
        };
        for band in bands {
            candidates.push(RenderCandidate {
                min_tx: band.min_tx,
                max_tx: band.max_tx,
                min_ty: band.min_ty,
                max_ty: band.max_ty,
                tiles: band.tiles,
                node_set: node_set.clone(),
            });
        }
    }
    candidates
}

pub fn greedy_merge_candidates(
    mut cs: Vec<RenderCandidate>,
    cm: &CostModel,
    tc: &TileConfig,
) -> Vec<RenderCandidate> {
    loop {
        if cs.len() < 2 {
            break;
        }
        let mut best_gain = 0.0f64;
        let mut best = (0usize, 1usize);
        for i in 0..cs.len() {
            for j in i + 1..cs.len() {
                let merged = merge_candidates(&cs[i], &cs[j]);
                let gain = candidate_cost(&cs[i], cm, tc) + candidate_cost(&cs[j], cm, tc)
                    - candidate_cost(&merged, cm, tc);
                if gain > best_gain {
                    best_gain = gain;
                    best = (i, j);
                }
            }
        }
        if best_gain <= 0.0 {
            break;
        }
        let (i, j) = best;
        let merged = merge_candidates(&cs[i], &cs[j]);
        cs.remove(j);
        cs.remove(i);
        cs.push(merged);
    }
    cs
}

pub fn crop_pixels(pixels: &[u8], src_w: u32, x: u32, y: u32, w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for row in y..y + h {
        let start = ((row * src_w + x) * 4) as usize;
        out.extend_from_slice(&pixels[start..start + (w * 4) as usize]);
    }
    out
}

// ---------------------------------------------------------------------------
// PartialRenderCtx — shared rendering context
// ---------------------------------------------------------------------------

pub struct PartialRenderCtx {
    tile_cache: LruCache<u64, Vec<u8>>,
    pub cost_model: CostModel,
    pub tc: TileConfig,
}

impl PartialRenderCtx {
    pub fn new() -> Self {
        let tile_bytes = (TILE_SIZE * TILE_SIZE * 4) as usize;
        let cache_cap =
            NonZeroUsize::new((TILE_CACHE_MB * 1024 * 1024).div_ceil(tile_bytes)).unwrap();
        Self {
            tile_cache: LruCache::new(cache_cap),
            cost_model: CostModel::default(),
            tc: TileConfig::new(TILE_SIZE),
        }
    }
}

impl Default for PartialRenderCtx {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// PartialRenderScene — per-panel incremental render state
// ---------------------------------------------------------------------------

pub struct PartialRenderScene {
    frame_buf: Vec<u8>,
    prev_stub_bboxes: HashMap<String, Rect>,
    tile_node_map: HashMap<(u32, u32), BTreeSet<String>>,
    incr_set: OptativeSet<IncrNode>,
    ctx: Ctx,
}

impl PartialRenderScene {
    pub fn new() -> Self {
        Self {
            frame_buf: Vec::new(),
            prev_stub_bboxes: HashMap::new(),
            tile_node_map: HashMap::new(),
            incr_set: OptativeSet::new(),
            ctx: Ctx {
                changed_ids: Vec::new(),
                node_dims: HashMap::new(),
            },
        }
    }
}

impl Default for PartialRenderScene {
    fn default() -> Self {
        Self::new()
    }
}

impl PartialRenderScene {
    /// Render one frame. `root` is the panel content as serde_json::Value.
    /// `w` and `h` are physical pixel dimensions. Returns the RGBA pixel buffer.
    pub fn render_frame(
        &mut self,
        pctx: &mut PartialRenderCtx,
        global: &takumi::GlobalContext,
        root: &serde_json::Value,
        w: u32,
        h: u32,
        dpr: f32,
    ) -> &[u8] {
        let Some(root_incr) = IncrNode::from_json(root) else {
            return &self.frame_buf;
        };

        // 1. Reconcile — populates changed_ids
        self.ctx.changed_ids.clear();
        self.incr_set
            .reconcile(vec![root_incr.clone()], &mut self.ctx, &mut ());

        let tc = &pctx.tc;
        let cols = w.div_ceil(tc.tile_size);
        let rows = h.div_ceil(tc.tile_size);

        debug!(changed = self.ctx.changed_ids.len(), "incr reconcile");

        // No-op short-circuit: nothing changed and buffer is populated
        if self.ctx.changed_ids.is_empty() && !self.frame_buf.is_empty() {
            return &self.frame_buf;
        }

        // 2. Measure actual layout to get correct node bboxes
        let node_map = build_node_map(&root_incr);

        let layout_node = parse_layout(root).unwrap_or_else(|_| Node::container(vec![]));
        let t_layout = std::time::Instant::now();
        let measured = takumi_measure_layout(
            RenderOptions::builder()
                .global(global)
                .viewport(Viewport::new((Some(w), Some(h))).with_device_pixel_ratio(dpr))
                .node(layout_node)
                .build(),
        )
        .expect("layout measure");
        debug!(layout_us = t_layout.elapsed().as_micros(), "incr layout");
        let mut new_bboxes = HashMap::new();
        collect_bboxes(&measured, &root_incr, &mut new_bboxes);
        let bboxes: &HashMap<String, Rect> = &new_bboxes;

        // 3. Compute dirty tiles and update tile→node map
        let mut dirty: HashSet<(u32, u32)> = HashSet::new();

        if self.frame_buf.len() != (w * h * 4) as usize {
            // First frame: full build
            self.frame_buf = vec![0u8; (w * h * 4) as usize];
            self.tile_node_map = build_tile_node_map(bboxes, cols, rows, tc);
            for ty in 0..rows {
                for tx in 0..cols {
                    dirty.insert((tx, ty));
                }
            }
        } else {
            // Incremental update
            for id in &self.ctx.changed_ids {
                if let Some(old_r) = self.prev_stub_bboxes.get(id.as_str()) {
                    for (tx, ty) in tiles_for_bbox(old_r, cols, rows, tc) {
                        if let Some(s) = self.tile_node_map.get_mut(&(tx, ty)) {
                            s.remove(id.as_str());
                        }
                    }
                    mark_dirty(old_r, tc.tile_size, w, h, &mut dirty, tc);
                }
                if let Some(new_r) = bboxes.get(id.as_str()) {
                    for (tx, ty) in tiles_for_bbox(new_r, cols, rows, tc) {
                        self.tile_node_map
                            .entry((tx, ty))
                            .or_default()
                            .insert(id.clone());
                    }
                    mark_dirty(new_r, tc.tile_size, w, h, &mut dirty, tc);
                }
            }
            // Check for unchanged nodes that moved due to reflow
            let changed_set: HashSet<&str> =
                self.ctx.changed_ids.iter().map(String::as_str).collect();
            for (id, new_r) in bboxes {
                if changed_set.contains(id.as_str()) {
                    continue;
                }
                if let Some(old_r) = self.prev_stub_bboxes.get(id.as_str()) {
                    if (new_r.x - old_r.x).abs() > 0.5 || (new_r.y - old_r.y).abs() > 0.5 {
                        for (tx, ty) in tiles_for_bbox(old_r, cols, rows, tc) {
                            if let Some(s) = self.tile_node_map.get_mut(&(tx, ty)) {
                                s.remove(id.as_str());
                            }
                        }
                        for (tx, ty) in tiles_for_bbox(new_r, cols, rows, tc) {
                            self.tile_node_map
                                .entry((tx, ty))
                                .or_default()
                                .insert(id.clone());
                        }
                        mark_dirty(new_r, tc.tile_size, w, h, &mut dirty, tc);
                        mark_dirty(old_r, tc.tile_size, w, h, &mut dirty, tc);
                    }
                }
            }
        }

        let dirty_before_cache = dirty.len();
        debug!(
            dirty = dirty_before_cache,
            total = (cols * rows),
            "incr dirty tiles"
        );

        // 4. Cache lookup — stitch hits, remove from dirty
        let fps: HashMap<(u32, u32), u64> = dirty
            .iter()
            .map(|&(tx, ty)| {
                (
                    (tx, ty),
                    tile_fingerprint(tx, ty, &self.tile_node_map, bboxes, &node_map),
                )
            })
            .collect();
        dirty.retain(
            |&(tx, ty)| match pctx.tile_cache.get(&fps[&(tx, ty)]).cloned() {
                Some(px) => {
                    stitch(
                        &mut self.frame_buf,
                        w,
                        h,
                        &px,
                        tc.tile_size,
                        tx * tc.tile_size,
                        ty * tc.tile_size,
                    );
                    false
                }
                None => true,
            },
        );

        let cache_hits = dirty_before_cache - dirty.len();
        debug!(cache_hits, rendered = dirty.len(), "incr cache");

        // 5. Candidate grouping + greedy merge + render
        let candidates: Vec<RenderCandidate> = if dirty.is_empty() {
            vec![]
        } else {
            let raw = compute_candidates(&dirty, &self.tile_node_map, tc);
            greedy_merge_candidates(raw, &pctx.cost_model, tc)
        };

        let t_render = std::time::Instant::now();
        for cand in &candidates {
            let batch_px_x = cand.min_tx * tc.tile_size;
            let batch_px_y = cand.min_ty * tc.tile_size;
            let batch_w = (cand.max_tx - cand.min_tx + 1) * tc.tile_size;
            let batch_h = (cand.max_ty - cand.min_ty + 1) * tc.tile_size;

            let buf = tc.shadow_buf as f32;
            let qx = batch_px_x as f32 - buf;
            let qy = batch_px_y as f32 - buf;
            let qw = batch_w as f32 + 2.0 * buf;
            let qh = batch_h as f32 + 2.0 * buf;
            let canvas_w = batch_w + 2 * tc.shadow_buf;
            let canvas_h = batch_h + 2 * tc.shadow_buf;

            let mut nodes: Vec<serde_json::Value> = Vec::new();
            collect_flat_whitelist(
                &root_incr,
                bboxes,
                &cand.node_set,
                qx,
                qy,
                qw,
                qh,
                &mut nodes,
                tc,
            );

            // All coordinates above are physical pixels (bboxes scaled by DPR).
            // Takumi's CSS engine scales px values by DPR, so we must convert
            // physical coords to CSS coords before passing to the renderer.
            let inv_dpr = 1.0 / dpr;
            for node in &mut nodes {
                if let Some(style) = node.pointer_mut("/style") {
                    for key in ["left", "top", "width", "height"] {
                        if let Some(v) = style.get_mut(key) {
                            if let Some(f) = v.as_f64() {
                                *v = serde_json::Value::from(f * inv_dpr as f64);
                            }
                        }
                    }
                }
            }

            let scene = serde_json::json!({
                "type": "container",
                "style": { "display": "block", "position": "relative",
                    "width": canvas_w as f32 * inv_dpr, "height": canvas_h as f32 * inv_dpr,
                    "overflow": "hidden" },
                "children": nodes
            });
            let node = parse_layout(&scene).unwrap_or_else(|_| Node::container(vec![]));
            // With CSS dimensions = physical/DPR and DPR applied, takumi renders
            // exactly canvas_w × canvas_h physical pixels.
            let cand_img = takumi_render(
                RenderOptions::builder()
                    .global(global)
                    .viewport(Viewport::new((None, None)).with_device_pixel_ratio(dpr))
                    .node(node)
                    .build(),
            )
            .expect("candidate render");
            let (cand_actual_w, _) = cand_img.dimensions();
            let cand_px = cand_img.into_raw();

            for &(tx, ty) in &cand.tiles {
                let px_x = tx * tc.tile_size;
                let px_y = ty * tc.tile_size;
                let off_x = tc.shadow_buf + (tx - cand.min_tx) * tc.tile_size;
                let off_y = tc.shadow_buf + (ty - cand.min_ty) * tc.tile_size;
                let tile_px = crop_pixels(
                    &cand_px,
                    cand_actual_w,
                    off_x,
                    off_y,
                    tc.tile_size,
                    tc.tile_size,
                );
                stitch(
                    &mut self.frame_buf,
                    w,
                    h,
                    &tile_px,
                    tc.tile_size,
                    px_x,
                    px_y,
                );
                pctx.tile_cache.put(fps[&(tx, ty)], tile_px);
            }
        }

        debug!(
            render_us = t_render.elapsed().as_micros(),
            batches = candidates.len(),
            "incr render"
        );

        self.prev_stub_bboxes = new_bboxes;

        &self.frame_buf
    }
}
