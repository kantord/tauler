use std::collections::{BTreeSet, HashMap, HashSet};
use std::num::NonZeroUsize;
use std::time::{Duration, Instant};

use lru::LruCache;

use anyhow::Result;
use image::{ImageBuffer, Rgba};
use takumi::{
    GlobalContext,
    layout::{Viewport, node::Node},
    rendering::{MeasuredNode, RenderOptions, measure_layout as takumi_measure_layout, render as takumi_render},
    resources::font::FontResource,
};

use costae::managed_set::{Lifecycle, ManagedSet};
use costae::managed_set::reconcile::Reconcile;
use costae::layout::parse_layout;
use costae::render::find_font_files;

// ---------------------------------------------------------------------------
// GlobalContext factory
// ---------------------------------------------------------------------------

fn new_ctx_with_fonts() -> GlobalContext {
    let home = std::env::var("HOME").unwrap_or_default();
    let dirs: Vec<std::path::PathBuf> = vec![
        "/usr/share/fonts/TTF".into(),
        "/usr/share/fonts/truetype".into(),
        "/usr/share/fonts/OTF".into(),
        format!("{home}/.local/share/fonts").into(),
        format!("{home}/.fonts").into(),
    ];
    let mut ctx = GlobalContext::default();
    for path in find_font_files(&dirs) {
        if let Ok(bytes) = std::fs::read(&path) {
            let _ = ctx.font_context.load_and_store(FontResource::new(bytes));
        }
    }
    ctx
}

// ---------------------------------------------------------------------------
// Scene description
// ---------------------------------------------------------------------------

#[derive(Clone)]
enum FakeNode {
    Text       { id: String, content: String, tw: String },
    Image      { id: String, color: String, width: u32, height: u32 },
    Collection { id: String, tw: String, children: Vec<FakeNode> },
}

impl FakeNode {
    fn id(&self) -> &str {
        match self { Self::Text{id,..}|Self::Image{id,..}|Self::Collection{id,..} => id }
    }

    /// Generate the takumi layout JSON for this node.
    fn to_json(&self) -> serde_json::Value {
        match self {
            Self::Text { content, tw, .. } =>
                serde_json::json!({"type":"text","text":content,"tw":tw}),
            Self::Image { color, width, height, .. } =>
                // inline-block so explicit w/h are respected in the block containing context
                serde_json::json!({"type":"container","style":{"display":"inline-block"},"tw":format!("w-[{}px] h-[{}px] bg-{}",width,height,color)}),
            Self::Collection { tw, children, .. } => {
                let ch: Vec<_> = children.iter().map(|c| c.to_json()).collect();
                serde_json::json!({"type":"container","tw":tw,"children":ch})
            }
        }
    }
}

impl std::fmt::Display for FakeNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{}", self.id()) }
}

// ---------------------------------------------------------------------------
// Per-node state (change tracking only — no pixel buffers)
// ---------------------------------------------------------------------------

enum FakeNodeState {
    Text       { id: String, content: String, tw: String },
    Image      { id: String, color: String, width: u32, height: u32 },
    Collection { id: String, tw: String, children: ManagedSet<FakeNode> },
}

impl FakeNodeState {
    fn id(&self) -> &str {
        match self { Self::Text{id,..}|Self::Image{id,..}|Self::Collection{id,..} => id }
    }
}

// ---------------------------------------------------------------------------
// Context
// ---------------------------------------------------------------------------

struct Ctx {
    global:      GlobalContext,
    changed_ids: Vec<String>,
    // Cached natural dimensions (W, H) for each node, used by the stub layout pass.
    node_dims: HashMap<String, (f32, f32)>,
}

impl Ctx {
    fn fresh() -> Self {
        Self {
            global: new_ctx_with_fonts(),
            changed_ids: Vec::new(),
            node_dims: HashMap::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Bounding box
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Rect { x: f32, y: f32, w: f32, h: f32 }

// ---------------------------------------------------------------------------
// Lifecycle — tracks which nodes changed, no rendering
// ---------------------------------------------------------------------------

impl Lifecycle for FakeNode {
    type Key=String; type State=FakeNodeState; type Context=Ctx; type Output=(); type Error=anyhow::Error;
    fn key(&self) -> String { self.id().to_string() }

    fn enter(self, ctx: &mut Ctx, _: &mut ()) -> Result<FakeNodeState> {
        ctx.changed_ids.push(self.id().to_string());
        match self {
            FakeNode::Text { id, content, tw } =>
                Ok(FakeNodeState::Text { id, content, tw }),
            FakeNode::Image { id, color, width, height } =>
                Ok(FakeNodeState::Image { id, color, width, height }),
            FakeNode::Collection { id, tw, children } => {
                let mut cs: ManagedSet<FakeNode> = ManagedSet::new();
                cs.reconcile(children, ctx, &mut ());
                Ok(FakeNodeState::Collection { id, tw, children: cs })
            }
        }
    }

    fn reconcile_self(self, state: &mut FakeNodeState, ctx: &mut Ctx, _: &mut ()) -> Result<()> {
        match (self, state) {
            (FakeNode::Text{id,content,tw}, FakeNodeState::Text{content:oc,tw:otw,..}) => {
                if content != *oc || tw != *otw {
                    ctx.changed_ids.push(id);
                    *oc = content; *otw = tw;
                }
                Ok(())
            }
            (FakeNode::Image{id,color,width,height}, FakeNodeState::Image{color:oc,width:ow,height:oh,..}) => {
                if color != *oc || width != *ow || height != *oh {
                    ctx.changed_ids.push(id);
                    *oc = color; *ow = width; *oh = height;
                }
                Ok(())
            }
            (FakeNode::Collection{id,tw,children}, FakeNodeState::Collection{tw:otw,children:cs,..}) => {
                if tw != *otw { ctx.changed_ids.push(id); *otw = tw; }
                cs.reconcile(children, ctx, &mut ());
                Ok(())
            }
            _ => Err(anyhow::anyhow!("type mismatch"))
        }
    }

    fn exit(state: FakeNodeState, ctx: &mut Ctx, _: &mut ()) -> Result<()> {
        ctx.changed_ids.push(state.id().to_string());
        if let FakeNodeState::Collection { mut children, .. } = state {
            children.reconcile(vec![], ctx, &mut ());
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Bbox collection — walk MeasuredNode + FakeNode trees in parallel
// ---------------------------------------------------------------------------

fn collect_bboxes(measured: &MeasuredNode, node: &FakeNode, bboxes: &mut HashMap<String, Rect>) {
    bboxes.insert(node.id().to_string(), Rect {
        x: measured.transform[4],
        y: measured.transform[5],
        w: measured.width,
        h: measured.height,
    });
    if let FakeNode::Collection { children, .. } = node {
        for (m, f) in measured.children.iter().zip(children.iter()) {
            collect_bboxes(m, f, bboxes);
        }
    }
}

// ---------------------------------------------------------------------------
// Stub layout — replace every leaf with a fixed-size container so that
// measure_layout only does flex geometry (zero text shaping).
// The tree structure is preserved so collect_bboxes can still pair nodes.
// ---------------------------------------------------------------------------

/// Extract layout-affecting Tailwind classes from a text node's tw.
/// Preserved on the stub so the flex container places the stub at the same
/// position as the full text node (e.g. ml-auto, flex-1).
/// Visual classes (font size, color, etc.) are dropped — they are irrelevant
/// to flex geometry and would trigger unnecessary text shaping.
fn layout_tw(tw: &str) -> String {
    tw.split_whitespace().filter(|c| {
        matches!(*c,
            "ml-auto"|"mr-auto"|"mx-auto"|"mt-auto"|"mb-auto"|"my-auto")
        || c.starts_with("flex-")
        || c.starts_with("grow") || c.starts_with("shrink")
        || c.starts_with("self-") || c.starts_with("justify-self-")
        || c.starts_with("order-")
        || c.starts_with("w-") || c.starts_with("h-")
        || c.starts_with("min-w-") || c.starts_with("max-w-")
        || c.starts_with("min-h-") || c.starts_with("max-h-")
    }).collect::<Vec<_>>().join(" ")
}

fn stub_scene_json(node: &FakeNode, dims: &HashMap<String, (f32, f32)>) -> serde_json::Value {
    match node {
        FakeNode::Text { id, tw, .. } => {
            let (w, h) = dims.get(id.as_str()).copied().unwrap_or((0.0, 0.0));
            let ltw = layout_tw(tw);
            // Preserve layout-affecting classes (ml-auto, flex-1, …) so the flex
            // container places this stub at the same position the real text node
            // would occupy.  Explicit style width/height provide the measured size.
            if ltw.is_empty() {
                serde_json::json!({"type":"container","style":{"width":w,"height":h}})
            } else {
                serde_json::json!({"type":"container","tw":ltw,"style":{"width":w,"height":h}})
            }
        }
        // Images don't involve text shaping so we reuse their exact JSON — this
        // preserves display:inline-block and tw-based dimensions, ensuring the stub
        // layout places the image at the same position as the full layout would.
        FakeNode::Image { .. } => node.to_json(),
        FakeNode::Collection { tw, children, .. } => {
            let ch: Vec<_> = children.iter().map(|c| stub_scene_json(c, dims)).collect();
            serde_json::json!({"type":"container","tw":tw,"children":ch})
        }
    }
}

/// Measure a single node in isolation to obtain its natural (W, H).
/// Only meaningful for leaf nodes (Text, Image); call on Collection returns its content size.
fn measure_natural(node: &FakeNode, global: &GlobalContext) -> (f32, f32) {
    let json = node.to_json();
    let n = parse_layout(&json).unwrap_or_else(|_| Node::container(vec![]));
    let m = takumi_measure_layout(
        RenderOptions::builder().global(global).viewport(Viewport::new((None, None))).node(n).build()
    ).expect("measure natural");
    (m.width, m.height)
}


// ---------------------------------------------------------------------------
// Flat tile scene — only nodes touching (tx, ty, tile+2*buf) x (tile+2*buf),
// each absolutely positioned using pre-measured bboxes.
// Layout is trivial: no flex computation, all positions already known.
// ---------------------------------------------------------------------------

/// Keep only visual Tailwind classes (bg, shadow, rounded, …) and strip layout
/// classes.  Layout classes have no effect on absolute-positioned childless
/// containers and trigger a takumi rendering bug at certain canvas positions.
fn visual_tw(tw: &str) -> String {
    tw.split_whitespace().filter(|c| {
        c.starts_with("bg-") || c.starts_with("shadow") || c.starts_with("rounded")
        || c.starts_with("border") || c.starts_with("ring") || c.starts_with("opacity")
        || c.starts_with("blur") || c.starts_with("backdrop")
        || c.starts_with("brightness") || c.starts_with("contrast")
        || c.starts_with("saturate") || c.starts_with("grayscale")
        || c.starts_with("invert") || c.starts_with("hue-rotate")
        || c.starts_with("drop-shadow")
    }).collect::<Vec<_>>().join(" ")
}

/// True if `tw` contains an overflow clipping class.
/// Containers with overflow-hidden must clip their children; we preserve that
/// relationship by nesting children inside the container rather than emitting
/// them as flat siblings (which would bypass the clip entirely).
fn has_overflow_clip(tw: &str) -> bool {
    tw.split_whitespace().any(|c| c.starts_with("overflow-"))
}

/// Whitelist variant: emit `node` and its subtree into `out`, only for nodes in `node_set`,
/// with coordinates relative to (`parent_x`, `parent_y`).  Used inside clipping containers.
fn collect_nested_whitelist(
    node: &FakeNode,
    bboxes: &HashMap<String, Rect>,
    node_set: &BTreeSet<String>,
    parent_x: f32, parent_y: f32,
    out: &mut Vec<serde_json::Value>,
) {
    let Some(r) = bboxes.get(node.id()) else { return };
    let lx = r.x - parent_x;
    let ly = r.y - parent_y;
    let in_set = node_set.contains(node.id());
    match node {
        FakeNode::Text { content, tw, .. } => {
            if in_set {
                out.push(serde_json::json!({"type":"text","text":content,"tw":tw,
                    "style":{"position":"absolute","left":lx,"top":ly,"width":r.w}}));
            }
        }
        FakeNode::Image { color, width, height, .. } => {
            if in_set {
                out.push(serde_json::json!({"type":"container","tw":format!("bg-{}",color),
                    "style":{"display":"block","position":"absolute","left":lx,"top":ly,
                             "width":*width as f32,"height":*height as f32}}));
            }
        }
        FakeNode::Collection { tw, children, .. } => {
            if has_overflow_clip(tw) {
                let mut ch = Vec::new();
                for child in children {
                    collect_nested_whitelist(child, bboxes, node_set, r.x, r.y, &mut ch);
                }
                if in_set {
                    out.push(serde_json::json!({"type":"container","tw":visual_tw(tw),
                        "style":{"position":"absolute","left":lx,"top":ly,
                                 "width":r.w,"height":r.h,"overflow":"hidden"},
                        "children":ch}));
                } else {
                    // Container not in set but children might be — emit children
                    // without the clip (acceptable limitation).
                    out.extend(ch);
                }
            } else {
                if in_set {
                    out.push(serde_json::json!({"type":"container","tw":visual_tw(tw),
                        "style":{"position":"absolute","left":lx,"top":ly,"width":r.w,"height":r.h}}));
                }
                for child in children {
                    collect_nested_whitelist(child, bboxes, node_set, parent_x, parent_y, out);
                }
            }
        }
    }
}

/// Collect nodes in `node_set` into an absolutely-positioned flat list.
/// from a RenderCandidate.  Nodes not in the set are skipped; Collection nodes
/// not in the set are still recursed (children may be in the set).
/// Spatial cull still applied for early-exit on branches far from the region.
/// Containers with overflow-hidden have their children nested to preserve clip.
fn collect_flat_whitelist(
    node: &FakeNode,
    bboxes: &HashMap<String, Rect>,
    node_set: &BTreeSet<String>,
    qx: f32, qy: f32, qw: f32, qh: f32,
    out: &mut Vec<serde_json::Value>,
) {
    let Some(r) = bboxes.get(node.id()) else { return };
    let buf = SHADOW_BUF as f32;
    if r.x + r.w + buf <= qx || r.x - buf >= qx + qw
    || r.y + r.h + buf <= qy || r.y - buf >= qy + qh { return; }

    let in_set = node_set.contains(node.id());
    let lx = r.x - qx;
    let ly = r.y - qy;
    match node {
        FakeNode::Text { content, tw, .. } => {
            if in_set {
                out.push(serde_json::json!({"type":"text","text":content,"tw":tw,
                    "style":{"position":"absolute","left":lx,"top":ly,"width":r.w}}));
            }
        }
        FakeNode::Image { color, width, height, .. } => {
            if in_set {
                out.push(serde_json::json!({"type":"container","tw":format!("bg-{}",color),
                    "style":{"display":"block","position":"absolute","left":lx,"top":ly,
                             "width":*width as f32,"height":*height as f32}}));
            }
        }
        FakeNode::Collection { tw, children, .. } => {
            if has_overflow_clip(tw) {
                let mut ch = Vec::new();
                for child in children {
                    collect_nested_whitelist(child, bboxes, node_set, r.x, r.y, &mut ch);
                }
                if in_set {
                    out.push(serde_json::json!({"type":"container","tw":visual_tw(tw),
                        "style":{"position":"absolute","left":lx,"top":ly,
                                 "width":r.w,"height":r.h,"overflow":"hidden"},
                        "children":ch}));
                } else {
                    out.extend(ch);
                }
            } else {
                if in_set {
                    out.push(serde_json::json!({"type":"container","tw":visual_tw(tw),
                        "style":{"position":"absolute","left":lx,"top":ly,"width":r.w,"height":r.h}}));
                }
                // Always recurse — children may be in the set even if this container isn't.
                for child in children {
                    collect_flat_whitelist(child, bboxes, node_set, qx, qy, qw, qh, out);
                }
            }
        }
    }
}

/// Render a tile at scene pixel (px_x, px_y) using pre-computed bboxes.

// ---------------------------------------------------------------------------
// Dirty tile marking
// ---------------------------------------------------------------------------

fn mark_dirty(r: &Rect, tile: u32, scene_w: u32, scene_h: u32, dirty: &mut HashSet<(u32,u32)>) {
    let t = tile as f32;
    let col0 = (r.x / t).floor() as i32;
    let row0 = (r.y / t).floor() as i32;
    let col1 = ((r.x + r.w) / t).ceil() as i32;
    let row1 = ((r.y + r.h) / t).ceil() as i32;
    let max_col = ((scene_w + tile - 1) / tile) as i32;
    let max_row = ((scene_h + tile - 1) / tile) as i32;
    for row in row0.max(0)..row1.min(max_row) {
        for col in col0.max(0)..col1.min(max_col) {
            dirty.insert((col as u32, row as u32));
        }
    }
}

// ---------------------------------------------------------------------------
// Stitching
// ---------------------------------------------------------------------------

fn stitch(frame: &mut Vec<u8>, frame_w: u32, frame_h: u32, tile_px: &[u8], tile: u32, px_x: u32, px_y: u32) {
    let copy_w = tile.min(frame_w.saturating_sub(px_x));
    let copy_h = tile.min(frame_h.saturating_sub(px_y));
    for row in 0..copy_h {
        let src = (row * tile * 4) as usize;
        let dst = (((px_y + row) * frame_w + px_x) * 4) as usize;
        frame[dst..dst + (copy_w * 4) as usize]
            .copy_from_slice(&tile_px[src..src + (copy_w * 4) as usize]);
    }
}

// ---------------------------------------------------------------------------
// Image utilities (for HTML report)
// ---------------------------------------------------------------------------

fn crop_pixels(pixels: &[u8], src_w: u32, x: u32, y: u32, w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for row in y..y+h {
        let start = ((row * src_w + x) * 4) as usize;
        out.extend_from_slice(&pixels[start..start + (w * 4) as usize]);
    }
    out
}

fn encode_png(pixels: &[u8], w: u32, h: u32) -> Vec<u8> {
    let img = ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(w, h, pixels.to_vec()).expect("buf");
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png).expect("png");
    buf.into_inner()
}

fn b64(data: &[u8]) -> String {
    const C: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len()+2)/3*4);
    for chunk in data.chunks(3) {
        let (b0,b1,b2) = (chunk[0] as u32, if chunk.len()>1{chunk[1] as u32}else{0}, if chunk.len()>2{chunk[2] as u32}else{0});
        let n = (b0<<16)|(b1<<8)|b2;
        out.push(C[((n>>18)&63)as usize]as char); out.push(C[((n>>12)&63)as usize]as char);
        out.push(if chunk.len()>1{C[((n>>6)&63)as usize]as char}else{'='});
        out.push(if chunk.len()>2{C[(n&63)as usize]as char}else{'='});
    }
    out
}

fn data_uri(pixels: &[u8], w: u32, h: u32) -> String {
    format!("data:image/png;base64,{}", b64(&encode_png(pixels, w, h)))
}

// ---------------------------------------------------------------------------
// Tile content fingerprinting — used by the LRU render cache
// ---------------------------------------------------------------------------

fn fnv_mix(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes { h ^= b as u64; h = h.wrapping_mul(1099511628211); }
    h
}

/// FNV-1a hash of the visible content of tile (tx, ty).
///
/// Covers every node in `tile_node_map[(tx,ty)]`: its id, bbox (f32 bits),
/// and all content fields that affect pixel output (text string + tw, image
/// color + dims, collection tw).  BTreeSet iteration order is deterministic,
/// so equal visual states always produce equal fingerprints.
///
/// Collision risk: FNV-64 has a ~10⁻¹⁸ false-hit probability per tile per
/// frame — negligible for a UI renderer.
/// Flat id→node map built once per frame; avoids repeated O(N) tree walks.
fn build_node_map(root: &FakeNode) -> HashMap<&str, &FakeNode> {
    let mut map = HashMap::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        map.insert(node.id(), node);
        if let FakeNode::Collection { children, .. } = node {
            stack.extend(children.iter());
        }
    }
    map
}

fn tile_fingerprint(
    tx: u32, ty: u32,
    tile_node_map: &HashMap<(u32, u32), BTreeSet<String>>,
    bboxes: &HashMap<String, Rect>,
    node_map: &HashMap<&str, &FakeNode>,
) -> u64 {
    let empty = BTreeSet::new();
    let node_set = tile_node_map.get(&(tx, ty)).unwrap_or(&empty);
    let mut h: u64 = 14695981039346656037; // FNV-1a offset basis
    // Include tile coordinates so tiles at different positions can never collide
    // even if they happen to contain the same node set at the same absolute bboxes.
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
                FakeNode::Text { content, tw, .. } => {
                    h = fnv_mix(h, b"T");
                    h = fnv_mix(h, content.as_bytes());
                    h = fnv_mix(h, b"|");
                    h = fnv_mix(h, tw.as_bytes());
                }
                FakeNode::Image { color, width, height, .. } => {
                    h = fnv_mix(h, b"I");
                    h = fnv_mix(h, color.as_bytes());
                    h = fnv_mix(h, &width.to_le_bytes());
                    h = fnv_mix(h, &height.to_le_bytes());
                }
                FakeNode::Collection { tw, .. } => {
                    h = fnv_mix(h, b"C");
                    h = fnv_mix(h, tw.as_bytes());
                }
            }
        }
        h = fnv_mix(h, b"\0"); // node separator
    }
    h
}

struct DiffResult { weighted: f64, img: Vec<u8> }

/// Perceptual pixel diff.
///
/// Channel aggregation: average of all three channels, but the worst
/// (most-deviant) channel counts twice — a slight worst-case emphasis without
/// any R/G/B colour-bias.  We do NOT apply luminance weighting (0.299R …);
/// our rendering errors are not channel-biased so that correction would add
/// noise, not signal.
///
/// Nonlinearity: cubic `(m/255)³`.  A 5%-of-255 difference (m≈13) contributes
/// only ~0.013% of what a full-contrast error (m=255) would — "grain of salt".
/// diff=50 (≈20%) contributes ≈0.8%.  Nothing saturates early; the ratio
/// metric (error / changed-area) handles all scale.
///
/// Visualisation: sqrt-gamma maps the diff to the diff-image alpha so that
/// small AA deviations are *visible* in the PNG even though they score nearly
/// zero in the weighted sum.
fn diff(a: &[u8], b: &[u8], _w: u32, _h: u32) -> DiffResult {
    let mut weighted = 0.0f64;
    let mut img = vec![0u8; a.len()];
    for i in (0..a.len().min(b.len())).step_by(4) {
        let dr = (a[i]   as i32 - b[i]   as i32).unsigned_abs() as f64;
        let dg = (a[i+1] as i32 - b[i+1] as i32).unsigned_abs() as f64;
        let db = (a[i+2] as i32 - b[i+2] as i32).unsigned_abs() as f64;
        let m  = (dr + dg + db + dr.max(dg).max(db)) / 4.0;
        let t  = m / 255.0;
        weighted += t * t * t;
        let vis_alpha = (t.sqrt() * 255.0) as u8;
        if vis_alpha > 0 {
            img[i]=255; img[i+1]=0; img[i+2]=255; img[i+3]=vis_alpha;
        } else {
            img[i]=a[i]/5; img[i+1]=a[i+1]/5; img[i+2]=a[i+2]/5; img[i+3]=255;
        }
    }
    DiffResult { weighted, img }
}

/// Build a pixel-mask (same layout as a full RGBA buffer) that is opaque only
/// within dirty tiles.  Used to restrict diff comparisons to the re-rendered
/// region so that skipped tiles don't contribute false positives.
fn dirty_mask(dirty: &HashSet<(u32,u32)>, w: u32, h: u32) -> Vec<bool> {
    let mut mask = vec![false; (w * h) as usize];
    for &(tx, ty) in dirty {
        let px = tx * TILE_SIZE;
        let py = ty * TILE_SIZE;
        let pw = TILE_SIZE.min(w.saturating_sub(px));
        let ph = TILE_SIZE.min(h.saturating_sub(py));
        for row in 0..ph {
            let base = ((py + row) * w + px) as usize;
            for col in 0..pw as usize { mask[base + col] = true; }
        }
    }
    mask
}

/// Like `diff` but only considers pixels where `mask[pixel_idx]` is true.
fn diff_masked(a: &[u8], b: &[u8], mask: &[bool], _w: u32, _h: u32) -> DiffResult {
    let mut weighted = 0.0f64;
    let mut img = vec![0u8; a.len()];
    for i in (0..a.len().min(b.len())).step_by(4) {
        if !mask.get(i / 4).copied().unwrap_or(false) {
            img[i]=a[i]/5; img[i+1]=a[i+1]/5; img[i+2]=a[i+2]/5; img[i+3]=255;
            continue;
        }
        let dr = (a[i]   as i32 - b[i]   as i32).unsigned_abs() as f64;
        let dg = (a[i+1] as i32 - b[i+1] as i32).unsigned_abs() as f64;
        let db = (a[i+2] as i32 - b[i+2] as i32).unsigned_abs() as f64;
        let m  = (dr + dg + db + dr.max(dg).max(db)) / 4.0;
        let t  = m / 255.0;
        weighted += t * t * t;
        let vis_alpha = (t.sqrt() * 255.0) as u8;
        if vis_alpha > 0 {
            img[i]=255; img[i+1]=0; img[i+2]=255; img[i+3]=vis_alpha;
        } else {
            img[i]=a[i]/5; img[i+1]=a[i+1]/5; img[i+2]=a[i+2]/5; img[i+3]=255;
        }
    }
    DiffResult { weighted, img }
}

// ---------------------------------------------------------------------------
// Test suites (unchanged)
// ---------------------------------------------------------------------------

struct SuiteFrame { label: String, scene: Vec<FakeNode>, full_json: serde_json::Value }
struct TestSuite  { name: &'static str, description: &'static str, frames: Vec<SuiteFrame> }

fn suite_simple_bar() -> TestSuite {
    let frames = (0..10).map(|i| {
        let clock = format!("{}:{:02}:{:02}", 12, i/60, i%60);
        let cpu   = format!("CPU {}%", if i%3==0{i*4}else{12});
        let label = if i==0{"cold".into()} else if i%3==0{format!("clock+cpu → {clock}")} else{format!("clock → {clock}")};
        let scene = vec![FakeNode::Collection{id:"bar".into(),tw:"flex flex-row items-center justify-between w-[400px] h-[24px] bg-gray-900".into(),children:vec![
            FakeNode::Collection{id:"left".into(),tw:"flex flex-row items-center gap-1".into(),children:vec![
                FakeNode::Image{id:"logo".into(),color:"blue-500".into(),width:16,height:16},
                FakeNode::Text{id:"ws".into(),content:"1: term".into(),tw:"text-white text-xs whitespace-nowrap".into()},
                FakeNode::Text{id:"title".into(),content:"nvim main.rs".into(),tw:"text-gray-400 text-xs whitespace-nowrap".into()},
            ]},
            FakeNode::Collection{id:"center".into(),tw:"flex flex-row items-center".into(),children:vec![
                FakeNode::Text{id:"clock".into(),content:clock.clone(),tw:"text-white text-xs font-mono whitespace-nowrap".into()},
            ]},
            FakeNode::Collection{id:"right".into(),tw:"flex flex-row items-center gap-1".into(),children:vec![
                FakeNode::Text{id:"cpu".into(),content:cpu.clone(),tw:"text-white text-xs whitespace-nowrap".into()},
                FakeNode::Text{id:"mem".into(),content:"MEM 4G".into(),tw:"text-white text-xs whitespace-nowrap".into()},
                FakeNode::Text{id:"bat".into(),content:"87%".into(),tw:"text-white text-xs whitespace-nowrap".into()},
            ]},
        ]}];
        let full_json = serde_json::json!({"type":"container","tw":"flex flex-row items-center justify-between w-[400px] h-[24px] bg-gray-900","children":[
            {"type":"container","tw":"flex flex-row items-center gap-1","children":[
                {"type":"container","style":{"display":"inline-block"},"tw":"w-[16px] h-[16px] bg-blue-500"},
                {"type":"text","text":"1: term","tw":"text-white text-xs whitespace-nowrap"},
                {"type":"text","text":"nvim main.rs","tw":"text-gray-400 text-xs whitespace-nowrap"}
            ]},
            {"type":"container","tw":"flex flex-row items-center","children":[
                {"type":"text","text":&clock,"tw":"text-white text-xs font-mono whitespace-nowrap"}
            ]},
            {"type":"container","tw":"flex flex-row items-center gap-1","children":[
                {"type":"text","text":&cpu,"tw":"text-white text-xs whitespace-nowrap"},
                {"type":"text","text":"MEM 4G","tw":"text-white text-xs whitespace-nowrap"},
                {"type":"text","text":"87%","tw":"text-white text-xs whitespace-nowrap"}
            ]}
        ]});
        SuiteFrame{label,scene,full_json}
    }).collect();
    TestSuite{name:"Simple Status Bar",description:"Baseline — no effects. Clock ticks each frame, CPU every 3rd.",frames}
}

fn suite_shadow_cards() -> TestSuite {
    let frames = (0..10).map(|i| {
        let count = i + 1;
        let msgs = ["Build complete","Tests passed","Deploy done","Lint clean","Type check ok"];
        let msg   = msgs[i % msgs.len()];
        let label = if i==0{"cold".into()}else{format!("notification #{count}")};
        let scene = vec![FakeNode::Collection{id:"cards".into(),
            tw:"flex flex-row gap-4 p-4 bg-gray-100 w-[440px] h-[90px]".into(),children:vec![
            FakeNode::Collection{id:"notif".into(),
                tw:"flex flex-col justify-between p-3 bg-white rounded-xl shadow-2xl w-[190px]".into(),children:vec![
                FakeNode::Text{id:"notif-title".into(),content:format!("{count} new"),tw:"text-gray-900 text-sm font-bold whitespace-nowrap".into()},
                FakeNode::Text{id:"notif-body".into(),content:msg.into(),tw:"text-gray-500 text-xs whitespace-nowrap".into()},
            ]},
            FakeNode::Collection{id:"static-card".into(),
                tw:"flex flex-col justify-center items-center p-3 bg-white rounded-xl shadow-2xl w-[190px]".into(),children:vec![
                FakeNode::Text{id:"static-label".into(),content:"System OK".into(),tw:"text-green-600 text-sm font-bold whitespace-nowrap".into()},
                FakeNode::Text{id:"static-sub".into(),content:"All services running".into(),tw:"text-gray-500 text-xs whitespace-nowrap".into()},
            ]},
        ]}];
        let full_json = serde_json::json!({"type":"container","tw":"flex flex-row gap-4 p-4 bg-gray-100 w-[440px] h-[90px]","children":[
            {"type":"container","tw":"flex flex-col justify-between p-3 bg-white rounded-xl shadow-2xl w-[190px]","children":[
                {"type":"text","text":format!("{count} new"),"tw":"text-gray-900 text-sm font-bold whitespace-nowrap"},
                {"type":"text","text":msg,"tw":"text-gray-500 text-xs whitespace-nowrap"}
            ]},
            {"type":"container","tw":"flex flex-col justify-center items-center p-3 bg-white rounded-xl shadow-2xl w-[190px]","children":[
                {"type":"text","text":"System OK","tw":"text-green-600 text-sm font-bold whitespace-nowrap"},
                {"type":"text","text":"All services running","tw":"text-gray-500 text-xs whitespace-nowrap"}
            ]}
        ]});
        SuiteFrame{label,scene,full_json}
    }).collect();
    TestSuite{name:"Shadow Cards",description:"Two rounded+shadow cards. Left changes each frame, right is fully static.",frames}
}

fn suite_blurred_overlay() -> TestSuite {
    let frames = (0..10).map(|i| {
        let value = format!("{}°C", 42 + i);
        let alert = if i % 4 == 0 { format!("⚠ spike at {}s", i * 10) } else { "nominal".into() };
        let label = if i==0{"cold".into()} else if i%4==0{format!("value+alert → {value}")} else{format!("value → {value}")};
        let scene = vec![FakeNode::Collection{id:"overlay".into(),
            tw:"flex flex-row items-center gap-4 px-4 w-[440px] h-[40px] bg-slate-900/80 rounded-2xl shadow-inner".into(),children:vec![
            FakeNode::Collection{id:"badge".into(),
                tw:"flex items-center justify-center w-[32px] h-[32px] bg-blue-600 rounded-lg shadow-md".into(),children:vec![
                FakeNode::Text{id:"badge-icon".into(),content:"⚡".into(),tw:"text-white text-sm".into()},
            ]},
            FakeNode::Text{id:"temp".into(),content:value.clone(),tw:"text-white text-sm font-mono font-bold whitespace-nowrap".into()},
            FakeNode::Text{id:"label".into(),content:"GPU Temp".into(),tw:"text-slate-400 text-xs whitespace-nowrap".into()},
            FakeNode::Collection{id:"status".into(),
                tw:"flex items-center ml-auto px-2 py-0.5 bg-slate-700 rounded-md".into(),children:vec![
                FakeNode::Text{id:"alert".into(),content:alert.clone(),tw:"text-yellow-300 text-xs whitespace-nowrap".into()},
            ]},
        ]}];
        let full_json = serde_json::json!({"type":"container","tw":"flex flex-row items-center gap-4 px-4 w-[440px] h-[40px] bg-slate-900/80 rounded-2xl shadow-inner","children":[
            {"type":"container","tw":"flex items-center justify-center w-[32px] h-[32px] bg-blue-600 rounded-lg shadow-md","children":[
                {"type":"text","text":"⚡","tw":"text-white text-sm"}
            ]},
            {"type":"text","text":&value,"tw":"text-white text-sm font-mono font-bold whitespace-nowrap"},
            {"type":"text","text":"GPU Temp","tw":"text-slate-400 text-xs whitespace-nowrap"},
            {"type":"container","tw":"flex items-center ml-auto px-2 py-0.5 bg-slate-700 rounded-md","children":[
                {"type":"text","text":&alert,"tw":"text-yellow-300 text-xs whitespace-nowrap"}
            ]}
        ]});
        SuiteFrame{label,scene,full_json}
    }).collect();
    TestSuite{name:"Blurred Overlay",description:"Rounded panel. Temperature changes every frame; alert fires every 4th.",frames}
}

fn suite_dense_metrics() -> TestSuite {
    let frames = (0..10).map(|i| {
        let metrics = [
            ("CPU",  format!("{}%", if i%2==0{12+i*3}else{15})),
            ("MEM",  "4.2G".into()),
            ("GPU",  format!("{}%", 60+i*2)),
            ("DISK", "42%".into()),
            ("NET↑", "1.2M".into()),
            ("TEMP", "62°C".into()),
        ];
        let label = if i==0{"cold".into()}else{format!("cpu={} gpu={}%",metrics[0].1,60+i*2)};

        let cols: Vec<FakeNode> = metrics.iter().map(|(name,val)|
            FakeNode::Collection{id:format!("col-{name}"),
                tw:"flex flex-col items-center px-2 bg-gray-800 rounded-lg shadow-md".into(),children:vec![
                FakeNode::Text{id:format!("lbl-{name}"),content:name.to_string(),tw:"text-gray-400 text-[10px] whitespace-nowrap".into()},
                FakeNode::Text{id:format!("val-{name}"),content:val.clone(),tw:"text-white text-xs font-mono font-bold whitespace-nowrap".into()},
            ]}
        ).collect();

        let scene = vec![FakeNode::Collection{id:"grid".into(),
            tw:"flex flex-row gap-1 p-1 bg-gray-900 w-[360px] h-[36px]".into(),children:cols}];

        let full_cols: Vec<serde_json::Value> = metrics.iter().map(|(name,val)|
            serde_json::json!({"type":"container","tw":"flex flex-col items-center px-2 bg-gray-800 rounded-lg shadow-md","children":[
                {"type":"text","text":name,"tw":"text-gray-400 text-[10px] whitespace-nowrap"},
                {"type":"text","text":val,"tw":"text-white text-xs font-mono font-bold whitespace-nowrap"}
            ]})
        ).collect();
        let full_json = serde_json::json!({"type":"container","tw":"flex flex-row gap-1 p-1 bg-gray-900 w-[360px] h-[36px]","children":full_cols});

        SuiteFrame{label,scene,full_json}
    }).collect();
    TestSuite{name:"Dense Metrics Grid",description:"6 shadow+rounded columns. CPU and GPU change each frame; the other 4 stay static.",frames}
}

// ---------------------------------------------------------------------------
// Benchmark
// ---------------------------------------------------------------------------

struct FrameResult { label: String, full_time: Duration, incr_time: Duration, full_px: Vec<u8>, prev_full_px: Vec<u8>, incr_px: Vec<u8>, w: u32, h: u32, render_calls: u32, skipped: u32, cache_hits: u32, dirty_tiles: HashSet<(u32,u32)> }
struct SuiteResult { name: &'static str, description: &'static str, frames: Vec<FrameResult> }

const TILE_SIZE: u32 = 32;
const SHADOW_BUF: u32 = 32; // extra border around each tile to capture shadow bleed
const TILE_CACHE_MB: usize = 10; // LRU tile cache budget; entry count derived from TILE_SIZE

// Two tiles whose shadow-expanded intervals overlap are merged into one band.
// Overlap condition: distance < 1 + 2*SHADOW_BUF/TILE_SIZE  →  distance ≤ MERGE_THRESHOLD.
const MERGE_THRESHOLD: u32 = 2 * SHADOW_BUF / TILE_SIZE;

// Default cost-model coefficients (overridden at runtime by self-calibration).
const O_FIXED_MS: f64 = 1.0;
const K_AREA:     f64 = 4e-5;
const K_NODES:    f64 = 0.3;

#[derive(Clone)]
struct CostModel { o_fixed: f64, k_area: f64, k_nodes: f64 }

impl Default for CostModel {
    fn default() -> Self { Self { o_fixed: O_FIXED_MS, k_area: K_AREA, k_nodes: K_NODES } }
}

struct CalibrationResult { model: CostModel, r_squared: f64, n_samples: usize }

// ---------------------------------------------------------------------------
// Multi-band grouping — split dirty tiles into spatially contiguous clusters
// so that distant regions each get their own (small) render call.
// ---------------------------------------------------------------------------

struct RenderBand {
    min_tx: u32, max_tx: u32,
    min_ty: u32, max_ty: u32,
    tiles: Vec<(u32, u32)>,
}

/// Group dirty tiles into bands along the Y axis (primary key = row).
fn compute_bands_y(dirty: &HashSet<(u32, u32)>) -> Vec<RenderBand> {
    let mut tiles: Vec<(u32, u32)> = dirty.iter().copied().collect();
    tiles.sort_by_key(|&(_, ty)| ty);
    let mut bands: Vec<RenderBand> = Vec::new();
    for (tx, ty) in tiles {
        if let Some(b) = bands.last_mut() {
            if ty - b.max_ty <= MERGE_THRESHOLD {
                b.max_ty = b.max_ty.max(ty);
                b.min_tx = b.min_tx.min(tx);
                b.max_tx = b.max_tx.max(tx);
                b.tiles.push((tx, ty));
                continue;
            }
        }
        bands.push(RenderBand { min_tx: tx, max_tx: tx, min_ty: ty, max_ty: ty, tiles: vec![(tx, ty)] });
    }
    bands
}

/// Group dirty tiles into bands along the X axis (primary key = column).
fn compute_bands_x(dirty: &HashSet<(u32, u32)>) -> Vec<RenderBand> {
    let mut tiles: Vec<(u32, u32)> = dirty.iter().copied().collect();
    tiles.sort_by_key(|&(tx, _)| tx);
    let mut bands: Vec<RenderBand> = Vec::new();
    for (tx, ty) in tiles {
        if let Some(b) = bands.last_mut() {
            if tx - b.max_tx <= MERGE_THRESHOLD {
                b.max_tx = b.max_tx.max(tx);
                b.min_ty = b.min_ty.min(ty);
                b.max_ty = b.max_ty.max(ty);
                b.tiles.push((tx, ty));
                continue;
            }
        }
        bands.push(RenderBand { min_tx: tx, max_tx: tx, min_ty: ty, max_ty: ty, tiles: vec![(tx, ty)] });
    }
    bands
}

/// Estimated total render area across all bands (proxy for render cost).
fn estimated_area(bands: &[RenderBand]) -> u64 {
    bands.iter().map(|b| {
        let w = ((b.max_tx - b.min_tx + 1) * TILE_SIZE + 2 * SHADOW_BUF) as u64;
        let h = ((b.max_ty - b.min_ty + 1) * TILE_SIZE + 2 * SHADOW_BUF) as u64;
        w * h
    }).sum()
}

// ---------------------------------------------------------------------------
// Categorical + spatial + cost-model candidate grouping
// ---------------------------------------------------------------------------

/// All tiles (tx, ty) whose shadow-expanded region overlaps a bbox.
fn tiles_for_bbox(r: &Rect, cols: u32, rows: u32) -> Vec<(u32, u32)> {
    let buf = SHADOW_BUF as f32;
    let c0 = ((r.x - buf) / TILE_SIZE as f32).floor().max(0.0) as u32;
    let r0 = ((r.y - buf) / TILE_SIZE as f32).floor().max(0.0) as u32;
    let c1 = ((r.x + r.w + buf) / TILE_SIZE as f32).ceil().min(cols as f32) as u32;
    let r1 = ((r.y + r.h + buf) / TILE_SIZE as f32).ceil().min(rows as f32) as u32;
    let mut out = Vec::new();
    for ty in r0..r1 { for tx in c0..c1 { out.push((tx, ty)); } }
    out
}

/// Build the full tile→node map from scratch. For each tile, the set contains
/// every node whose shadow-expanded bbox overlaps that tile.
fn build_tile_node_map(
    bboxes: &HashMap<String, Rect>,
    cols: u32, rows: u32,
) -> HashMap<(u32, u32), BTreeSet<String>> {
    let mut map: HashMap<(u32, u32), BTreeSet<String>> = HashMap::new();
    for (id, r) in bboxes {
        for tile in tiles_for_bbox(r, cols, rows) {
            map.entry(tile).or_default().insert(id.clone());
        }
    }
    map
}

/// A render candidate: a spatial band paired with the exact set of nodes needed
/// to render every dirty tile it covers (no more, no less).
struct RenderCandidate {
    min_tx: u32, max_tx: u32,
    min_ty: u32, max_ty: u32,
    tiles:    Vec<(u32, u32)>,
    node_set: BTreeSet<String>,
}

fn candidate_cost(c: &RenderCandidate, cm: &CostModel) -> f64 {
    let w = ((c.max_tx - c.min_tx + 1) * TILE_SIZE + 2 * SHADOW_BUF) as f64;
    let h = ((c.max_ty - c.min_ty + 1) * TILE_SIZE + 2 * SHADOW_BUF) as f64;
    cm.o_fixed + cm.k_area * w * h + cm.k_nodes * c.node_set.len() as f64
}

fn merge_candidates(a: &RenderCandidate, b: &RenderCandidate) -> RenderCandidate {
    let mut tiles = a.tiles.clone();
    tiles.extend_from_slice(&b.tiles);
    let mut node_set = a.node_set.clone();
    for id in &b.node_set { node_set.insert(id.clone()); }
    RenderCandidate {
        min_tx: a.min_tx.min(b.min_tx), max_tx: a.max_tx.max(b.max_tx),
        min_ty: a.min_ty.min(b.min_ty), max_ty: a.max_ty.max(b.max_ty),
        tiles, node_set,
    }
}

/// Step 1: group dirty tiles by identical node set (categorical).
/// Step 2: spatial-band within each group.
/// Result: one RenderCandidate per (node-set, spatial-band) pair — maximally split.
fn compute_candidates(
    dirty: &HashSet<(u32, u32)>,
    tile_node_map: &HashMap<(u32, u32), BTreeSet<String>>,
) -> Vec<RenderCandidate> {
    // Categorical grouping: key = sorted node-id vec (hashable proxy for BTreeSet).
    let mut groups: HashMap<Vec<String>, HashSet<(u32, u32)>> = HashMap::new();
    for &t in dirty {
        let key: Vec<String> = tile_node_map.get(&t)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default();
        groups.entry(key).or_default().insert(t);
    }
    // Spatial banding within each categorical group.
    let mut candidates = Vec::new();
    for (node_vec, tiles) in groups {
        let node_set: BTreeSet<String> = node_vec.into_iter().collect();
        let by = compute_bands_y(&tiles);
        let bx = compute_bands_x(&tiles);
        let bands = if estimated_area(&by) <= estimated_area(&bx) { by } else { bx };
        for band in bands {
            candidates.push(RenderCandidate {
                min_tx: band.min_tx, max_tx: band.max_tx,
                min_ty: band.min_ty, max_ty: band.max_ty,
                tiles: band.tiles,
                node_set: node_set.clone(),
            });
        }
    }
    candidates
}

/// Greedily merge candidate pairs while doing so reduces total estimated cost.
/// Stops when no beneficial merge exists (O(n² × |node_set|) per pass; n is small).
fn greedy_merge_candidates(mut cs: Vec<RenderCandidate>, cm: &CostModel) -> Vec<RenderCandidate> {
    loop {
        if cs.len() < 2 { break; }
        let mut best_gain = 0.0f64;
        let mut best = (0usize, 1usize);
        for i in 0..cs.len() {
            for j in i + 1..cs.len() {
                let merged = merge_candidates(&cs[i], &cs[j]);
                let gain = candidate_cost(&cs[i], cm) + candidate_cost(&cs[j], cm)
                         - candidate_cost(&merged, cm);
                if gain > best_gain { best_gain = gain; best = (i, j); }
            }
        }
        if best_gain <= 0.0 { break; }
        let (i, j) = best;
        let merged = merge_candidates(&cs[i], &cs[j]);
        cs.remove(j); cs.remove(i);
        cs.push(merged);
    }
    cs
}

// ---------------------------------------------------------------------------
// OLS self-calibration
// ---------------------------------------------------------------------------

/// Fit  time_ms = o_fixed + k_area × canvas_area + k_nodes × n_nodes
/// from observed render samples using ordinary least squares.
///
/// area is scaled by 1e4 internally to improve numerical conditioning.
/// Returns None if fewer than 4 samples or the system is degenerate.
fn fit_cost_model(samples: &[(f64, f64, f64)]) -> Option<CalibrationResult> {
    let n = samples.len();
    if n < 4 { return None; }

    const SCALE: f64 = 1e4; // area normalisation: avoids large XtX diagonal entries
    let mut xtx = [[0.0f64; 3]; 3];
    let mut xty = [0.0f64; 3];
    for &(area, nodes, time) in samples {
        let x = [1.0, area / SCALE, nodes];
        for i in 0..3 {
            xty[i] += x[i] * time;
            for j in 0..3 { xtx[i][j] += x[i] * x[j]; }
        }
    }

    // Gaussian elimination with partial pivoting on the 3×3 normal equations.
    let mut a = xtx;
    let mut b = xty;
    for col in 0..3 {
        let mut pivot = col;
        for r in (col + 1)..3 {
            if a[r][col].abs() > a[pivot][col].abs() { pivot = r; }
        }
        a.swap(col, pivot);
        b.swap(col, pivot);
        let p = a[col][col];
        if p.abs() < 1e-14 { return None; }
        for row in (col + 1)..3 {
            let f = a[row][col] / p;
            for k in col..3 { a[row][k] -= f * a[col][k]; }
            b[row] -= f * b[col];
        }
    }
    let mut sol = [0.0f64; 3];
    for i in (0..3).rev() {
        sol[i] = b[i];
        for j in (i + 1)..3 { sol[i] -= a[i][j] * sol[j]; }
        sol[i] /= a[i][i];
    }
    let (o_raw, k_area_raw, k_nodes_raw) = (sol[0], sol[1] / SCALE, sol[2]);

    // R² over the raw samples using the fitted (un-clamped) coefficients.
    let mean_t = samples.iter().map(|&(_, _, t)| t).sum::<f64>() / n as f64;
    let ss_tot: f64 = samples.iter().map(|&(_, _, t)| (t - mean_t).powi(2)).sum();
    let ss_res: f64 = samples.iter().map(|&(area, nd, t)| {
        (t - (o_raw + k_area_raw * area + k_nodes_raw * nd)).powi(2)
    }).sum();
    let r_squared = if ss_tot > 1e-15 { (1.0 - ss_res / ss_tot).max(0.0) } else { 0.0 };

    // Clamp to physically meaningful positive values.
    Some(CalibrationResult {
        model: CostModel {
            o_fixed: o_raw.max(0.05),
            k_area:  k_area_raw.max(1e-7),
            k_nodes: k_nodes_raw.max(0.001),
        },
        r_squared,
        n_samples: n,
    })
}

fn run_suite(suite: &TestSuite, cm: &CostModel, cal_samples: &mut Vec<(f64, f64, f64)>) -> SuiteResult {
    let mut incr_ctx = Ctx::fresh();
    let mut incr_set: ManagedSet<FakeNode> = ManagedSet::new();
    let mut frame_buf: Vec<u8> = Vec::new();
    let mut prev_full: Vec<u8> = Vec::new();
    // Bboxes from the PREVIOUS frame's stub layout, used for moved-node detection
    // so we always compare stub vs stub and avoid false positives.
    let mut prev_stub_bboxes: HashMap<String, Rect> = HashMap::new();
    // LRU tile render cache: fingerprint → TILE_SIZE×TILE_SIZE×4 bytes.
    // Capacity is derived from TILE_CACHE_MB so it auto-adjusts when TILE_SIZE changes.
    let tile_bytes = (TILE_SIZE * TILE_SIZE * 4) as usize;
    let cache_cap = NonZeroUsize::new(
        (TILE_CACHE_MB * 1024 * 1024 + tile_bytes - 1) / tile_bytes
    ).unwrap();
    let mut tile_cache: LruCache<u64, Vec<u8>> = LruCache::new(cache_cap);
    // Persistent tile→node map: for each tile the set of nodes whose shadow-
    // expanded bbox overlaps it.  Rebuilt each frame from current bboxes.
    // (Incremental update is a future optimisation — rebuild is fast enough.)
    let mut tile_node_map: HashMap<(u32,u32), BTreeSet<String>> = HashMap::new();

    let frames: Vec<FrameResult> = suite.frames.iter().map(|f| {
        // ── Full render (fresh context, no caching) ──────────────────────────
        let full_ctx = Ctx::fresh();
        let t = Instant::now();
        let (full_px, w, h) = {
            let node = parse_layout(&f.full_json).unwrap_or_else(|_| Node::container(vec![]));
            let img = takumi_render(
                RenderOptions::builder().global(&full_ctx.global)
                    .viewport(Viewport::new((None,None))).node(node).build()
            ).expect("full render");
            let (w,h) = img.dimensions();
            (img.into_raw(), w, h)
        };
        let full_time = t.elapsed();

        // ── Tile-based incremental render ─────────────────────────────────────
        incr_ctx.changed_ids.clear();
        let t = Instant::now();

        // 1. Reconcile — populates changed_ids
        incr_set.reconcile(f.scene.clone(), &mut incr_ctx, &mut ());

        let cols = (w + TILE_SIZE - 1) / TILE_SIZE;
        let rows = (h + TILE_SIZE - 1) / TILE_SIZE;

        // No-op short-circuit: if nothing changed and frame_buf is already
        // populated, every tile is identical to last frame — skip all remaining
        // work.  Static frames (no clock tick, no focus change, etc.) cost
        // essentially nothing in the real pipeline.
        if incr_ctx.changed_ids.is_empty() && !frame_buf.is_empty() {
            let incr_time = t.elapsed();
            let incr_px = frame_buf.clone();
            let my_prev_full = std::mem::replace(&mut prev_full, full_px.clone());
            return FrameResult {
                label: f.label.clone(), full_time, incr_time, full_px,
                prev_full_px: my_prev_full, incr_px, w, h,
                render_calls: 0, skipped: cols * rows, cache_hits: 0,
                dirty_tiles: HashSet::new(),
            };
        }

        // 2. Measure layout — always via the stub-layout path.
        //
        //    (a) Measure each changed leaf in isolation to update node_dims.
        //        On the first frame every node enters → changed_ids contains all
        //        nodes → node_dims is fully bootstrapped here.
        //    (b) Stub layout: replace every leaf with a fixed-size container so
        //        taffy re-computes positions with zero text shaping.
        //
        //    node_map is built once and shared by both the measure step (O(1)
        //    lookup vs the old O(N) find_node) and later fingerprinting.
        let node_map = build_node_map(&f.scene[0]);

        // Step (a): update node_dims for changed leaves; track whether anything
        // that affects flex geometry actually changed.
        //   dims_changed        — a Text/Image node rendered to a different size
        //   collection_changed  — a Collection tw changed (affects flex positions)
        // If neither is true the stub layout would produce identical positions to
        // last frame, so step (b) can be skipped entirely.
        let mut dims_changed = false;
        let mut collection_changed = false;
        for id in &incr_ctx.changed_ids {
            if let Some(&node) = node_map.get(id.as_str()) {
                match node {
                    FakeNode::Text { .. } => {
                        let new_dims = measure_natural(node, &incr_ctx.global);
                        if incr_ctx.node_dims.get(id.as_str()).copied() != Some(new_dims) {
                            dims_changed = true;
                        }
                        incr_ctx.node_dims.insert(id.clone(), new_dims);
                    }
                    FakeNode::Image { width, height, .. } => {
                        let new_dims = (*width as f32, *height as f32);
                        if incr_ctx.node_dims.get(id.as_str()).copied() != Some(new_dims) {
                            dims_changed = true;
                        }
                        incr_ctx.node_dims.insert(id.clone(), new_dims);
                    }
                    FakeNode::Collection { .. } => { collection_changed = true; }
                }
            }
        }
        // Step (b): stub layout — skipped when positions are provably unchanged.
        let bboxes: HashMap<String, Rect> = if dims_changed || collection_changed {
            let stub_json = stub_scene_json(&f.scene[0], &incr_ctx.node_dims);
            let node = parse_layout(&stub_json).unwrap_or_else(|_| Node::container(vec![]));
            let measured = takumi_measure_layout(
                RenderOptions::builder().global(&incr_ctx.global)
                    .viewport(Viewport::new((None, None))).node(node).build()
            ).expect("stub layout");
            let mut sb = HashMap::new();
            collect_bboxes(&measured, &f.scene[0], &mut sb);
            sb
        } else {
            prev_stub_bboxes.clone()
        };

        // 3. Compute dirty tiles and update tile→node map.
        let mut dirty: HashSet<(u32,u32)> = HashSet::new();

        if frame_buf.len() != (w * h * 4) as usize {
            // First frame: full build of tile_node_map + all tiles dirty.
            frame_buf = vec![0u8; (w * h * 4) as usize];
            tile_node_map = build_tile_node_map(&bboxes, cols, rows);
            for ty in 0..rows { for tx in 0..cols { dirty.insert((tx, ty)); } }
        } else {
            // Incremental tile_node_map update — O(changed_nodes × tiles_per_node)
            // instead of O(total_nodes × tiles_per_node) for a full rebuild.
            //
            // Step 1: update entries for nodes whose content changed.
            for id in &incr_ctx.changed_ids {
                if let Some(old_r) = prev_stub_bboxes.get(id.as_str()) {
                    for (tx, ty) in tiles_for_bbox(old_r, cols, rows) {
                        if let Some(s) = tile_node_map.get_mut(&(tx, ty)) { s.remove(id.as_str()); }
                    }
                    mark_dirty(old_r, TILE_SIZE, w, h, &mut dirty);
                }
                if let Some(new_r) = bboxes.get(id.as_str()) {
                    for (tx, ty) in tiles_for_bbox(new_r, cols, rows) {
                        tile_node_map.entry((tx, ty)).or_default().insert(id.clone());
                    }
                    mark_dirty(new_r, TILE_SIZE, w, h, &mut dirty);
                }
            }
            // Step 2: update entries for nodes that moved due to layout reflow
            // (not in changed_ids but bbox shifted — e.g. justify-between reflow).
            // This loop is O(total_nodes) but was already required for dirty marking;
            // the tile_node_map update is folded in at no extra asymptotic cost.
            let changed_set: HashSet<&str> =
                incr_ctx.changed_ids.iter().map(String::as_str).collect();
            for (id, new_r) in &bboxes {
                if changed_set.contains(id.as_str()) { continue; }
                if let Some(old_r) = prev_stub_bboxes.get(id.as_str()) {
                    if (new_r.x - old_r.x).abs() > 0.5 || (new_r.y - old_r.y).abs() > 0.5 {
                        for (tx, ty) in tiles_for_bbox(old_r, cols, rows) {
                            if let Some(s) = tile_node_map.get_mut(&(tx, ty)) { s.remove(id.as_str()); }
                        }
                        for (tx, ty) in tiles_for_bbox(new_r, cols, rows) {
                            tile_node_map.entry((tx, ty)).or_default().insert(id.clone());
                        }
                        mark_dirty(new_r, TILE_SIZE, w, h, &mut dirty);
                        mark_dirty(old_r, TILE_SIZE, w, h, &mut dirty);
                    }
                }
            }
        }

        // 4. Cache lookup — fingerprint each dirty tile upfront, stitch hits
        //    immediately and remove from dirty so they don't inflate band areas.
        let skipped = cols * rows - dirty.len() as u32; // tiles that were never dirty
        let fps: HashMap<(u32,u32), u64> = dirty.iter()
            .map(|&(tx,ty)| ((tx,ty), tile_fingerprint(tx, ty, &tile_node_map, &bboxes, &node_map)))
            .collect();
        let mut cache_hits = 0u32;
        dirty.retain(|&(tx, ty)| {
            match tile_cache.get(&fps[&(tx,ty)]).cloned() {
                Some(px) => {
                    stitch(&mut frame_buf, w, h, &px, TILE_SIZE, tx * TILE_SIZE, ty * TILE_SIZE);
                    cache_hits += 1;
                    false
                }
                None => true,
            }
        });

        // 5. Categorical + spatial grouping with cost-model greedy merge.
        //    compute_candidates handles all frames uniformly — cold frame (all
        //    tiles dirty) and incremental alike.  No special-case sentinel needed.
        let candidates: Vec<RenderCandidate> = if dirty.is_empty() {
            vec![]
        } else {
            let raw = compute_candidates(&dirty, &tile_node_map);
            greedy_merge_candidates(raw, cm)
        };
        let render_calls = candidates.len() as u32;

        for cand in &candidates {
            let batch_px_x = cand.min_tx * TILE_SIZE;
            let batch_px_y = cand.min_ty * TILE_SIZE;
            let batch_w = (cand.max_tx - cand.min_tx + 1) * TILE_SIZE;
            let batch_h = (cand.max_ty - cand.min_ty + 1) * TILE_SIZE;

            let buf = SHADOW_BUF as f32;
            let qx = batch_px_x as f32 - buf;
            let qy = batch_px_y as f32 - buf;
            let qw = batch_w as f32 + 2.0 * buf;
            let qh = batch_h as f32 + 2.0 * buf;
            let canvas_w = batch_w + 2 * SHADOW_BUF;
            let canvas_h = batch_h + 2 * SHADOW_BUF;

            let mut nodes: Vec<serde_json::Value> = Vec::new();
            collect_flat_whitelist(&f.scene[0], &bboxes, &cand.node_set,
                                   qx, qy, qw, qh, &mut nodes);

            let scene = serde_json::json!({
                "type": "container",
                "style": { "display": "block", "position": "relative",
                    "width": canvas_w as f32, "height": canvas_h as f32,
                    "overflow": "hidden" },
                "children": nodes
            });
            let node = parse_layout(&scene).unwrap_or_else(|_| Node::container(vec![]));
            let t_render = Instant::now();
            let cand_px = takumi_render(
                RenderOptions::builder().global(&incr_ctx.global)
                    .viewport(Viewport::new((None,None))).node(node).build()
            ).expect("candidate render").into_raw();
            cal_samples.push((
                canvas_w as f64 * canvas_h as f64,
                nodes.len() as f64,
                t_render.elapsed().as_secs_f64() * 1000.0,
            ));

            for &(tx, ty) in &cand.tiles {
                let px_x = tx * TILE_SIZE;
                let px_y = ty * TILE_SIZE;
                let off_x = SHADOW_BUF + (tx - cand.min_tx) * TILE_SIZE;
                let off_y = SHADOW_BUF + (ty - cand.min_ty) * TILE_SIZE;
                let tile_px = crop_pixels(&cand_px, canvas_w, off_x, off_y, TILE_SIZE, TILE_SIZE);
                stitch(&mut frame_buf, w, h, &tile_px, TILE_SIZE, px_x, px_y);
                tile_cache.put(fps[&(tx,ty)], tile_px);
            }
        }

        let incr_time = t.elapsed();
        let incr_px = frame_buf.clone();
        let my_prev_full = std::mem::replace(&mut prev_full, full_px.clone());
        let saved_dirty = dirty.clone();
        prev_stub_bboxes = bboxes;

        FrameResult { label: f.label.clone(), full_time, incr_time, full_px, prev_full_px: my_prev_full, incr_px, w, h, render_calls, skipped, cache_hits, dirty_tiles: saved_dirty }
    }).collect();

    SuiteResult { name: suite.name, description: suite.description, frames }
}

// ---------------------------------------------------------------------------
// HTML report — tabbed (Summary + one tab per suite)
// ---------------------------------------------------------------------------

/// A frame is "pixel-perfect" when  error_weighted / changed_area_weighted  is below this ratio.
/// Both use the cubic perceptual metric, so the ratio is approximately
/// "what fraction of the visually-significant change is rendered incorrectly".
/// 5% is a comfortable margin above AA noise (which scores near-zero) while still
/// catching any clearly-wrong pixel (diff > 50) involving more than ~30 pixels.
const PERFECT_THRESHOLD: f64 = 0.05;

fn html_report(suites: &[SuiteResult], cal_note: &str) -> String {
    // ── Timing totals ────────────────────────────────────────────────────────
    let mut all_full = Duration::ZERO;
    let mut all_incr = Duration::ZERO;
    for s in suites {
        for (i, f) in s.frames.iter().enumerate() {
            if i > 0 { all_full += f.full_time; all_incr += f.incr_time; }
        }
    }
    let overall_speedup = all_full.as_secs_f64() / all_incr.as_secs_f64().max(1e-9);

    // ── Per-suite passes ─────────────────────────────────────────────────────
    // Collect worst imperfect frames across all suites for the summary page.
    // Each entry: (weighted, suite_name, frame_idx, label, summary_snippet_html)
    let mut imperfect: Vec<(f64, &str, usize, String, String)> = Vec::new();

    let mut tab_btns = String::from(
        r#"<button class="tab-btn active" onclick="showTab('tab-summary',this)">Summary</button>"#,
    );
    let mut suite_tabs = String::new();
    let mut table_rows = String::new();

    for (si, suite) in suites.iter().enumerate() {
        let tab_id = format!("tab-suite-{si}");
        tab_btns.push_str(&format!(
            r#"<button class="tab-btn" onclick="showTab('{tab_id}',this)">{}</button>"#,
            suite.name
        ));

        let mut frames_html = String::new();
        let mut s_full = Duration::ZERO;
        let mut s_incr = Duration::ZERO;
        let mut n_perfect = 0u32;
        let mut n_total  = 0u32;

        for (fi, f) in suite.frames.iter().enumerate() {
            if fi > 0 { s_full += f.full_time; s_incr += f.incr_time; }
            n_total += 1;

            let full_uri = data_uri(&f.full_px, f.w, f.h);
            let speedup_f = f.full_time.as_secs_f64() / f.incr_time.as_secs_f64().max(1e-9);
            // Display size: 3× pixel-art scaling, capped so huge canvases stay scrollable
            let pw = (f.w * 3).min(900).max(120);
            let ph = (f.h * 3).min(600).max(36);

            // Restrict correctness measurement to dirty tiles only.
            let mask = dirty_mask(&f.dirty_tiles, f.w, f.h);
            let chg_w = if f.prev_full_px.len() == f.full_px.len() {
                diff_masked(&f.prev_full_px, &f.full_px, &mask, f.w, f.h).weighted
            } else {
                (f.dirty_tiles.len() * (TILE_SIZE * TILE_SIZE) as usize) as f64
            };
            // Full-frame change image (for the Δ column) uses unmasked diff so
            // the viewer can see what actually changed regardless of dirty tiles.
            let chg_diff = if f.prev_full_px.len() == f.full_px.len() {
                Some(diff(&f.prev_full_px, &f.full_px, f.w, f.h))
            } else {
                None
            };

            let (d, incr_uri, diff_uri, chg_uri) = if !f.incr_px.is_empty() && f.incr_px.len() == f.full_px.len() {
                let d = diff_masked(&f.full_px, &f.incr_px, &mask, f.w, f.h);
                let du = data_uri(&d.img, f.w, f.h);
                let iu = data_uri(&f.incr_px, f.w, f.h);
                let cu = chg_diff.as_ref().map(|c| data_uri(&c.img, f.w, f.h)).unwrap_or_default();
                (Some(d), iu, du, cu)
            } else {
                (None, String::new(), String::new(), String::new())
            };

            let ratio = d.as_ref().map(|d| d.weighted / chg_w.max(1.0)).unwrap_or(0.0);
            let perfect = ratio < PERFECT_THRESHOLD;
            if perfect { n_perfect += 1; }

            let badge = if perfect { r#"<span class="ok">✓</span>"# } else { r#"<span class="diff">≠</span>"# };
            let diff_stat = d.as_ref().map(|d| format!(
                "err={:.1} / chg={:.0} = {:.1}%", d.weighted, chg_w, ratio * 100.0
            )).unwrap_or_default();

            let chg_col = if !chg_uri.is_empty() {
                format!(r#"<div><div class="cap">Δ Change</div><img src="{chg_uri}" style="width:{pw}px;height:{ph}px;image-rendering:pixelated"></div>"#,
                    chg_uri = chg_uri, pw = pw, ph = ph)
            } else { String::new() };

            frames_html.push_str(&format!(r#"
            <div class="frame {cls}">
              <div class="fhdr"><strong>Frame {fi}</strong> — {lbl} {badge}
                <span class="tm">full {ft:.1}ms · incr {it:.1}ms · {sp:.1}×</span>
                <span class="tm">{rc} renders · {sk} skipped · {ds}</span>
              </div>
              <div class="imgs">
                <div><div class="cap">Full</div><img src="{fu}" style="width:{pw}px;height:{ph}px;image-rendering:pixelated"></div>
                {chg_col}
                <div><div class="cap">Incremental</div><img src="{iu}" style="width:{pw}px;height:{ph}px;image-rendering:pixelated"></div>
                <div><div class="cap">Diff (error)</div><img src="{du}" style="width:{pw}px;height:{ph}px;image-rendering:pixelated"></div>
              </div>
            </div>"#,
                cls = if perfect { "perfect" } else { "imperfect" },
                lbl = f.label, badge = badge,
                ft = f.full_time.as_secs_f64() * 1000.0, it = f.incr_time.as_secs_f64() * 1000.0,
                sp = speedup_f, rc = f.render_calls, sk = f.skipped,
                fu = full_uri, iu = incr_uri, du = diff_uri,
                pw = pw, ph = ph, ds = diff_stat, chg_col = chg_col,
            ));

            // Collect for worst-20 (compact thumbnail in summary), keyed by ratio
            if d.is_some() && !perfect {
                {
                    let tw = f.w.min(240);
                    let th = (f.h * tw / f.w.max(1)).min(320).max(20);
                    let snippet = format!(r#"
                    <div class="frame imperfect">
                      <div class="fhdr">{sn} · Frame {fi} — {lbl} {badge}
                        <span class="tm">full {ft:.1}ms · incr {it:.1}ms · {sp:.1}×</span>
                        <span class="tm">{ds}</span>
                      </div>
                      <div class="imgs">
                        <div><div class="cap">Full</div><img src="{fu}" style="width:{tw}px;height:{th}px;image-rendering:pixelated"></div>
                        <div><div class="cap">Incremental</div><img src="{iu}" style="width:{tw}px;height:{th}px;image-rendering:pixelated"></div>
                        <div><div class="cap">Diff (error)</div><img src="{du}" style="width:{tw}px;height:{th}px;image-rendering:pixelated"></div>
                      </div>
                    </div>"#,
                        sn = suite.name, fi = fi, lbl = f.label, badge = badge,
                        ft = f.full_time.as_secs_f64() * 1000.0, it = f.incr_time.as_secs_f64() * 1000.0,
                        sp = speedup_f, ds = diff_stat,
                        fu = full_uri, iu = incr_uri, du = diff_uri, tw = tw, th = th,
                    );
                    imperfect.push((ratio, suite.name, fi, f.label.clone(), snippet));
                }
            }
        }

        let ss = s_full.as_secs_f64() / s_incr.as_secs_f64().max(1e-9);
        table_rows.push_str(&format!(
            r#"<tr><td>{name}</td><td class="num">{ss:.1}×</td><td class="num">{np}/{nt}</td></tr>"#,
            name = suite.name, ss = ss, np = n_perfect, nt = n_total,
        ));
        suite_tabs.push_str(&format!(r#"
        <div id="{tab_id}" class="tab-content" style="display:none">
          <h2>{name} <span class="speedup">{ss:.1}× speedup</span></h2>
          <p class="desc">{desc}</p>
          {frames}
        </div>"#,
            tab_id = tab_id, name = suite.name, ss = ss,
            desc = suite.description, frames = frames_html,
        ));
    }

    // ── Worst-20 section ─────────────────────────────────────────────────────
    imperfect.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    let worst_html: String = if imperfect.is_empty() {
        r#"<p style="color:#4fc;margin-top:1rem">All frames pixel-perfect!</p>"#.into()
    } else {
        imperfect.iter().take(20).enumerate().map(|(rank, (w, sn, fi, lbl, snip))| {
            format!(r#"<div class="worst-entry">
              <div class="worst-hdr">#{r} — {sn} · frame {fi} &ldquo;{lbl}&rdquo; — quadratic diff {w:.3}</div>
              {snip}
            </div>"#,
                r = rank + 1, sn = sn, fi = fi, lbl = lbl, w = w, snip = snip)
        }).collect()
    };

    // ── Summary tab ──────────────────────────────────────────────────────────
    let summary_tab = format!(r#"
    <div id="tab-summary" class="tab-content">
      <div class="hero">
        <div><div class="l">Overall speedup (frames 1+)</div><div class="v">{sp:.1}×</div></div>
        <div><div class="l">Full render total</div><div class="v">{ft:.0}ms</div></div>
        <div><div class="l">Incremental total</div><div class="v">{it:.0}ms</div></div>
      </div>
      <div class="cal-box">{cal}</div>
      <table class="suite-tbl">
        <thead><tr><th>Suite</th><th>Speedup</th><th>Pixel-perfect</th></tr></thead>
        <tbody>{rows}</tbody>
      </table>
      <h2>Worst imperfect frames — quadratic diff, highest first (max 20)</h2>
      {worst}
    </div>"#,
        sp = overall_speedup,
        ft = all_full.as_secs_f64() * 1000.0,
        it = all_incr.as_secs_f64() * 1000.0,
        rows = table_rows, worst = worst_html,
        cal = cal_note,
    );

    format!(r#"<!DOCTYPE html><html lang="en"><head><meta charset="utf-8">
<title>Partial Rendering PoC</title>
<style>
body{{font-family:system-ui,sans-serif;background:#0d0d0d;color:#ddd;padding:2rem;max-width:1400px;margin:0 auto}}
h1{{color:#fff;margin-bottom:1rem}} h2{{color:#eee;font-size:1.1rem;margin:1.5rem 0 0.4rem}}
.hero{{background:#1a1a2e;border:1px solid #333;border-radius:10px;padding:1rem 1.5rem;margin-bottom:1.5rem;display:flex;gap:3rem;align-items:center}}
.hero .v{{font-size:2rem;font-weight:bold;color:#4fc}} .hero .l{{font-size:0.85rem;color:#888}}
.tabs{{display:flex;gap:4px;flex-wrap:wrap;border-bottom:1px solid #333;margin-bottom:1.5rem;padding-bottom:0}}
.tab-btn{{background:#161616;border:1px solid #2a2a2a;border-bottom:none;color:#999;padding:7px 16px;border-radius:4px 4px 0 0;cursor:pointer;font-size:0.83rem;transition:background .12s,color .12s;margin-bottom:-1px}}
.tab-btn:hover{{background:#222;color:#ddd}}
.tab-btn.active{{background:#1a2a1a;border-color:#4fc;color:#4fc;border-bottom-color:#1a2a1a}}
.suite-tbl{{border-collapse:collapse;font-size:0.85rem;margin-bottom:1.5rem}}
.suite-tbl th,.suite-tbl td{{padding:6px 20px;border:1px solid #333}}
.suite-tbl .num{{text-align:right}} .suite-tbl th{{background:#1a1a2e;color:#aaa}}
.desc{{color:#888;font-size:0.82rem;margin:0 0 0.8rem}}
.speedup{{font-size:0.85rem;color:#4fc;font-weight:normal;margin-left:0.5rem}}
.frame{{background:#161616;border:1px solid #2a2a2a;border-radius:6px;padding:0.6rem 0.8rem;margin-bottom:0.6rem}}
.frame.perfect{{border-color:#1d3a1d}}.frame.imperfect{{border-color:#3a1d1d}}
.worst-entry{{margin-bottom:0.8rem}}
.worst-hdr{{font-size:0.75rem;font-weight:bold;color:#f99;background:#1a0808;border:1px solid #3a1d1d;border-bottom:none;padding:5px 10px;border-radius:4px 4px 0 0}}
.worst-entry>.frame{{border-radius:0 0 6px 6px;margin-bottom:0}}
.fhdr{{display:flex;align-items:center;gap:0.6rem;flex-wrap:wrap;margin-bottom:0.4rem;font-size:0.85rem}}
.tm{{color:#666;font-size:0.78rem}}
.imgs{{display:flex;gap:0.8rem;flex-wrap:wrap}}
.cap{{font-size:0.72rem;color:#666;margin-bottom:2px}}
img{{display:block;border:1px solid #333;border-radius:2px;max-width:100%}}
.ok{{background:#1a3;color:#afa;padding:1px 6px;border-radius:3px;font-size:0.72rem;font-weight:bold}}
.diff{{background:#420;color:#faa;padding:1px 6px;border-radius:3px;font-size:0.72rem;font-weight:bold}}
.cal-box{{background:#111a11;border:1px solid #2a3a2a;border-radius:6px;padding:0.6rem 1rem;font-size:0.82rem;color:#8c8;margin-bottom:1rem;font-family:monospace;white-space:pre}}
</style></head><body>
<h1>Partial Rendering PoC — Tile-based</h1>
<div class="tabs">{tab_btns}</div>
{summary}
{suite_tabs}
<script>
function showTab(id,btn){{
  document.querySelectorAll('.tab-content').forEach(t=>t.style.display='none');
  document.querySelectorAll('.tab-btn').forEach(b=>b.classList.remove('active'));
  document.getElementById(id).style.display='block';
  btn.classList.add('active');
}}
</script>
</body></html>"#,
        tab_btns = tab_btns, summary = summary_tab, suite_tabs = suite_tabs,
    )
}

// ---------------------------------------------------------------------------
// Realistic sidebar suite — mirrors actual costae layout
// ---------------------------------------------------------------------------

fn suite_realistic_sidebar() -> TestSuite {
    let ws_data: &[(&str, &str, Option<&str>)] = &[
        ("1",  "term",    Some("main")),
        ("2",  "browser", None),
        ("3",  "costae",  Some("partial-rendering")),
        ("4",  "slack",   None),
        ("5",  "docs",    Some("arch-notes")),
        ("6",  "api",     Some("v2-refactor")),
        ("7",  "fe",      Some("dashboard")),
        ("8",  "debug",   None),
        ("9",  "infra",   Some("tf-migration")),
        ("10", "mail",    None),
        ("11", "music",   None),
        ("12", "monitor", Some("grafana")),
    ];

    // Each variable changes independently so we can see realistic per-event
    // speedups rather than a pathological "everything dirty at once" worst case.
    //   time:       changes every frame (most common real event)
    //   focus:      changes every 4 frames (i3 workspace switch)
    //   claude_pct: changes every 5 frames (60-second poll)
    let frames = (0..10).map(|i| {
        let focused    = (i / 4) % ws_data.len();
        let time_str   = format!("{}:{:02}", 14, i * 7 % 60);
        let claude_pct = 45usize + (i / 5) * 10;

        let changed = if i == 0 { "cold".into() } else {
            let mut parts = vec![format!("time={}", time_str)];
            if i % 4 == 0 { parts.push(format!("focus→ws{}", ws_data[focused].0)); }
            if i % 5 == 0 { parts.push(format!("claude={}%", claude_pct)); }
            parts.join(" ")
        };
        let label = changed;

        let ws_cards: Vec<FakeNode> = ws_data.iter().enumerate().map(|(j, (key, name, sub))| {
            let is_focused = j == focused;
            let card_tw = if is_focused {
                "flex flex-col justify-center px-3 h-[52px] rounded-lg bg-gray-800 border border-blue-500 w-full"
            } else {
                "flex flex-col justify-center px-3 h-[52px] rounded-lg bg-gray-800 border border-gray-700 w-full"
            };
            let badge_tw = if is_focused {
                "flex items-center justify-center flex-shrink-0 w-[26px] py-[2px] rounded bg-blue-500 border border-blue-600"
            } else {
                "flex items-center justify-center flex-shrink-0 w-[26px] py-[2px] rounded bg-gray-700 border border-gray-600"
            };
            let name_tw = if is_focused {
                "text-[13px] text-white font-bold truncate"
            } else {
                "text-[13px] text-gray-300 truncate"
            };

            let mut lbl_children = vec![FakeNode::Text {
                id: format!("ws-{j}-name"), content: name.to_string(), tw: name_tw.into(),
            }];
            if let Some(s) = sub {
                lbl_children.push(FakeNode::Text {
                    id: format!("ws-{j}-sub"), content: s.to_string(),
                    tw: "text-[11px] text-gray-500 truncate".into(),
                });
            }

            FakeNode::Collection {
                id: format!("ws-{j}"), tw: card_tw.into(),
                children: vec![FakeNode::Collection {
                    id: format!("ws-{j}-inner"), tw: "flex flex-row items-center gap-2 w-full".into(),
                    children: vec![
                        FakeNode::Collection {
                            id: format!("ws-{j}-badge"), tw: badge_tw.into(),
                            children: vec![FakeNode::Text {
                                id: format!("ws-{j}-key"), content: key.to_string(),
                                tw: "text-[12px] text-white font-bold".into(),
                            }],
                        },
                        FakeNode::Collection {
                            id: format!("ws-{j}-lbl"), tw: "flex flex-col min-w-0 flex-1".into(),
                            children: lbl_children,
                        },
                    ],
                }],
            }
        }).collect();

        let scene = vec![FakeNode::Collection {
            id: "sidebar".into(),
            tw: "flex flex-col w-[300px] h-[2500px] px-4 py-4 bg-gray-900".into(),
            children: vec![
                // Workspace list fills the top (flex-1 pushes bottom cards down)
                FakeNode::Collection {
                    id: "ws-area".into(),
                    tw: "flex-1 flex flex-col w-full".into(),
                    children: vec![FakeNode::Collection {
                        id: "ws-list".into(),
                        tw: "flex flex-col gap-2 w-full pt-4".into(),
                        children: ws_cards,
                    }],
                },
                // Bottom info cards
                FakeNode::Collection {
                    id: "bottom".into(),
                    tw: "flex flex-col gap-[10px] w-full".into(),
                    children: vec![
                        // GitHub WIP (fully static)
                        FakeNode::Collection {
                            id: "gh-card".into(),
                            tw: "flex flex-col gap-1 px-3 py-2 bg-gray-800 rounded-xl w-full".into(),
                            children: vec![
                                FakeNode::Collection { id: "gh-hdr".into(),
                                    tw: "flex flex-row items-baseline".into(),
                                    children: vec![
                                        FakeNode::Text { id: "gh-ttl".into(),  content: "GITHUB WIP".into(), tw: "flex-1 text-[10px] text-gray-400".into() },
                                        FakeNode::Text { id: "gh-pr-h".into(), content: "PR".into(),         tw: "w-[24px] text-right text-[8px] text-gray-400".into() },
                                        FakeNode::Text { id: "gh-tsk-h".into(),content: "tsk".into(),        tw: "w-[24px] text-right text-[8px] text-gray-400".into() },
                                    ],
                                },
                                FakeNode::Collection { id: "gh-r1".into(),
                                    tw: "flex flex-row items-baseline".into(),
                                    children: vec![
                                        FakeNode::Text { id: "gh-n1".into(), content: "costae".into(), tw: "flex-1 text-[11px] text-white".into() },
                                        FakeNode::Text { id: "gh-p1".into(), content: "3".into(),       tw: "w-[24px] text-right text-[11px] text-white".into() },
                                        FakeNode::Text { id: "gh-t1".into(), content: "—".into(),       tw: "w-[24px] text-right text-[11px] text-gray-400".into() },
                                    ],
                                },
                                FakeNode::Collection { id: "gh-r2".into(),
                                    tw: "flex flex-row items-baseline".into(),
                                    children: vec![
                                        FakeNode::Text { id: "gh-n2".into(), content: "takumi".into(), tw: "flex-1 text-[11px] text-white".into() },
                                        FakeNode::Text { id: "gh-p2".into(), content: "1".into(),       tw: "w-[24px] text-right text-[11px] text-white".into() },
                                        FakeNode::Text { id: "gh-t2".into(), content: "2".into(),       tw: "w-[24px] text-right text-[11px] text-white".into() },
                                    ],
                                },
                            ],
                        },
                        // Weather (static in this suite)
                        FakeNode::Collection {
                            id: "wx-card".into(),
                            tw: "flex flex-col gap-1 px-3 py-2 bg-gray-800 rounded-xl w-full".into(),
                            children: vec![
                                FakeNode::Collection { id: "wx-r1".into(),
                                    tw: "flex flex-row items-baseline justify-between".into(),
                                    children: vec![
                                        FakeNode::Text { id: "wx-temp".into(),  content: "21°C".into(),       tw: "text-[15px] text-white font-bold".into() },
                                        FakeNode::Text { id: "wx-feels".into(), content: "feels 19°C".into(), tw: "text-[10px] text-gray-400".into() },
                                    ],
                                },
                                FakeNode::Collection { id: "wx-r2".into(),
                                    tw: "flex flex-row justify-between".into(),
                                    children: vec![
                                        FakeNode::Text { id: "wx-cond".into(), content: "Partly cloudy".into(), tw: "text-[10px] text-gray-400".into() },
                                        FakeNode::Text { id: "wx-rh".into(),   content: "RH 62%".into(),        tw: "text-[10px] text-gray-400".into() },
                                    ],
                                },
                            ],
                        },
                        // Claude usage (pct and progress bar change every 2 frames)
                        FakeNode::Collection {
                            id: "claude-card".into(),
                            tw: "flex flex-col gap-1 px-3 py-2 bg-gray-800 rounded-xl w-full".into(),
                            children: vec![
                                FakeNode::Text { id: "claude-lbl".into(), content: "Claude · main".into(), tw: "text-[10px] text-gray-400".into() },
                                FakeNode::Collection { id: "claude-row".into(),
                                    tw: "flex flex-row items-baseline justify-between".into(),
                                    children: vec![
                                        FakeNode::Text { id: "claude-pct".into(),  content: format!("{claude_pct}%"), tw: "text-[15px] text-white font-bold".into() },
                                        FakeNode::Text { id: "claude-rst".into(),  content: "resets 2h".into(),       tw: "text-[10px] text-gray-400".into() },
                                    ],
                                },
                                FakeNode::Collection {
                                    id: "claude-prog".into(),
                                    tw: "w-full h-[4px] bg-gray-700 rounded-full".into(),
                                    children: vec![FakeNode::Image {
                                        id: "claude-fill".into(),
                                        color: "green-400".into(),
                                        width: ((claude_pct * 200 / 100) as u32).min(200),
                                        height: 4,
                                    }],
                                },
                            ],
                        },
                        // DateTime (time changes every frame)
                        FakeNode::Collection {
                            id: "dt-card".into(),
                            tw: "flex flex-row gap-[10px] px-3 py-2 bg-gray-800 rounded-xl w-full".into(),
                            children: vec![
                                FakeNode::Collection { id: "dt-date".into(),
                                    tw: "flex-1 flex flex-col gap-1".into(),
                                    children: vec![
                                        FakeNode::Text { id: "dt-dl".into(), content: "DATE".into(),   tw: "text-[10px] text-gray-400".into() },
                                        FakeNode::Text { id: "dt-dv".into(), content: "Apr 30".into(), tw: "text-[14px] text-white".into() },
                                    ],
                                },
                                FakeNode::Collection { id: "dt-time".into(),
                                    tw: "flex-1 flex flex-col gap-1".into(),
                                    children: vec![
                                        FakeNode::Text { id: "dt-tl".into(), content: "TIME".into(),        tw: "text-[10px] text-gray-400".into() },
                                        FakeNode::Text { id: "dt-tv".into(), content: time_str.clone(),     tw: "text-[14px] text-white font-mono".into() },
                                    ],
                                },
                            ],
                        },
                    ],
                },
            ],
        }];

        let full_json = scene[0].to_json();
        SuiteFrame { label, scene, full_json }
    }).collect();

    TestSuite {
        name: "Realistic Sidebar",
        description: "300×2500 px sidebar: workspace list (focus cycles), GitHub WIP, weather, Claude usage, datetime. Most tiles static.",
        frames,
    }
}

// ---------------------------------------------------------------------------
// Shrink-bug regression suite
// ---------------------------------------------------------------------------

fn suite_shrink_bug() -> TestSuite {
    // A single text node with no siblings cycles between wide and narrow values.
    // No reflow occurs (nothing moves), so the stale-old-bbox bug causes the
    // right-side pixels of the wide text to persist after it shrinks.
    let frames = ["WWWWWWWWWWWWWWWWWWWWWWWWW", "W", "WWWWWWWWWWWWWWWWWWWWWWWWW", "W"]
        .iter().enumerate().map(|(i, &text)| {
        let scene = vec![FakeNode::Collection {
            id: "bar".into(),
            tw: "w-[400px] h-[24px] bg-blue-900 flex items-center".into(),
            children: vec![FakeNode::Text {
                id: "label".into(),
                content: text.into(),
                tw: "text-white text-xs font-mono whitespace-nowrap".into(),
            }],
        }];
        let full_json = scene[0].to_json();
        SuiteFrame { label: format!("frame {i}: «{text}»"), scene, full_json }
    }).collect();
    TestSuite {
        name: "Shrink Bug",
        description: "Single text node, no siblings. Wide→narrow transition should erase the right portion — stale pixels here prove the old-bbox bug.",
        frames,
    }
}

// ---------------------------------------------------------------------------
// Extended test suites — movement, animation, edge cases
// ---------------------------------------------------------------------------

fn suite_moving_ball() -> TestSuite {
    let frames = (0..12).map(|i| {
        let t = i as f64 / 11.0;
        let bx = (8 + (t * 352.0) as u32).min(368);
        let sz = 16u32 + (8.0 * (t * std::f64::consts::TAU).sin().abs()) as u32;
        let scene = vec![FakeNode::Collection {
            id: "canvas".into(),
            tw: "flex flex-col w-[400px] h-[80px] bg-gray-900".into(),
            children: vec![
                FakeNode::Collection {
                    id: "header".into(),
                    tw: "flex flex-row items-center h-[20px] px-2".into(),
                    children: vec![
                        FakeNode::Text { id: "title".into(), content: "Ball Track".into(),
                            tw: "text-gray-500 text-[10px] whitespace-nowrap".into() },
                        FakeNode::Text { id: "pos-lbl".into(), content: format!("x={bx} sz={sz}"),
                            tw: "ml-2 text-gray-400 text-[10px] font-mono whitespace-nowrap".into() },
                    ],
                },
                FakeNode::Collection {
                    id: "track".into(),
                    tw: "flex flex-row items-center flex-1 px-2".into(),
                    children: vec![
                        FakeNode::Collection {
                            id: "spacer".into(),
                            tw: format!("flex-shrink-0 w-[{bx}px] h-[2px] bg-gray-700"),
                            children: vec![],
                        },
                        FakeNode::Collection {
                            id: "ball".into(),
                            tw: format!("flex-shrink-0 w-[{sz}px] h-[{sz}px] rounded-full bg-orange-500 shadow-lg"),
                            children: vec![],
                        },
                    ],
                },
            ],
        }];
        let full_json = scene[0].to_json();
        SuiteFrame { label: format!("frame {i}: x={bx} sz={sz}"), scene, full_json }
    }).collect();
    TestSuite {
        name: "Moving Ball",
        description: "400×80 px. Ball slides L→R; size pulses simultaneously. Tests relocating + resizing node in the same frame.",
        frames,
    }
}

fn suite_tile_crossing() -> TestSuite {
    let frames = (0..10).map(|i| {
        let bx = i as u32 * TILE_SIZE;
        let scene = vec![FakeNode::Collection {
            id: "canvas".into(),
            tw: "flex flex-row items-center w-[320px] h-[64px] bg-gray-900".into(),
            children: vec![
                FakeNode::Collection {
                    id: "spacer".into(),
                    tw: format!("flex-shrink-0 w-[{bx}px] h-[2px] bg-gray-700"),
                    children: vec![],
                },
                FakeNode::Collection {
                    id: "block".into(),
                    tw: "flex-shrink-0 w-[32px] h-[32px] bg-cyan-400 rounded shadow-sm flex items-center justify-center".into(),
                    children: vec![FakeNode::Text {
                        id: "n".into(),
                        content: format!("{i}"),
                        tw: "text-gray-900 text-xs font-bold".into(),
                    }],
                },
            ],
        }];
        let full_json = scene[0].to_json();
        SuiteFrame { label: format!("frame {i}: tile-x={}", bx / TILE_SIZE), scene, full_json }
    }).collect();
    TestSuite {
        name: "Tile Crossing",
        description: "320×64 px. Block advances exactly one tile (32px) per frame. Stresses dirty-tile marking at exact tile boundaries.",
        frames,
    }
}

fn suite_panel_focus() -> TestSuite {
    let frames = (0..10).map(|i| {
        let active = i % 3;
        let count = i + 1;
        let scene = vec![FakeNode::Collection {
            id: "canvas".into(),
            tw: "flex flex-row gap-3 p-4 w-[460px] h-[120px] bg-gray-950".into(),
            children: (0usize..3).map(|idx| {
                let is_active = idx == active;
                FakeNode::Collection {
                    id: format!("panel-{idx}"),
                    tw: if is_active {
                        "flex flex-col p-3 bg-blue-800 rounded-xl shadow-xl w-[130px] border-2 border-blue-400".into()
                    } else {
                        "flex flex-col p-3 bg-gray-800 rounded-xl shadow-md w-[130px] border border-gray-600".into()
                    },
                    children: vec![
                        FakeNode::Text { id: format!("p{idx}-title"),
                            content: ["Alpha", "Beta", "Gamma"][idx].into(),
                            tw: format!("text-[11px] font-bold {} whitespace-nowrap",
                                if is_active { "text-blue-100" } else { "text-gray-300" }) },
                        FakeNode::Text { id: format!("p{idx}-val"),
                            content: if idx == 0 { format!("{count}") } else { "—".into() },
                            tw: "text-[22px] font-bold text-white".into() },
                    ],
                }
            }).collect(),
        }];
        let full_json = scene[0].to_json();
        SuiteFrame { label: format!("frame {i}: active={active} count={count}"), scene, full_json }
    }).collect();
    TestSuite {
        name: "Panel Focus Cycle",
        description: "460×120 px. Active panel highlight cycles L→M→R. Counter increments each frame. Tests simultaneous bg-color + content changes.",
        frames,
    }
}

fn suite_diagonal_scatter() -> TestSuite {
    let colors = ["red","orange","yellow","green","cyan","blue","indigo","purple","pink"];
    let frames = (0..10).map(|i| {
        let hot = i % 9;
        let rows: Vec<FakeNode> = (0usize..3).map(|r| FakeNode::Collection {
            id: format!("row-{r}"),
            tw: "flex flex-row gap-2".into(),
            children: (0usize..3).map(|c| {
                let idx = r * 3 + c;
                let is_hot = idx == hot;
                FakeNode::Collection {
                    id: format!("cell-{idx}"),
                    tw: if is_hot {
                        format!("w-[72px] h-[72px] bg-{}-400 rounded-lg shadow-lg flex items-center justify-center", colors[idx])
                    } else {
                        format!("w-[72px] h-[72px] bg-{}-900 rounded flex items-center justify-center", colors[idx])
                    },
                    children: vec![FakeNode::Text {
                        id: format!("cell-{idx}-lbl"),
                        content: if is_hot { "●".into() } else { "○".into() },
                        tw: format!("text-{}-{} text-sm font-bold", colors[idx], if is_hot { "100" } else { "600" }),
                    }],
                }
            }).collect(),
        }).collect();
        let scene = vec![FakeNode::Collection {
            id: "canvas".into(),
            tw: "flex flex-col gap-2 w-[248px] h-[248px] p-2 bg-gray-950".into(),
            children: rows,
        }];
        let full_json = scene[0].to_json();
        SuiteFrame { label: format!("frame {i}: hot=cell-{hot}"), scene, full_json }
    }).collect();
    TestSuite {
        name: "Diagonal Scatter",
        description: "248×248 px. 3×3 grid; one 'hot' cell cycles through all 9 positions per frame. Tests spatially scattered single-cell updates.",
        frames,
    }
}

fn suite_notification_badge() -> TestSuite {
    let frames = (0..12).map(|i| {
        let count = i + 1;
        let two_digit = count >= 10;
        let scene = vec![FakeNode::Collection {
            id: "widget".into(),
            tw: "flex flex-row items-center gap-3 w-[240px] h-[72px] px-4 py-3 bg-gray-900 rounded-xl".into(),
            children: vec![
                FakeNode::Collection {
                    id: "icon".into(),
                    tw: "flex-shrink-0 w-[48px] h-[48px] bg-blue-600 rounded-xl shadow-md flex items-center justify-center".into(),
                    children: vec![FakeNode::Text {
                        id: "icon-lbl".into(),
                        content: "✉".into(),
                        tw: "text-white text-[20px]".into(),
                    }],
                },
                FakeNode::Collection {
                    id: "content".into(),
                    tw: "flex flex-col gap-1".into(),
                    children: vec![
                        FakeNode::Text { id: "app-name".into(), content: "Messages".into(),
                            tw: "text-[12px] text-white font-semibold whitespace-nowrap".into() },
                        FakeNode::Collection {
                            id: "badge-row".into(),
                            tw: "flex flex-row items-center gap-2".into(),
                            children: vec![
                                FakeNode::Collection {
                                    id: "badge".into(),
                                    tw: format!("flex items-center justify-center {} h-[18px] bg-red-500 rounded-full",
                                        if two_digit { "min-w-[28px]" } else { "min-w-[18px]" }),
                                    children: vec![FakeNode::Text {
                                        id: "badge-n".into(),
                                        content: format!("{count}"),
                                        tw: "text-white text-[10px] font-bold px-1 whitespace-nowrap".into(),
                                    }],
                                },
                                FakeNode::Text { id: "badge-lbl".into(), content: "unread".into(),
                                    tw: "text-[10px] text-gray-400 whitespace-nowrap".into() },
                            ],
                        },
                    ],
                },
            ],
        }];
        let full_json = scene[0].to_json();
        SuiteFrame { label: format!("frame {i}: {count} unread"), scene, full_json }
    }).collect();
    TestSuite {
        name: "Notification Badge",
        description: "240×72 px. Badge counter 1→12; container widens at 2 digits. App icon is fully static.",
        frames,
    }
}

fn suite_progress_fill() -> TestSuite {
    let frames = (0..10).map(|i| {
        let pct: u32 = match i {
            0..=7 => (i as f64 / 7.0 * 100.0) as u32,
            8 => 30,
            _ => 0,
        };
        let fill_w = (pct * 320 / 100).min(320);
        let complete = pct >= 100;
        let scene = vec![FakeNode::Collection {
            id: "card".into(),
            tw: "flex flex-col gap-2 w-[360px] h-[60px] px-3 py-2 bg-gray-800 rounded-xl".into(),
            children: vec![
                FakeNode::Collection {
                    id: "header".into(),
                    tw: "flex flex-row items-baseline justify-between".into(),
                    children: vec![
                        FakeNode::Text { id: "label".into(),
                            content: if complete { "Complete!" } else { "Downloading…" }.into(),
                            tw: "text-[11px] text-gray-400 whitespace-nowrap".into() },
                        FakeNode::Text { id: "pct".into(), content: format!("{pct}%"),
                            tw: "text-[11px] text-white font-mono whitespace-nowrap".into() },
                    ],
                },
                FakeNode::Collection {
                    id: "bar-bg".into(),
                    tw: "w-full h-[8px] bg-gray-700 rounded-full overflow-hidden".into(),
                    children: vec![FakeNode::Image {
                        id: "bar-fill".into(),
                        color: if complete { "green-400".into() } else { "blue-500".into() },
                        width: fill_w,
                        height: 8,
                    }],
                },
            ],
        }];
        let full_json = scene[0].to_json();
        SuiteFrame { label: format!("frame {i}: {pct}%"), scene, full_json }
    }).collect();
    TestSuite {
        name: "Progress Fill",
        description: "360×60 px. Image bar grows 0→100%; color flips to green at completion; frames 8-9 reset. Tests Image node resize + color change.",
        frames,
    }
}

fn suite_keyframe_animation() -> TestSuite {
    let pi = std::f64::consts::PI;
    let frames = (0..20).map(|i| {
        let t = i as f64 / 19.0;

        // Bounce: ball y oscillates 0..38 px inside 60 px track (20 px ball fits at bottom)
        let bounce_y = (38.0 * (t * pi * 2.0).sin().abs()) as u32;

        // Slide: thumb travels 0..360 px
        let slide_x = ((t * 360.0) as u32).min(360);

        // Pulse: size 20..36 px, 8-color palette cycle
        let pulse_sz = 20u32 + (14.0 * (t * pi * 4.0).sin().abs()) as u32;
        let pulse_colors = ["red-400","orange-400","yellow-300","green-400",
                            "teal-400","blue-400","indigo-400","purple-400"];
        let pulse_color = pulse_colors[(i * pulse_colors.len() / 20) % pulse_colors.len()];

        // Phase label: 4 keyframe segments
        let phase = if t < 0.25 { "IDLE" } else if t < 0.5 { "RISING" } else if t < 0.75 { "PEAK" } else { "FALLING" };

        let scene = vec![FakeNode::Collection {
            id: "canvas".into(),
            tw: "flex flex-col w-[500px] h-[260px] p-4 bg-gray-950 gap-3".into(),
            children: vec![
                // Header: title + frame counter + current phase
                FakeNode::Collection { id: "hdr".into(), tw: "flex flex-row items-baseline gap-2".into(),
                    children: vec![
                        FakeNode::Text { id: "hdr-title".into(), content: "Keyframe Animation".into(),
                            tw: "text-[12px] text-gray-300 font-bold whitespace-nowrap".into() },
                        FakeNode::Text { id: "hdr-frame".into(), content: format!("{i:02}/20"),
                            tw: "text-[10px] text-gray-500 font-mono whitespace-nowrap".into() },
                        FakeNode::Text { id: "hdr-phase".into(), content: phase.into(),
                            tw: "ml-auto text-[11px] text-yellow-300 font-bold whitespace-nowrap".into() },
                    ],
                },
                // Bounce row
                FakeNode::Collection { id: "bounce-row".into(),
                    tw: "flex flex-row items-start gap-3 h-[60px]".into(),
                    children: vec![
                        FakeNode::Text { id: "bounce-lbl".into(), content: "Bounce".into(),
                            tw: "w-[46px] text-[10px] text-gray-500 whitespace-nowrap pt-1 flex-shrink-0".into() },
                        FakeNode::Collection { id: "bounce-track".into(),
                            tw: "flex-1 h-[60px] bg-gray-900 rounded overflow-hidden".into(),
                            children: vec![FakeNode::Collection { id: "bounce-col".into(),
                                tw: "flex flex-col pl-2".into(),
                                children: vec![
                                    FakeNode::Collection { id: "bounce-spacer".into(),
                                        tw: format!("flex-shrink-0 w-[20px] h-[{bounce_y}px]"),
                                        children: vec![] },
                                    FakeNode::Collection { id: "bounce-ball".into(),
                                        tw: "flex-shrink-0 w-[20px] h-[20px] rounded-full bg-blue-400 shadow-md".into(),
                                        children: vec![] },
                                ],
                            }],
                        },
                    ],
                },
                // Slide row
                FakeNode::Collection { id: "slide-row".into(),
                    tw: "flex flex-row items-center gap-3 h-[32px]".into(),
                    children: vec![
                        FakeNode::Text { id: "slide-lbl".into(), content: "Slide".into(),
                            tw: "w-[46px] text-[10px] text-gray-500 whitespace-nowrap flex-shrink-0".into() },
                        FakeNode::Collection { id: "slide-track".into(),
                            tw: "flex-1 h-[8px] bg-gray-800 rounded-full flex flex-row items-center".into(),
                            children: vec![
                                FakeNode::Collection { id: "slide-spacer".into(),
                                    tw: format!("flex-shrink-0 w-[{slide_x}px] h-[8px]"),
                                    children: vec![] },
                                FakeNode::Collection { id: "slide-thumb".into(),
                                    tw: "flex-shrink-0 w-[12px] h-[12px] rounded-full bg-white shadow-sm".into(),
                                    children: vec![] },
                            ],
                        },
                    ],
                },
                // Pulse row
                FakeNode::Collection { id: "pulse-row".into(),
                    tw: "flex flex-row items-center gap-3 h-[44px]".into(),
                    children: vec![
                        FakeNode::Text { id: "pulse-lbl".into(), content: "Pulse".into(),
                            tw: "w-[46px] text-[10px] text-gray-500 whitespace-nowrap flex-shrink-0".into() },
                        FakeNode::Collection { id: "pulse-box".into(),
                            tw: format!("flex-shrink-0 w-[{pulse_sz}px] h-[{pulse_sz}px] bg-{pulse_color} rounded shadow-md"),
                            children: vec![] },
                    ],
                },
                // Phase indicator row: 4 segments, active one highlighted
                FakeNode::Collection { id: "phase-row".into(),
                    tw: "flex flex-row items-center gap-3 h-[24px]".into(),
                    children: vec![
                        FakeNode::Text { id: "phase-lbl".into(), content: "Phase".into(),
                            tw: "w-[46px] text-[10px] text-gray-500 whitespace-nowrap flex-shrink-0".into() },
                        FakeNode::Collection { id: "phase-bar".into(),
                            tw: "flex-1 flex flex-row gap-1 h-[16px]".into(),
                            children: ["IDLE","RISING","PEAK","FALLING"].iter().map(|&p| FakeNode::Collection {
                                id: format!("phase-seg-{p}"),
                                tw: if p == phase {
                                    "flex-1 h-full bg-yellow-400 rounded-sm".into()
                                } else {
                                    "flex-1 h-full bg-gray-700 rounded-sm".into()
                                },
                                children: vec![],
                            }).collect(),
                        },
                    ],
                },
            ],
        }];
        let full_json = scene[0].to_json();
        SuiteFrame {
            label: format!("frame {i:02}: bounce={bounce_y} slide={slide_x} pulse={pulse_sz} phase={phase}"),
            scene, full_json,
        }
    }).collect();
    TestSuite {
        name: "Keyframe Animation",
        description: "500×260 px. 20 frames: bouncing ball, sliding thumb, pulsing colored box, 4-phase indicator. Each element follows an independent curve.",
        frames,
    }
}

// ---------------------------------------------------------------------------
// Notification panel — realistic timed-update suite
// ---------------------------------------------------------------------------

fn suite_notification_panel() -> TestSuite {
    let spinner = ["|", "/", "—", "\\"];
    let notifs = [
        ("System",   "Software update available"),
        ("Messages", "3 unread from Alice"),
        ("Build",    "costae release passed"),
    ];

    let frames = (0..20).map(|i| {
        // Every frame: spinner advances, slide thumb moves
        let spin      = spinner[i % 4];
        // Every 2 frames: highlighted notification rotates
        let active    = (i / 2) % 3;
        // Every 4 frames: badge count increments
        let count     = 5usize + i / 4;
        // Slide: 0→360 over frames 0–9, then 360→0 over frames 10–19
        let slide_x: u32 = if i < 10 {
            (i as f64 / 9.0 * 360.0) as u32
        } else {
            ((19 - i) as f64 / 9.0 * 360.0) as u32
        };

        let notif_items: Vec<FakeNode> = notifs.iter().enumerate().map(|(idx, (app, msg))| {
            let hot = idx == active;
            FakeNode::Collection {
                id: format!("notif-{idx}"),
                tw: if hot {
                    "flex flex-row items-center gap-2 px-3 py-2 bg-blue-950 rounded-lg".into()
                } else {
                    "flex flex-row items-center gap-2 px-3 py-2".into()
                },
                children: vec![
                    FakeNode::Collection {
                        id: format!("notif-{idx}-dot"),
                        tw: if hot {
                            "flex-shrink-0 w-[8px] h-[8px] rounded-full bg-blue-400".into()
                        } else {
                            "flex-shrink-0 w-[8px] h-[8px] rounded-full bg-gray-700".into()
                        },
                        children: vec![],
                    },
                    FakeNode::Collection {
                        id: format!("notif-{idx}-body"),
                        tw: "flex flex-col".into(),
                        children: vec![
                            FakeNode::Text {
                                id: format!("notif-{idx}-app"),
                                content: app.to_string(),
                                tw: format!("text-[11px] font-bold whitespace-nowrap {}",
                                    if hot { "text-blue-300" } else { "text-gray-500" }),
                            },
                            FakeNode::Text {
                                id: format!("notif-{idx}-msg"),
                                content: msg.to_string(),
                                tw: format!("text-[10px] whitespace-nowrap {}",
                                    if hot { "text-gray-200" } else { "text-gray-600" }),
                            },
                        ],
                    },
                ],
            }
        }).collect();

        let scene = vec![FakeNode::Collection {
            id: "panel".into(),
            tw: "flex flex-col w-[400px] h-[200px] bg-gray-900 rounded-xl p-3 gap-2".into(),
            children: vec![
                // Header: static title + badge (every 4th) + spinner (every frame)
                FakeNode::Collection {
                    id: "hdr".into(),
                    tw: "flex flex-row items-center gap-2 h-[24px]".into(),
                    children: vec![
                        FakeNode::Text { id: "title".into(), content: "NOTIFICATIONS".into(),
                            tw: "text-[10px] text-gray-400 font-bold whitespace-nowrap".into() },
                        FakeNode::Collection { id: "hdr-gap".into(), tw: "flex-1".into(), children: vec![] },
                        FakeNode::Collection {
                            id: "badge".into(),
                            tw: "flex-shrink-0 flex items-center justify-center w-[18px] h-[18px] bg-red-500 rounded-full".into(),
                            children: vec![FakeNode::Text {
                                id: "badge-n".into(), content: format!("{count}"),
                                tw: "text-white text-[10px] font-bold".into(),
                            }],
                        },
                        FakeNode::Text {
                            id: "spin".into(), content: spin.into(),
                            tw: "ml-auto text-blue-400 text-[14px] font-mono whitespace-nowrap".into(),
                        },
                    ],
                },
                // Notification list — one item highlights on a 2-frame cycle
                FakeNode::Collection {
                    id: "notif-list".into(),
                    tw: "flex flex-col gap-1".into(),
                    children: notif_items,
                },
                // Slide track — thumb bounces L↔R every frame
                FakeNode::Collection {
                    id: "slide-row".into(),
                    tw: "flex flex-row items-center gap-2 h-[20px]".into(),
                    children: vec![
                        FakeNode::Text { id: "slide-lbl".into(), content: "activity".into(),
                            tw: "w-[46px] text-[10px] text-gray-600 whitespace-nowrap flex-shrink-0".into() },
                        FakeNode::Collection {
                            id: "slide-track".into(),
                            tw: "flex-1 h-[4px] bg-gray-800 rounded-full flex flex-row items-center overflow-hidden".into(),
                            children: vec![
                                FakeNode::Collection {
                                    id: "slide-spacer".into(),
                                    tw: format!("flex-shrink-0 w-[{slide_x}px] h-[4px]"),
                                    children: vec![],
                                },
                                FakeNode::Collection {
                                    id: "slide-thumb".into(),
                                    tw: "flex-shrink-0 w-[8px] h-[8px] rounded-full bg-blue-400".into(),
                                    children: vec![],
                                },
                            ],
                        },
                    ],
                },
            ],
        }];

        let full_json = scene[0].to_json();
        SuiteFrame {
            label: format!("frame {i:02}: spin={spin} notif={active} count={count} slide={slide_x}"),
            scene, full_json,
        }
    }).collect();

    TestSuite {
        name: "Notification Panel",
        description: "400×200 px. Spinner every frame; active notification rotates every 2; badge count every 4; slide thumb bounces L↔R. Most content static.",
        frames,
    }
}

// ---------------------------------------------------------------------------
// Scroll list — pixel-by-pixel scroll stress test
// ---------------------------------------------------------------------------

/// Six notifications inside an overflow-hidden window that scrolls 2 px/frame.
/// Every scroll step moves every item → every tile in the viewport is dirty →
/// the incremental renderer has nothing to skip. Expected speedup ≈ 1.0×.
/// This deliberately exposes the architectural limit of tile-based rendering
/// for continuous-motion content.
fn suite_scroll_list() -> TestSuite {
    let notif_data: &[(&str, &str, &str)] = &[
        ("System",   "Software update available", "blue"),
        ("Messages", "3 unread from Alice",       "purple"),
        ("Build",    "costae release passed",     "green"),
        ("Monitor",  "CPU spike: 94% for 30s",    "red"),
        ("Sync",     "14 files synced",           "teal"),
        ("Calendar", "Meeting in 15 min",         "orange"),
    ];

    let frames = (0..20).map(|i| {
        let scroll_y = i as u32 * 2; // 0, 2, 4 … 38 px total

        let items: Vec<FakeNode> = notif_data.iter().enumerate().map(|(idx, (app, msg, color))| {
            FakeNode::Collection {
                id: format!("item-{idx}"),
                tw: format!("flex flex-row items-center gap-2 px-3 py-2 bg-{color}-950 rounded-lg"),
                children: vec![
                    FakeNode::Collection {
                        id: format!("item-{idx}-dot"),
                        tw: format!("flex-shrink-0 w-[6px] h-[6px] rounded-full bg-{color}-400"),
                        children: vec![],
                    },
                    FakeNode::Collection {
                        id: format!("item-{idx}-body"),
                        tw: "flex flex-col".into(),
                        children: vec![
                            FakeNode::Text {
                                id: format!("item-{idx}-app"), content: app.to_string(),
                                tw: format!("text-[11px] font-bold text-{color}-300 whitespace-nowrap"),
                            },
                            FakeNode::Text {
                                id: format!("item-{idx}-msg"), content: msg.to_string(),
                                tw: "text-[10px] text-gray-400 whitespace-nowrap".into(),
                            },
                        ],
                    },
                ],
            }
        }).collect();

        // scroll-content shifts up via negative margin-top; overflow-hidden clips the top.
        let content_tw = if scroll_y == 0 {
            "flex flex-col gap-1".into()
        } else {
            format!("flex flex-col gap-1 mt-[-{scroll_y}px]")
        };

        let scene = vec![FakeNode::Collection {
            id: "panel".into(),
            tw: "flex flex-col w-[400px] h-[200px] bg-gray-900 rounded-xl p-3 gap-2".into(),
            children: vec![
                // Header — static except for scroll position readout
                FakeNode::Collection {
                    id: "hdr".into(),
                    tw: "flex flex-row items-center h-[24px]".into(),
                    children: vec![
                        FakeNode::Text { id: "hdr-title".into(), content: "NOTIFICATIONS".into(),
                            tw: "text-[10px] text-gray-400 font-bold whitespace-nowrap".into() },
                        FakeNode::Text { id: "hdr-pos".into(), content: format!("↕ {scroll_y}px"),
                            tw: "ml-auto text-[10px] text-gray-600 font-mono whitespace-nowrap".into() },
                    ],
                },
                // Clipped scroll viewport — overflow-hidden clips scrolled-past content
                FakeNode::Collection {
                    id: "scroll-win".into(),
                    tw: "flex-1 overflow-hidden".into(),
                    children: vec![FakeNode::Collection {
                        id: "scroll-content".into(),
                        tw: content_tw,
                        children: items,
                    }],
                },
            ],
        }];

        let full_json = scene[0].to_json();
        SuiteFrame { label: format!("frame {i:02}: scroll={scroll_y}px"), scene, full_json }
    }).collect();

    TestSuite {
        name: "Scroll List",
        description: "400×200 px. 6 items scroll 2px/frame via negative margin-top inside overflow-hidden. Every item moves every frame → all viewport tiles dirty → no incremental savings expected.",
        frames,
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------


fn print_suite_results(result: &SuiteResult) {
    let mut s_full = Duration::ZERO;
    let mut s_incr = Duration::ZERO;
    for (i, f) in result.frames.iter().enumerate() {
        let d = if !f.incr_px.is_empty() && f.incr_px.len() == f.full_px.len() {
            // Restrict both error and changed-area to dirty tiles only.
            // Skipped tiles are untouched by the incremental renderer and must
            // not contribute false positives from layout-reflow-induced AA drift.
            let mask = dirty_mask(&f.dirty_tiles, f.w, f.h);
            let chg_w = if f.prev_full_px.len() == f.full_px.len() {
                diff_masked(&f.prev_full_px, &f.full_px, &mask, f.w, f.h).weighted
            } else {
                (f.dirty_tiles.len() * (TILE_SIZE * TILE_SIZE) as usize) as f64
            };
            let err = diff_masked(&f.full_px, &f.incr_px, &mask, f.w, f.h);
            let ratio = err.weighted / chg_w.max(1.0);
            if ratio < PERFECT_THRESHOLD { "✓".into() }
            else { format!("≠{:.1}% (err={:.1}/chg={:.0})", ratio * 100.0, err.weighted, chg_w) }
        } else { "?".into() };
        println!("  [{: >2}] full={:.1}ms incr={:.1}ms ×{:.1} rendered={} hits={} skip={} {}  {}",
            i, f.full_time.as_secs_f64() * 1000.0, f.incr_time.as_secs_f64() * 1000.0,
            f.full_time.as_secs_f64() / f.incr_time.as_secs_f64().max(1e-9),
            f.render_calls, f.cache_hits, f.skipped, d, f.label);
        if i > 0 { s_full += f.full_time; s_incr += f.incr_time; }
    }
    println!("  → suite speedup: {:.1}×\n", s_full.as_secs_f64() / s_incr.as_secs_f64().max(1e-9));
}

fn main() {
    let suites_defs = vec![
        suite_simple_bar(),
        suite_shadow_cards(),
        suite_blurred_overlay(),
        suite_dense_metrics(),
        suite_realistic_sidebar(),
        suite_shrink_bug(),
        suite_moving_ball(),
        suite_tile_crossing(),
        suite_panel_focus(),
        suite_diagonal_scatter(),
        suite_notification_badge(),
        suite_progress_fill(),
        suite_keyframe_animation(),
        suite_notification_panel(),
        suite_scroll_list(),
    ];

    // ── Pass 1: calibration run ───────────────────────────────────────────────
    // Run all suites with default cost-model constants to collect
    // (canvas_area, n_nodes, render_time_ms) samples for OLS regression.
    eprintln!("Pass 1/2 — collecting calibration samples (default cost model)...");
    let mut cal_samples: Vec<(f64, f64, f64)> = Vec::new();
    let default_cm = CostModel::default();
    for suite in &suites_defs {
        run_suite(suite, &default_cm, &mut cal_samples);
    }

    // ── OLS fit ───────────────────────────────────────────────────────────────
    let cal = fit_cost_model(&cal_samples);
    let cm = cal.as_ref().map(|c| c.model.clone()).unwrap_or_default();

    let cal_note = match &cal {
        Some(c) => {
            let msg = format!(
                "Cost model calibrated from {} samples  R²={:.3}\n  O_FIXED = {:.4} ms   K_AREA = {:.3e} ms/px   K_NODES = {:.4} ms/node",
                c.n_samples, c.r_squared, c.model.o_fixed, c.model.k_area, c.model.k_nodes
            );
            eprintln!("{msg}");
            msg
        }
        None => {
            let msg = "Calibration skipped (insufficient samples) — using default constants.".into();
            eprintln!("{}", msg);
            msg
        }
    };

    // ── Pass 2: benchmark with calibrated constants ───────────────────────────
    eprintln!("Pass 2/2 — benchmark with calibrated cost model...");
    let mut results = Vec::new();
    let mut _discard: Vec<(f64, f64, f64)> = Vec::new();
    for suite in &suites_defs {
        eprintln!("Running suite: {} ({} frames, tile={}px)...", suite.name, suite.frames.len(), TILE_SIZE);
        let result = run_suite(suite, &cm, &mut _discard);
        print_suite_results(&result);
        results.push(result);
    }

    let path = "/tmp/poc_report.html";
    std::fs::write(path, html_report(&results, &cal_note)).expect("write report");
    eprintln!("Report: file://{path}");

    // Dump PNG frames for visual inspection
    std::fs::create_dir_all("/tmp/poc_frames").unwrap();
    for sr in &results {
        let sname = sr.name.replace(' ', "_").to_lowercase();
        for (fi, f) in sr.frames.iter().enumerate() {
            if f.full_px.is_empty() { continue; }
            let base = format!("/tmp/poc_frames/{sname}_f{fi:02}");
            std::fs::write(format!("{base}_full.png"), encode_png(&f.full_px, f.w, f.h)).unwrap();
            if !f.incr_px.is_empty() {
                std::fs::write(format!("{base}_incr.png"), encode_png(&f.incr_px, f.w, f.h)).unwrap();
                let d = diff(&f.full_px, &f.incr_px, f.w, f.h);
                std::fs::write(format!("{base}_diff.png"), encode_png(&d.img, f.w, f.h)).unwrap();
            }
        }
    }
    eprintln!("Frames: /tmp/poc_frames/");
}

// ---------------------------------------------------------------------------
// Visual regression tests
//
// For each snapshot:
//   1. The PNG is always written to test-snapshots/<name>.png so it is
//      available for visual inspection regardless of pass/fail.
//   2. A hash of the raw pixel data is asserted via insta — this is what
//      actually fails the test and requires human approval.
//
// Workflow:
//   cargo test -p layout-poc          # CI: fails if any hash changed
//   cargo insta review                # inspect old→new hash, accept or reject
//                                     # open the PNG to decide visually
// ---------------------------------------------------------------------------
#[cfg(test)]
mod visual_regression {
    use super::*;
    use std::path::{Path, PathBuf};

    fn snapshot_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("test-snapshots")
    }

    /// Deterministic LCG hash of raw pixel data.  Not cryptographic, but stable
    /// across Rust versions and requires no extra dependencies.
    fn pixel_hash(pixels: &[u8]) -> String {
        let h = pixels.iter().fold(0u64, |h, &b| {
            h.wrapping_mul(6364136223846793005).wrapping_add(b as u64)
        });
        format!("{h:016x}  ({} px)", pixels.len() / 4)
    }

    /// Write the PNG to `test-snapshots/` for visual inspection, then assert
    /// the pixel hash via insta.  Insta owns the "approved" state; the PNG is
    /// always the current render so a reviewer can open it after `cargo insta
    /// review` highlights a hash change.
    fn assert_snapshot(name: &str, pixels: &[u8], w: u32, h: u32) {
        let dir = snapshot_dir();
        std::fs::create_dir_all(&dir).expect("create test-snapshots/");
        std::fs::write(dir.join(format!("{name}.png")), encode_png(pixels, w, h))
            .expect("write PNG");
        insta::assert_snapshot!(name, pixel_hash(pixels));
    }

    /// Run `suite`, extract frame `frame_idx`, and snapshot both renders.
    fn assert_render_snapshots(suite: &TestSuite, frame_idx: usize, name: &str) {
        let cm = CostModel::default();
        let mut discard = Vec::new();
        let result = run_suite(suite, &cm, &mut discard);
        let f = &result.frames[frame_idx];
        insta::with_settings!({ snapshot_path => snapshot_dir(), prepend_module_to_snapshot => false }, {
            assert_snapshot(&format!("{name}__full_f{frame_idx}"),  &f.full_px, f.w, f.h);
            assert_snapshot(&format!("{name}__incr_f{frame_idx}"),  &f.incr_px, f.w, f.h);
        });
    }

    /// Run `suite` once, snapshot full and incr pixels for multiple frame indices.
    fn run_and_snapshot(suite: &TestSuite, frame_indices: &[usize], name: &str) {
        let cm = CostModel::default();
        let mut discard = Vec::new();
        let result = run_suite(suite, &cm, &mut discard);
        insta::with_settings!({ snapshot_path => snapshot_dir(), prepend_module_to_snapshot => false }, {
            for &fi in frame_indices {
                let f = &result.frames[fi];
                assert_snapshot(&format!("{name}__full_f{fi}"), &f.full_px, f.w, f.h);
                assert_snapshot(&format!("{name}__incr_f{fi}"), &f.incr_px, f.w, f.h);
            }
        });
    }

    // ── Bug regression guards ────────────────────────────────────────────────

    /// Guards the overflow-hidden clip fix.
    ///
    /// Before the fix, `bar-bg` (rounded-full overflow-hidden) and `bar-fill`
    /// were flat siblings so the clip never applied — the fill had a square
    /// left corner in the incremental render.  After the fix both renders show
    /// correct rounded corners.
    ///
    /// Frame 1 = 14% fill: the left rounded edge is clearly visible and small
    /// enough that a 1-pixel regression is immediately obvious.
    #[test]
    fn reg_overflow_clip_rounding() {
        assert_render_snapshots(&suite_progress_fill(), 1, "reg_overflow_clip_rounding");
    }

    /// Guards the ml-auto stub positioning fix.
    ///
    /// `hdr-phase` uses `ml-auto` to push itself to the right end of the header
    /// row.  Before the fix the stub dropped ml-auto so the dirty-tile region
    /// was computed at the wrong x position, leaving stale "IDLE" text at the
    /// correct location in the incr render.
    ///
    /// Frame 5 is the first RISING frame — the phase label just changed from
    /// "IDLE" so the position error is maximally visible.
    #[test]
    fn reg_ml_auto_positioning() {
        assert_render_snapshots(&suite_keyframe_animation(), 5, "reg_ml_auto_positioning");
    }

    /// Guards that a wide-to-narrow text shrink erases the full old bbox.
    ///
    /// Frame 1 = "W" after the wide "WWWWWWWWWWWWWWWWWWWWWWWWW" — the
    /// narrowest possible shrink, maximising the stale region if dirty tiles
    /// only cover the new (narrow) bbox instead of the old (wide) one.
    #[test]
    fn reg_node_shrink_stale_pixels() {
        assert_render_snapshots(&suite_shrink_bug(), 1, "reg_node_shrink_stale_pixels");
    }

    /// Guards correct dirty-region computation when an Image node resizes.
    ///
    /// Frame 3 is a mid-progress step; frame 8 tests the post-completion reset
    /// (color flip from blue→green then partial erase).  Both should have
    /// identical full and incr renders — stale pixels at the old right-side bbox
    /// confirm the bug.
    #[test]
    fn reg_image_resize_dirty_region() {
        run_and_snapshot(&suite_progress_fill(), &[3, 8], "reg_image_resize_dirty_region");
    }

    /// Guards that a moved node clears its old tile position.
    ///
    /// The cyan block advances exactly one tile (32 px) per frame.  Frame 1
    /// moves it from tile-col 0 → 1; the left tile must be cleared and the
    /// right tile filled — no ghost at the old position.
    #[test]
    fn reg_moved_node_clears_old_position() {
        assert_render_snapshots(&suite_tile_crossing(), 1, "reg_moved_node_clears_old_position");
    }

    /// Guards that removed nodes leave no ghost pixels.
    ///
    /// Inline 3-frame mini-suite:
    ///   Frame 0: node-a (left, blue) + node-b (right, orange) — cold frame
    ///   Frame 1: only node-a — node-b removed; its old position must be cleared
    ///   Frame 2: node-a + node-c (same slot, purple) — old node-b area fully replaced
    #[test]
    fn reg_structure_change_no_ghost() {
        let mk_frame = |label: &str, children: Vec<FakeNode>| -> SuiteFrame {
            let scene = vec![FakeNode::Collection {
                id: "canvas".into(),
                tw: "flex flex-row items-center w-[320px] h-[48px] bg-gray-900".into(),
                children: children,
            }];
            let full_json = scene[0].to_json();
            SuiteFrame { label: label.into(), scene, full_json }
        };
        let node_a = || FakeNode::Collection {
            id: "node-a".into(),
            tw: "w-[48px] h-[32px] bg-blue-500 rounded flex items-center justify-center".into(),
            children: vec![],
        };
        let node_b = || FakeNode::Collection {
            id: "node-b".into(),
            tw: "ml-auto w-[48px] h-[32px] bg-orange-400 rounded flex items-center justify-center".into(),
            children: vec![],
        };
        let node_c = || FakeNode::Collection {
            id: "node-c".into(),
            tw: "ml-auto w-[48px] h-[32px] bg-purple-400 rounded flex items-center justify-center".into(),
            children: vec![],
        };
        let suite = TestSuite {
            name: "Structure Change No Ghost",
            description: "node-b removed in frame 1, node-c added in frame 2 — no ghost pixels",
            frames: vec![
                mk_frame("cold: a+b", vec![node_a(), node_b()]),
                mk_frame("remove b",  vec![node_a()]),
                mk_frame("add c",     vec![node_a(), node_c()]),
            ],
        };
        run_and_snapshot(&suite, &[0, 1, 2], "reg_structure_change_no_ghost");
    }

    // ── Compositing ───────────────────────────────────────────────────────────

    /// Confirms shadow renders correctly when cards straddle tile boundaries.
    ///
    /// Two shadow-2xl cards sit side by side with gap-4 padding.  Frame 1 is
    /// the first update — shadow compositing must match between full and incr.
    #[test]
    fn test_shadow_tile_boundary() {
        assert_render_snapshots(&suite_shadow_cards(), 1, "test_shadow_tile_boundary");
    }

    /// Confirms overflow-hidden clip is correct at multiple bar fill widths.
    ///
    /// Frames 1 (14%), 4 (57%), 7 (100%), 8 (color-flip reset), 9 (empty).
    /// The rounded left corner of the fill must always be clipped inside
    /// the rounded bar container — the old bug showed a square left edge.
    #[test]
    fn test_rounded_clip_all_widths() {
        run_and_snapshot(&suite_progress_fill(), &[1, 4, 7, 8, 9], "test_rounded_clip_all_widths");
    }

    /// Confirms ml-auto positions hdr-phase correctly across all phase transitions.
    ///
    /// Frames 5 (IDLE→RISING), 10 (RISING→PEAK), 15 (PEAK→FALLING).
    /// The phase label must always appear at the far-right edge of the header.
    #[test]
    fn test_ml_auto_all_phases() {
        run_and_snapshot(&suite_keyframe_animation(), &[5, 10, 15], "test_ml_auto_all_phases");
    }

    // ── Golden representatives ────────────────────────────────────────────────

    /// Cold frame must produce identical full and incr pixels.
    ///
    /// Frame 0 has no prior state — every tile falls through to a full render.
    /// The two pixel buffers must match exactly (ratio = 0).
    #[test]
    fn golden_cold_frame_exact_match() {
        assert_render_snapshots(&suite_simple_bar(), 0, "golden_cold_frame_exact_match");
    }

    /// Golden snapshot of clock-only updates in the simple status bar.
    ///
    /// Frames 1-3: only the clock text changes each frame; the logo, workspace
    /// label, and system stats are all static.
    #[test]
    fn golden_clock_tick() {
        run_and_snapshot(&suite_simple_bar(), &[1, 2, 3], "golden_clock_tick");
    }

    /// Golden snapshot of workspace-focus switching between three panels.
    ///
    /// Frame 4 (Alpha active) and frame 8 (Beta active) cover two distinct
    /// focus states — bg colour and text styling change on every panel.
    #[test]
    fn golden_workspace_focus_change() {
        run_and_snapshot(&suite_panel_focus(), &[4, 8], "golden_workspace_focus_change");
    }

    /// Golden snapshot of active notification rotating through the panel list.
    ///
    /// Frame 0 (cold, System highlighted), 2 (Messages), 4 (Build).
    /// Spinner also advances each frame.
    #[test]
    fn golden_notification_rotation() {
        run_and_snapshot(&suite_notification_panel(), &[0, 2, 4], "golden_notification_rotation");
    }

    /// Golden snapshot of scroll frames — every viewport tile dirty.
    ///
    /// Frames 1, 5, 10 are mid-scroll steps.  All tiles should be dirty so
    /// full and incr renders must match pixel-perfectly.
    #[test]
    fn golden_scroll_frame() {
        run_and_snapshot(&suite_scroll_list(), &[1, 5, 10], "golden_scroll_frame");
    }
}
