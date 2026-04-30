use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

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
    global:           GlobalContext,
    changed_ids:      Vec<String>,
    structure_changed: bool,
    // Cached natural dimensions (W, H) for each node, used by the stub layout pass.
    node_dims: HashMap<String, (f32, f32)>,
}

impl Ctx {
    fn fresh() -> Self {
        Self {
            global: new_ctx_with_fonts(),
            changed_ids: Vec::new(),
            structure_changed: false,
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
        ctx.structure_changed = true;
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
        ctx.structure_changed = true;
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

fn stub_scene_json(node: &FakeNode, dims: &HashMap<String, (f32, f32)>) -> serde_json::Value {
    match node {
        FakeNode::Text { id, .. } => {
            let (w, h) = dims.get(id.as_str()).copied().unwrap_or((0.0, 0.0));
            serde_json::json!({"type":"container","style":{"width":w,"height":h}})
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

/// Walk the FakeNode tree and find the node with the given id.
fn find_node<'a>(root: &'a FakeNode, id: &str) -> Option<&'a FakeNode> {
    if root.id() == id { return Some(root); }
    if let FakeNode::Collection { children, .. } = root {
        for child in children {
            if let Some(found) = find_node(child, id) { return Some(found); }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Flat tile scene — only nodes touching (tx, ty, tile+2*buf) x (tile+2*buf),
// each absolutely positioned using pre-measured bboxes.
// Layout is trivial: no flex computation, all positions already known.
// ---------------------------------------------------------------------------

fn collect_flat(
    node: &FakeNode,
    bboxes: &HashMap<String, Rect>,
    // query region in scene coordinates (expanded by shadow buf)
    qx: f32, qy: f32, qw: f32, qh: f32,
    // offset so scene (qx,qy) → canvas (0,0)
    out: &mut Vec<serde_json::Value>,
) {
    let Some(r) = bboxes.get(node.id()) else { return };
    // skip nodes entirely outside query region
    if r.x + r.w <= qx || r.x >= qx + qw || r.y + r.h <= qy || r.y >= qy + qh { return; }
    let lx = r.x - qx;
    let ly = r.y - qy;
    match node {
        FakeNode::Text { content, tw, .. } => {
            out.push(serde_json::json!({
                "type": "text", "text": content, "tw": tw,
                "style": { "position": "absolute", "left": lx, "top": ly, "width": r.w }
            }));
        }
        FakeNode::Image { color, width, height, .. } => {
            out.push(serde_json::json!({
                "type": "container",
                "tw": format!("bg-{}", color),
                "style": { "display": "block", "position": "absolute",
                    "left": lx, "top": ly,
                    "width": *width as f32, "height": *height as f32 }
            }));
        }
        FakeNode::Collection { tw, children, .. } => {
            // Only pass visual tw classes (bg, shadow, rounded, border, ring…).
            // Layout classes (flex, gap, items-*, w-*, p-*, etc.) have no effect
            // on a childless absolute box and trigger a takumi rendering bug that
            // shifts sibling text glyphs at certain canvas positions.
            let visual_tw: String = tw.split_whitespace()
                .filter(|cls| {
                    let c = *cls;
                    c.starts_with("bg-") || c.starts_with("shadow")
                    || c.starts_with("rounded") || c.starts_with("border")
                    || c.starts_with("ring") || c.starts_with("opacity")
                    || c.starts_with("blur") || c.starts_with("backdrop")
                    || c.starts_with("brightness") || c.starts_with("contrast")
                    || c.starts_with("saturate") || c.starts_with("grayscale")
                    || c.starts_with("invert") || c.starts_with("hue-rotate")
                    || c.starts_with("drop-shadow")
                })
                .collect::<Vec<_>>()
                .join(" ");
            out.push(serde_json::json!({
                "type": "container", "tw": visual_tw,
                "style": { "position": "absolute",
                    "left": lx, "top": ly, "width": r.w, "height": r.h }
            }));
            for child in children { collect_flat(child, bboxes, qx, qy, qw, qh, out); }
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

struct DiffResult { weighted: f64, total: u32, max_ch: u8, img: Vec<u8> }

fn diff(a: &[u8], b: &[u8], w: u32, h: u32) -> DiffResult {
    let (mut weighted, mut max_ch) = (0.0f64, 0u8);
    let mut img = vec![0u8; a.len()];
    for i in (0..a.len().min(b.len())).step_by(4) {
        let m = (a[i]as i32-b[i]as i32).unsigned_abs().max(
                (a[i+1]as i32-b[i+1]as i32).unsigned_abs()).max(
                (a[i+2]as i32-b[i+2]as i32).unsigned_abs()) as u8;
        max_ch = max_ch.max(m);
        // Quadratic weighting: diff=1 → 0.01, diff=10 → 1.0 (100× ratio vs linear's 10×).
        // Small anti-aliasing deviations contribute almost nothing; real errors dominate.
        let linear = (m as f64 / 10.0).min(1.0);
        let w_px = linear * linear;
        weighted += w_px;
        let intensity = (w_px * 255.0) as u8;
        if intensity > 0 {
            img[i]=255; img[i+1]=0; img[i+2]=255; img[i+3]=intensity;
        } else {
            img[i]=a[i]/5; img[i+1]=a[i+1]/5; img[i+2]=a[i+2]/5; img[i+3]=255;
        }
    }
    DiffResult { weighted, total: w*h, max_ch, img }
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

struct FrameResult { label: String, full_time: Duration, incr_time: Duration, full_px: Vec<u8>, incr_px: Vec<u8>, w: u32, h: u32, render_calls: u32, skipped: u32 }
struct SuiteResult { name: &'static str, description: &'static str, frames: Vec<FrameResult> }

const TILE_SIZE: u32 = 32;
const SHADOW_BUF: u32 = 32; // extra border around each tile to capture shadow bleed

// Two tiles whose shadow-expanded intervals overlap are merged into one band.
// Overlap condition: distance < 1 + 2*SHADOW_BUF/TILE_SIZE  →  distance ≤ MERGE_THRESHOLD.
const MERGE_THRESHOLD: u32 = 2 * SHADOW_BUF / TILE_SIZE;

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

fn run_suite(suite: &TestSuite) -> SuiteResult {
    let mut incr_ctx = Ctx::fresh();
    let mut incr_set: ManagedSet<FakeNode> = ManagedSet::new();
    let mut frame_buf: Vec<u8> = Vec::new();
    let mut prev_bboxes: HashMap<String, Rect> = HashMap::new();
    // Bboxes from the PREVIOUS frame's stub layout (not full-measure), used for
    // moved-node detection so we compare stub vs stub and avoid false positives.
    let mut prev_stub_bboxes: HashMap<String, Rect> = HashMap::new();

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
        incr_ctx.structure_changed = false;
        let t = Instant::now();

        // 1. Reconcile — populates changed_ids and structure_changed
        incr_set.reconcile(f.scene.clone(), &mut incr_ctx, &mut ());

        // 2. Measure layout — two strategies depending on whether the structure changed.
        //
        //    Structure change  →  full measure_layout on the actual scene (shapes all text).
        //    Content-only      →  (a) measure each changed text node in isolation to get its
        //                             new (W, H), then (b) run a cheap stub-layout pass that
        //                             replaces every node with a fixed-size container so taffy
        //                             re-computes positions with zero additional text shaping.
        let mut bboxes: HashMap<String, Rect> = HashMap::new();

        let stub_bboxes: HashMap<String, Rect>;

        if incr_ctx.structure_changed || frame_buf.is_empty() {
            // Full measure; seed the dims cache so subsequent frames can stub.
            let scene_json = f.scene[0].to_json();
            let node = parse_layout(&scene_json).unwrap_or_else(|_| Node::container(vec![]));
            let measured = takumi_measure_layout(
                RenderOptions::builder().global(&incr_ctx.global)
                    .viewport(Viewport::new((None, None))).node(node).build()
            ).expect("measure layout");
            collect_bboxes(&measured, &f.scene[0], &mut bboxes);
            for (id, r) in &bboxes { incr_ctx.node_dims.insert(id.clone(), (r.w, r.h)); }
            // Also run the stub pass so prev_stub_bboxes is in the same coordinate
            // system as future stub runs (avoids false-positive moved-node detection).
            let stub_json = stub_scene_json(&f.scene[0], &incr_ctx.node_dims);
            let snode = parse_layout(&stub_json).unwrap_or_else(|_| Node::container(vec![]));
            let smeasured = takumi_measure_layout(
                RenderOptions::builder().global(&incr_ctx.global)
                    .viewport(Viewport::new((None, None))).node(snode).build()
            ).expect("seed stub layout");
            let mut sb = HashMap::new();
            collect_bboxes(&smeasured, &f.scene[0], &mut sb);
            stub_bboxes = sb;
        } else {
            // Step (a): update node_dims for any leaf whose dimensions may have changed.
            for id in &incr_ctx.changed_ids {
                if let Some(node) = find_node(&f.scene[0], id) {
                    match node {
                        FakeNode::Text { .. } => {
                            let dims = measure_natural(node, &incr_ctx.global);
                            incr_ctx.node_dims.insert(id.clone(), dims);
                        }
                        FakeNode::Image { width, height, .. } => {
                            // Dimensions are declared on the node; no shaping needed.
                            incr_ctx.node_dims.insert(id.clone(), (*width as f32, *height as f32));
                        }
                        _ => {}
                    }
                }
            }
            // Step (b): stub layout — replace all leaves with fixed-size containers.
            let stub_json = stub_scene_json(&f.scene[0], &incr_ctx.node_dims);
            let node = parse_layout(&stub_json).unwrap_or_else(|_| Node::container(vec![]));
            let measured = takumi_measure_layout(
                RenderOptions::builder().global(&incr_ctx.global)
                    .viewport(Viewport::new((None, None))).node(node).build()
            ).expect("stub layout");
            let mut sb = HashMap::new();
            collect_bboxes(&measured, &f.scene[0], &mut sb);
            bboxes = sb.clone();
            stub_bboxes = sb;
        }

        // 3. Compute dirty tiles.
        let cols = (w + TILE_SIZE - 1) / TILE_SIZE;
        let rows = (h + TILE_SIZE - 1) / TILE_SIZE;
        let mut dirty: HashSet<(u32,u32)> = HashSet::new();

        if frame_buf.len() != (w * h * 4) as usize {
            // First frame: everything dirty.
            frame_buf = vec![0u8; (w * h * 4) as usize];
            for ty in 0..rows { for tx in 0..cols { dirty.insert((tx, ty)); } }
        } else {
            // Changed-content tiles (new position).
            for id in &incr_ctx.changed_ids {
                if let Some(r) = bboxes.get(id.as_str()) {
                    mark_dirty(r, TILE_SIZE, w, h, &mut dirty);
                }
            }
            // Nodes that moved due to layout reflow (e.g. clock shifts when CPU width changes).
            // Compare stub vs prev_stub so both sides use the same coordinate system.
            for (id, new_r) in &bboxes {
                if let Some(old_r) = prev_stub_bboxes.get(id.as_str()) {
                    if (new_r.x - old_r.x).abs() > 0.5 || (new_r.y - old_r.y).abs() > 0.5 {
                        mark_dirty(new_r, TILE_SIZE, w, h, &mut dirty);
                        mark_dirty(old_r, TILE_SIZE, w, h, &mut dirty);
                    }
                }
            }
        }

        // 4. Group dirty tiles into bands, choose the axis (X or Y) with smaller
        //    estimated total render area, then do one render call per band.
        let bands = if dirty.is_empty() {
            vec![]
        } else {
            let by = compute_bands_y(&dirty);
            let bx = compute_bands_x(&dirty);
            if estimated_area(&by) <= estimated_area(&bx) { by } else { bx }
        };
        let render_calls = bands.len() as u32;
        let skipped = cols * rows - dirty.len() as u32;

        for band in &bands {
            let batch_px_x = band.min_tx * TILE_SIZE;
            let batch_px_y = band.min_ty * TILE_SIZE;
            let batch_w = (band.max_tx - band.min_tx + 1) * TILE_SIZE;
            let batch_h = (band.max_ty - band.min_ty + 1) * TILE_SIZE;

            let buf = SHADOW_BUF as f32;
            let qx = batch_px_x as f32 - buf;
            let qy = batch_px_y as f32 - buf;
            let qw = batch_w as f32 + 2.0 * buf;
            let qh = batch_h as f32 + 2.0 * buf;
            let canvas_w = batch_w + 2 * SHADOW_BUF;
            let canvas_h = batch_h + 2 * SHADOW_BUF;

            let mut nodes: Vec<serde_json::Value> = Vec::new();
            collect_flat(&f.scene[0], &bboxes, qx, qy, qw, qh, &mut nodes);

            let scene = serde_json::json!({
                "type": "container",
                "style": { "display": "block", "position": "relative",
                    "width": canvas_w as f32,
                    "height": canvas_h as f32,
                    "overflow": "hidden" },
                "children": nodes
            });
            let node = parse_layout(&scene).unwrap_or_else(|_| Node::container(vec![]));
            let band_px = takumi_render(
                RenderOptions::builder().global(&incr_ctx.global)
                    .viewport(Viewport::new((None,None))).node(node).build()
            ).expect("band render").into_raw();

            for &(tx, ty) in &band.tiles {
                let px_x = tx * TILE_SIZE;
                let px_y = ty * TILE_SIZE;
                let off_x = SHADOW_BUF + (tx - band.min_tx) * TILE_SIZE;
                let off_y = SHADOW_BUF + (ty - band.min_ty) * TILE_SIZE;
                let tile_px = crop_pixels(&band_px, canvas_w, off_x, off_y, TILE_SIZE, TILE_SIZE);
                stitch(&mut frame_buf, w, h, &tile_px, TILE_SIZE, px_x, px_y);
            }
        }

        let incr_time = t.elapsed();
        let incr_px = frame_buf.clone();
        prev_bboxes = bboxes;
        prev_stub_bboxes = stub_bboxes;

        FrameResult { label: f.label.clone(), full_time, incr_time, full_px, incr_px, w, h, render_calls, skipped }
    }).collect();

    SuiteResult { name: suite.name, description: suite.description, frames }
}

// ---------------------------------------------------------------------------
// HTML report — tabbed (Summary + one tab per suite)
// ---------------------------------------------------------------------------

const PERFECT_THRESHOLD: f64 = 1.0; // quadratic weighted diff below which a frame is "pixel-perfect"

fn html_report(suites: &[SuiteResult]) -> String {
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

            let (d, incr_uri, diff_uri) = if !f.incr_px.is_empty() && f.incr_px.len() == f.full_px.len() {
                let d = diff(&f.full_px, &f.incr_px, f.w, f.h);
                let du = data_uri(&d.img, f.w, f.h);
                let iu = data_uri(&f.incr_px, f.w, f.h);
                (Some(d), iu, du)
            } else {
                (None, String::new(), String::new())
            };

            let perfect = d.as_ref().map(|d| d.weighted < PERFECT_THRESHOLD).unwrap_or(false);
            if perfect { n_perfect += 1; }

            let badge = if perfect { r#"<span class="ok">✓</span>"# } else { r#"<span class="diff">≠</span>"# };
            let diff_stat = d.as_ref().map(|d| format!(
                "diff={:.3} ({:.4}%)", d.weighted, d.weighted / d.total as f64 * 100.0
            )).unwrap_or_default();

            frames_html.push_str(&format!(r#"
            <div class="frame {cls}">
              <div class="fhdr"><strong>Frame {fi}</strong> — {lbl} {badge}
                <span class="tm">full {ft:.1}ms · incr {it:.1}ms · {sp:.1}×</span>
                <span class="tm">{rc} renders · {sk} skipped · {ds}</span>
              </div>
              <div class="imgs">
                <div><div class="cap">Full</div><img src="{fu}" style="width:{pw}px;height:{ph}px;image-rendering:pixelated"></div>
                <div><div class="cap">Incremental</div><img src="{iu}" style="width:{pw}px;height:{ph}px;image-rendering:pixelated"></div>
                <div><div class="cap">Diff</div><img src="{du}" style="width:{pw}px;height:{ph}px;image-rendering:pixelated"></div>
              </div>
            </div>"#,
                cls = if perfect { "perfect" } else { "imperfect" },
                lbl = f.label, badge = badge,
                ft = f.full_time.as_secs_f64() * 1000.0, it = f.incr_time.as_secs_f64() * 1000.0,
                sp = speedup_f, rc = f.render_calls, sk = f.skipped,
                fu = full_uri, iu = incr_uri, du = diff_uri,
                pw = pw, ph = ph, ds = diff_stat,
            ));

            // Collect for worst-20 (compact thumbnail in summary)
            if let Some(ref dr) = d {
                if !perfect {
                    // Thumbnail: fit within 240×320 while preserving aspect ratio
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
                        <div><div class="cap">Diff</div><img src="{du}" style="width:{tw}px;height:{th}px;image-rendering:pixelated"></div>
                      </div>
                    </div>"#,
                        sn = suite.name, fi = fi, lbl = f.label, badge = badge,
                        ft = f.full_time.as_secs_f64() * 1000.0, it = f.incr_time.as_secs_f64() * 1000.0,
                        sp = speedup_f, ds = diff_stat,
                        fu = full_uri, iu = incr_uri, du = diff_uri, tw = tw, th = th,
                    );
                    imperfect.push((dr.weighted, suite.name, fi, f.label.clone(), snippet));
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
// Main
// ---------------------------------------------------------------------------


fn main() {
    eprintln!("Loading fonts...");

    let suites_defs = vec![
        suite_simple_bar(),
        suite_shadow_cards(),
        suite_blurred_overlay(),
        suite_dense_metrics(),
        suite_realistic_sidebar(),
    ];

    let mut results = Vec::new();
    for suite in &suites_defs {
        eprintln!("Running suite: {} ({} frames, tile={}px)...", suite.name, suite.frames.len(), TILE_SIZE);
        let result = run_suite(suite);

        let mut s_full = Duration::ZERO;
        let mut s_incr = Duration::ZERO;
        for (i, f) in result.frames.iter().enumerate() {
            let d = if !f.incr_px.is_empty() && f.incr_px.len()==f.full_px.len() {
                let d = diff(&f.full_px, &f.incr_px, f.w, f.h);
                if d.weighted<PERFECT_THRESHOLD { "✓".into() } else { format!("≠w={:.2}({:.3}%)",d.weighted,d.weighted/d.total as f64*100.0) }
            } else { "?".into() };
            println!("  [{: >2}] full={:.1}ms incr={:.1}ms ×{:.1} tiles={}/{} {}  {}",
                i, f.full_time.as_secs_f64()*1000.0, f.incr_time.as_secs_f64()*1000.0,
                f.full_time.as_secs_f64()/f.incr_time.as_secs_f64().max(1e-9),
                f.render_calls, f.render_calls+f.skipped, d, f.label);
            if i>0 { s_full+=f.full_time; s_incr+=f.incr_time; }
        }
        println!("  → suite speedup: {:.1}×\n", s_full.as_secs_f64()/s_incr.as_secs_f64().max(1e-9));
        results.push(result);
    }

    let path = "/tmp/poc_report.html";
    std::fs::write(path, html_report(&results)).expect("write report");
    eprintln!("Report: file://{path}");
}
