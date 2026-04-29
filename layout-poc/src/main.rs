use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use image::{ImageBuffer, Rgba};
use takumi::{GlobalContext, layout::{Viewport, node::Node}, rendering::{RenderOptions, render as takumi_render}, resources::{font::FontResource, image::ImageSource}};

use costae::managed_set::{Lifecycle, ManagedSet};
use costae::managed_set::reconcile::Reconcile;
use costae::layout::parse_layout;
use costae::render::find_font_files;

// ---------------------------------------------------------------------------
// GlobalContext factory — loads system fonts into a fresh independent context
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
}
impl std::fmt::Display for FakeNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{}", self.id()) }
}

// ---------------------------------------------------------------------------
// Per-node render result + state
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Rendered {
    pixels:          Arc<Vec<u8>>,
    w:               u32,   // full image width  (content + 2 × overflow_padding)
    h:               u32,   // full image height (content + 2 × overflow_padding)
    content_w:       u32,   // logical content width  (for layout / stub sizing)
    content_h:       u32,   // logical content height (for layout / stub sizing)
    overflow_padding: u32,
}

enum FakeNodeState {
    Text       { r: Rendered, content: String },
    Image      { r: Rendered },
    Collection { r: Rendered, tw: String, children: ManagedSet<FakeNode>, last_child_arcs: Vec<Arc<Vec<u8>>> },
}
impl FakeNodeState {
    fn rendered(&self) -> &Rendered {
        match self { Self::Text{r,..}|Self::Image{r,..}|Self::Collection{r,..} => r }
    }
}

// ---------------------------------------------------------------------------
// Context — owns an independent GlobalContext so full/incremental don't share
// ---------------------------------------------------------------------------

struct Ctx {
    global:           GlobalContext,
    render_calls:     u32,
    skipped:          u32,
    /// Extra transparent space added around every isolated node render to
    /// capture overflow effects (shadows, glows). Defaults to 24px.
    overflow_padding: u32,
}
impl Ctx {
    fn fresh() -> Self { Self { global: new_ctx_with_fonts(), render_calls: 0, skipped: 0, overflow_padding: 24 } }
    fn reset(&mut self) { self.render_calls = 0; self.skipped = 0; }
}

// ---------------------------------------------------------------------------
// Render helpers (explicit GlobalContext, no shared static)
// ---------------------------------------------------------------------------

fn render_padded(json: &serde_json::Value, padding: u32, g: &GlobalContext) -> Rendered {
    let wrapped = if padding > 0 {
        serde_json::json!({"type":"container","tw":format!("p-[{}px]",padding),"children":[json]})
    } else {
        json.clone()
    };
    let node = parse_layout(&wrapped).unwrap_or_else(|_| Node::container(vec![]));
    let img = takumi_render(
        RenderOptions::builder().global(g).viewport(Viewport::new((None,None))).node(node).build()
    ).expect("render");
    let (w, h) = img.dimensions();
    let content_w = w.saturating_sub(2 * padding);
    let content_h = h.saturating_sub(2 * padding);
    Rendered { pixels: Arc::new(img.into_raw()), w, h, content_w, content_h, overflow_padding: padding }
}

fn render_with_stubs(tw: &str, spec: &[FakeNode], set: &ManagedSet<FakeNode>, ctx: &mut Ctx) -> Rendered {
    let p = ctx.overflow_padding;
    let stubs: Vec<serde_json::Value> = spec.iter()
        .filter_map(|s| set.get(&s.id().to_string()).map(|st| (s, st)))
        .map(|(s, st)| {
            let r = st.rendered();
            let p = r.overflow_padding;
            // Forward layout-only classes (ml-auto, grow, shrink, flex-*, etc.) from the
            // original node so the parent's flex layout is identical to the full render.
            // Visual classes (shadow, bg, rounded) are intentionally dropped — they're
            // already baked into the stub image.
            let original_tw = match s {
                FakeNode::Text { tw, .. } | FakeNode::Collection { tw, .. } => tw.as_str(),
                FakeNode::Image { .. } => "",
            };
            let ltw = layout_classes(original_tw);
            let neg = -(p as f32);
            // Content-sized container participates in flex layout normally.
            // The padded image is absolutely positioned with negative insets
            // so the shadow bleeds out to all sides without affecting layout.
            // Use raw `style` properties (not tw) to avoid Tailwind parser
            // limitations with negative inset values.
            serde_json::json!({
                "type": "container",
                "tw": ltw,
                "style": {
                    "position": "relative",
                    "width": format!("{}px", r.content_w),
                    "height": format!("{}px", r.content_h),
                    "overflow": "visible"
                },
                "children": [{
                    "type": "image",
                    "src": format!("stub://{}", s.id()),
                    "style": {
                        "position": "absolute",
                        "top": format!("{}px", neg),
                        "left": format!("{}px", neg),
                        "width": format!("{}px", r.w),
                        "height": format!("{}px", r.h)
                    }
                }]
            })
        }).collect();

    let inner = serde_json::json!({"type":"container","tw":tw,"children":stubs});
    let scene = if p > 0 {
        serde_json::json!({"type":"container","tw":format!("p-[{}px]",p),"children":[inner]})
    } else { inner };
    ctx.render_calls += 1;

    for s in spec {
        if let Some(st) = set.get(&s.id().to_string()) {
            let r = st.rendered();
            if let Some(img) = ImageBuffer::<Rgba<u8>, _>::from_raw(r.w, r.h, (*r.pixels).clone()) {
                ctx.global.persistent_image_store.insert(
                    format!("stub://{}", s.id()),
                    ImageSource::from(img),
                );
            }
        }
    }
    let node = parse_layout(&scene).unwrap_or_else(|_| Node::container(vec![]));
    let img = takumi_render(
        RenderOptions::builder().global(&ctx.global).viewport(Viewport::new((None,None))).node(node).build()
    ).expect("render stubs");
    let (w, h) = img.dimensions();
    let content_w = w.saturating_sub(2 * p);
    let content_h = h.saturating_sub(2 * p);
    Rendered { pixels: Arc::new(img.into_raw()), w, h, content_w, content_h, overflow_padding: p }
}

fn child_arcs(spec: &[FakeNode], set: &ManagedSet<FakeNode>) -> Vec<Arc<Vec<u8>>> {
    spec.iter().filter_map(|s| set.get(&s.id().to_string())).map(|st| Arc::clone(&st.rendered().pixels)).collect()
}

// ---------------------------------------------------------------------------
// Image utilities
// ---------------------------------------------------------------------------

/// Extract Tailwind classes that affect parent flex layout but produce no pixels of their own.
/// These need to be forwarded to stub wrapper containers so the parent's flex layout stays correct.
fn layout_classes(tw: &str) -> String {
    tw.split_whitespace()
        .filter(|cls| {
            let base = cls.trim_start_matches('-');
            base.starts_with("m-")  || base.starts_with("ml-") || base.starts_with("mr-") ||
            base.starts_with("mt-") || base.starts_with("mb-") || base.starts_with("mx-") ||
            base.starts_with("my-") || base.starts_with("grow") || base.starts_with("shrink") ||
            base.starts_with("flex-") || base.starts_with("basis-") ||
            base.starts_with("order-") || base.starts_with("self-") ||
            base == "grow" || base == "shrink"
        })
        .collect::<Vec<_>>()
        .join(" ")
}

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

/// Weighted diff: per-pixel weight = min(max_channel_diff / 10, 1.0).
/// A channel difference of ≥10/255 counts as a fully different pixel;
/// smaller differences count proportionally. The diff image uses magenta
/// with intensity proportional to the weight.
struct DiffResult { weighted: f64, total: u32, max_ch: u8, img: Vec<u8> }

fn diff(a: &[u8], b: &[u8], w: u32, h: u32) -> DiffResult {
    let (mut weighted, mut max_ch) = (0.0f64, 0u8);
    let mut img = vec![0u8; a.len()];
    for i in (0..a.len().min(b.len())).step_by(4) {
        let m = (a[i]as i32-b[i]as i32).unsigned_abs().max(
                (a[i+1]as i32-b[i+1]as i32).unsigned_abs()).max(
                (a[i+2]as i32-b[i+2]as i32).unsigned_abs()) as u8;
        max_ch = max_ch.max(m);
        let w_px = (m as f64 / 10.0).min(1.0);
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
// Lifecycle
// ---------------------------------------------------------------------------

impl Lifecycle for FakeNode {
    type Key=String; type State=FakeNodeState; type Context=Ctx; type Output=(); type Error=anyhow::Error;
    fn key(&self) -> String { self.id().to_string() }

    fn enter(self, ctx: &mut Ctx, _: &mut ()) -> Result<FakeNodeState> {
        match self {
            FakeNode::Text { id: _, content, tw } => {
                ctx.render_calls += 1;
                let r = render_padded(&serde_json::json!({"type":"text","text":&content,"tw":&tw}), ctx.overflow_padding, &ctx.global);
                Ok(FakeNodeState::Text { r, content })
            }
            FakeNode::Image { id: _, color, width, height } => {
                ctx.render_calls += 1;
                // display:inline-block so width/height are respected when this node
                // participates in the inline layout of the outer padding wrapper.
                let r = render_padded(&serde_json::json!({"type":"container","style":{"display":"inline-block"},"tw":format!("w-[{}px] h-[{}px] bg-{}",width,height,color)}), ctx.overflow_padding, &ctx.global);
                Ok(FakeNodeState::Image { r })
            }
            FakeNode::Collection { id: _, tw, children } => {
                let mut cs: ManagedSet<FakeNode> = ManagedSet::new();
                cs.reconcile(children.clone(), ctx, &mut ());
                let arcs = child_arcs(&children, &cs);
                let r = render_with_stubs(&tw, &children, &cs, ctx);
                Ok(FakeNodeState::Collection { r, tw, children: cs, last_child_arcs: arcs })
            }
        }
    }

    fn reconcile_self(self, state: &mut FakeNodeState, ctx: &mut Ctx, _: &mut ()) -> Result<()> {
        match (self, state) {
            (FakeNode::Text{content,tw,..}, FakeNodeState::Text{r,content:old}) => {
                if content != *old {
                    ctx.render_calls += 1;
                    *r = render_padded(&serde_json::json!({"type":"text","text":&content,"tw":&tw}), ctx.overflow_padding, &ctx.global);
                    *old = content;
                }
                Ok(())
            }
            (FakeNode::Image{color,width,height,..}, FakeNodeState::Image{r}) => {
                if width!=r.content_w||height!=r.content_h {
                    ctx.render_calls+=1;
                    *r = render_padded(&serde_json::json!({"type":"container","tw":format!("w-[{}px] h-[{}px] bg-{}",width,height,color)}), ctx.overflow_padding, &ctx.global);
                }
                Ok(())
            }
            (FakeNode::Collection{tw,children,..}, FakeNodeState::Collection{r,tw:otw,children:cs,last_child_arcs:arcs}) => {
                cs.reconcile(children.clone(), ctx, &mut ());
                let new_arcs = child_arcs(&children, cs);
                let dirty = new_arcs.len()!=arcs.len() || new_arcs.iter().zip(arcs.iter()).any(|(a,b)|!Arc::ptr_eq(a,b));
                if dirty { *r=render_with_stubs(&tw,&children,cs,ctx); *arcs=new_arcs; *otw=tw; }
                else { ctx.skipped+=1; }
                Ok(())
            }
            _ => Err(anyhow::anyhow!("type mismatch"))
        }
    }

    fn exit(state: FakeNodeState, ctx: &mut Ctx, _: &mut ()) -> Result<()> {
        if let FakeNodeState::Collection{mut children,..}=state { children.reconcile(vec![],ctx,&mut ()); }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Test suites
// ---------------------------------------------------------------------------

struct SuiteFrame { label: String, scene: Vec<FakeNode>, full_json: serde_json::Value }
struct TestSuite  { name: &'static str, description: &'static str, frames: Vec<SuiteFrame> }

// --- Suite 1: simple status bar (baseline, no effects) ---

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
                {"type":"container","tw":"w-[16px] h-[16px] bg-blue-500"},
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

// --- Suite 2: shadow cards (box-shadow + rounded, changing counter, static card) ---

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
    TestSuite{name:"Shadow Cards",description:"Two rounded+shadow cards. Left changes each frame (counter+message), right is fully static — tests that shadow computation is skipped for the cached card.",frames}
}

// --- Suite 3: blurred overlay panel (backdrop-blur + opacity) ---

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
    TestSuite{name:"Blurred Overlay",description:"Rounded panel with shadow-inner and opacity. Temperature value changes every frame; alert fires every 4th. The static badge (shadow-md, rounded) stays cached.",frames}
}

// --- Suite 4: dense metrics grid (6 shadow columns, only 2 update) ---

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
    TestSuite{name:"Dense Metrics Grid",description:"6 shadow+rounded columns. CPU (col 0) and GPU (col 2) change each frame; the other 4 stay cached — tests that shadow-md is not recomputed for static columns.",frames}
}

// ---------------------------------------------------------------------------
// Benchmark — interleaved, separate GlobalContexts
// ---------------------------------------------------------------------------

struct FrameResult { label: String, full_time: Duration, incr_time: Duration, full_px: Vec<u8>, incr_px: Vec<u8>, w: u32, h: u32, render_calls: u32, skipped: u32 }
struct SuiteResult { name: &'static str, description: &'static str, frames: Vec<FrameResult> }

fn run_suite(suite: &TestSuite) -> SuiteResult {
    // Two completely independent contexts: full gets a fresh one every frame,
    // incremental keeps one alive across frames.
    let mut incr_ctx = Ctx::fresh();
    let mut incr_set: ManagedSet<FakeNode> = ManagedSet::new();

    let frames: Vec<FrameResult> = suite.frames.iter().map(|f| {
        // Full — fresh context per frame (no cross-frame caching)
        let mut full_ctx = Ctx::fresh();
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
        full_ctx.render_calls += 1;
        let full_time = t.elapsed();

        // Incremental — persistent context
        incr_ctx.reset();
        let t = Instant::now();
        incr_set.reconcile(f.scene.clone(), &mut incr_ctx, &mut ());
        let incr_time = t.elapsed();
        let incr_px = incr_set.get(&f.scene[0].id().to_string())
            .map(|s| {
                let r = s.rendered();
                let p = r.overflow_padding;
                if p > 0 {
                    crop_pixels(&r.pixels, r.w, p, p, r.content_w, r.content_h)
                } else {
                    (*r.pixels).clone()
                }
            }).unwrap_or_default();

        FrameResult {
            label: f.label.clone(), full_time, incr_time,
            full_px, incr_px, w, h,
            render_calls: incr_ctx.render_calls, skipped: incr_ctx.skipped,
        }
    }).collect();

    SuiteResult { name: suite.name, description: suite.description, frames }
}

// ---------------------------------------------------------------------------
// HTML report
// ---------------------------------------------------------------------------

fn html_report(suites: &[SuiteResult]) -> String {
    let mut all_full = Duration::ZERO;
    let mut all_incr = Duration::ZERO;
    for s in suites {
        for (i,f) in s.frames.iter().enumerate() {
            if i>0 { all_full+=f.full_time; all_incr+=f.incr_time; }
        }
    }
    let overall_speedup = all_full.as_secs_f64() / all_incr.as_secs_f64().max(1e-9);

    let mut suites_html = String::new();
    for suite in suites {
        let mut s_full = Duration::ZERO;
        let mut s_incr = Duration::ZERO;
        let mut frames_html = String::new();

        for (i, f) in suite.frames.iter().enumerate() {
            if i>0 { s_full+=f.full_time; s_incr+=f.incr_time; }

            let full_uri = data_uri(&f.full_px, f.w, f.h);
            let (d, incr_uri, diff_uri) = if !f.incr_px.is_empty() && f.incr_px.len()==f.full_px.len() {
                let d = diff(&f.full_px, &f.incr_px, f.w, f.h);
                let du = data_uri(&d.img, f.w, f.h);
                let iu = data_uri(&f.incr_px, f.w, f.h);
                (Some(d), iu, du)
            } else { (None, String::new(), String::new()) };

            let perfect = d.as_ref().map(|d|d.weighted<0.5).unwrap_or(false);
            let badge = if perfect { r#"<span class="ok">✓ pixel-perfect</span>"# }
                        else { r#"<span class="diff">≠ differs</span>"# };
            let speedup_f = f.full_time.as_secs_f64()/f.incr_time.as_secs_f64().max(1e-9);
            let pw = (f.w*3).max(120);
            let ph = (f.h*3).max(36);

            frames_html.push_str(&format!(r#"
            <div class="frame {cls}">
              <div class="fhdr"><strong>Frame {i}</strong> — {lbl} {badge}
                <span class="tm">full {ft:.1}ms · incr {it:.1}ms · {sp:.1}× speedup</span>
                <span class="tm">{rc} renders · {sk} skipped</span>
              </div>
              <div class="imgs">
                <div><div class="cap">Full render</div><img src="{fu}" style="width:{pw}px;height:{ph}px;image-rendering:pixelated"></div>
                <div><div class="cap">Incremental</div><img src="{iu}" style="width:{pw}px;height:{ph}px;image-rendering:pixelated"></div>
                <div><div class="cap">Diff {ds}</div><img src="{du}" style="width:{pw}px;height:{ph}px;image-rendering:pixelated"></div>
              </div>
            </div>"#,
                cls=if perfect{"perfect"}else{"imperfect"}, i=i, lbl=f.label, badge=badge,
                ft=f.full_time.as_secs_f64()*1000.0, it=f.incr_time.as_secs_f64()*1000.0,
                sp=speedup_f, rc=f.render_calls, sk=f.skipped,
                fu=full_uri, iu=incr_uri, du=diff_uri, pw=pw, ph=ph,
                ds=d.as_ref().map(|d|format!("(weighted {:.1}/{} = {:.2}%, max ch Δ={})",d.weighted,d.total,d.weighted/d.total as f64*100.0,d.max_ch)).unwrap_or_default(),
            ));
        }

        let ss = s_full.as_secs_f64()/s_incr.as_secs_f64().max(1e-9);
        suites_html.push_str(&format!(r#"
        <section class="suite">
          <h2>{name} <span class="speedup">{ss:.1}× speedup (frames 1+)</span></h2>
          <p class="desc">{desc}</p>
          {frames}
        </section>"#, name=suite.name, ss=ss, desc=suite.description, frames=frames_html));
    }

    format!(r#"<!DOCTYPE html><html lang="en"><head><meta charset="utf-8">
<title>Partial Rendering PoC</title>
<style>
body{{font-family:system-ui,sans-serif;background:#0d0d0d;color:#ddd;padding:2rem;max-width:1200px;margin:0 auto}}
h1{{color:#fff;margin-bottom:0.3rem}} h2{{color:#eee;font-size:1.1rem;margin:1.5rem 0 0.2rem}}
.hero{{background:#1a1a2e;border:1px solid #333;border-radius:10px;padding:1rem 1.5rem;margin-bottom:2rem;display:flex;gap:3rem;align-items:center}}
.hero .v{{font-size:2rem;font-weight:bold;color:#4fc}}
.hero .l{{font-size:0.85rem;color:#888}}
.suite{{margin-bottom:2.5rem}}
.desc{{color:#888;font-size:0.82rem;margin:0 0 0.8rem}}
.speedup{{font-size:0.85rem;color:#4fc;font-weight:normal;margin-left:0.5rem}}
.frame{{background:#161616;border:1px solid #2a2a2a;border-radius:6px;padding:0.6rem 0.8rem;margin-bottom:0.6rem}}
.frame.perfect{{border-color:#1d3a1d}}.frame.imperfect{{border-color:#3a1d1d}}
.fhdr{{display:flex;align-items:center;gap:0.6rem;flex-wrap:wrap;margin-bottom:0.4rem;font-size:0.85rem}}
.tm{{color:#666;font-size:0.78rem}}
.imgs{{display:flex;gap:0.8rem;flex-wrap:wrap}}
.cap{{font-size:0.72rem;color:#666;margin-bottom:2px}}
img{{display:block;border:1px solid #333;border-radius:2px}}
.ok{{background:#1a3;color:#afa;padding:1px 6px;border-radius:3px;font-size:0.72rem;font-weight:bold}}
.diff{{background:#420;color:#faa;padding:1px 6px;border-radius:3px;font-size:0.72rem;font-weight:bold}}
</style></head><body>
<h1>Partial Rendering PoC — Multi-Suite Report</h1>
<div class="hero">
  <div><div class="l">Overall speedup (all suites, frames 1+)</div><div class="v">{sp:.1}×</div></div>
  <div><div class="l">Full recompute total</div><div class="v">{ft:.0}ms</div></div>
  <div><div class="l">Incremental total</div><div class="v">{it:.0}ms</div></div>
</div>
{suites}
</body></html>"#,
        sp=overall_speedup,
        ft=all_full.as_secs_f64()*1000.0,
        it=all_incr.as_secs_f64()*1000.0,
        suites=suites_html,
    )
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    eprintln!("Loading fonts into first context (used to verify font paths)...");
    // Warm up font file discovery once; each suite creates its own contexts.
    let _ = new_ctx_with_fonts();

    let suites_defs = vec![
        suite_simple_bar(),
        suite_shadow_cards(),
        suite_blurred_overlay(),
        suite_dense_metrics(),
    ];

    let mut results = Vec::new();
    for suite in &suites_defs {
        eprintln!("Running suite: {} ({} frames)...", suite.name, suite.frames.len());
        let result = run_suite(suite);

        // Console summary
        let mut s_full = Duration::ZERO;
        let mut s_incr = Duration::ZERO;
        for (i, f) in result.frames.iter().enumerate() {
            let d = if !f.incr_px.is_empty() && f.incr_px.len()==f.full_px.len() {
                let d = diff(&f.full_px, &f.incr_px, f.w, f.h);
                if d.weighted<0.5 { "✓".into() } else { format!("≠w={:.1}({:.2}%)",d.weighted,d.weighted/d.total as f64*100.0) }
            } else { "?".into() };
            println!("  [{: >2}] full={:.1}ms incr={:.1}ms ×{:.1} calls={} skip={} {}  {}",
                i, f.full_time.as_secs_f64()*1000.0, f.incr_time.as_secs_f64()*1000.0,
                f.full_time.as_secs_f64()/f.incr_time.as_secs_f64().max(1e-9),
                f.render_calls, f.skipped, d, f.label);
            if i>0 { s_full+=f.full_time; s_incr+=f.incr_time; }
        }
        println!("  → suite speedup: {:.1}×\n", s_full.as_secs_f64()/s_incr.as_secs_f64().max(1e-9));
        results.push(result);
    }

    let path = "/tmp/poc_report.html";
    std::fs::write(path, html_report(&results)).expect("write report");
    eprintln!("Report: file://{path}");
}
