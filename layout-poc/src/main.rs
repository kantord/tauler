use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use image::{ImageBuffer, Rgba};
use takumi::layout::{Viewport, node::Node};
use takumi::rendering::{RenderOptions, render as takumi_render};
use takumi::resources::image::ImageSource;

use costae::managed_set::{Lifecycle, ManagedSet};
use costae::managed_set::reconcile::Reconcile;
use costae::layout::parse_layout;
use costae::render::{init_global_ctx, with_global_ctx};

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
        match self {
            Self::Text { id, .. } | Self::Image { id, .. } | Self::Collection { id, .. } => id,
        }
    }
}

impl std::fmt::Display for FakeNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.id())
    }
}

// ---------------------------------------------------------------------------
// Per-node render result
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Rendered { pixels: Arc<Vec<u8>>, w: u32, h: u32 }

enum FakeNodeState {
    Text       { r: Rendered, content: String },
    Image      { r: Rendered },
    Collection { r: Rendered, tw: String, children: ManagedSet<FakeNode>,
                 /// Arc pointers of children at last render — used to skip
                 /// re-renders when nothing changed.
                 last_child_arcs: Vec<Arc<Vec<u8>>> },
}

impl FakeNodeState {
    fn rendered(&self) -> &Rendered {
        match self { Self::Text { r, .. } | Self::Image { r, .. } | Self::Collection { r, .. } => r }
    }
}

// ---------------------------------------------------------------------------
// Context
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Ctx { render_calls: u32, skipped: u32 }

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Encode RGBA bytes as PNG — needed to load pixels into takumi's image store.
fn encode_png(pixels: &[u8], w: u32, h: u32) -> Vec<u8> {
    let img = ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(w, h, pixels.to_vec())
        .expect("invalid pixel buffer");
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png).expect("PNG encode");
    buf.into_inner()
}

/// Render any JSON node to RGBA. Returns pixels + dimensions.
fn render_json(json: &serde_json::Value) -> (Arc<Vec<u8>>, u32, u32) {
    with_global_ctx(|global| {
        let node = parse_layout(json).unwrap_or_else(|_| Node::container(vec![]));
        let opts = RenderOptions::builder()
            .global(global)
            .viewport(Viewport::new((None, None)))
            .node(node)
            .build();
        let img = takumi_render(opts).expect("render");
        let (w, h) = img.dimensions();
        (Arc::new(img.into_raw()), w, h)
    })
}

/// Render a collection by substituting each direct child with an image stub
/// (the child's cached pixels pre-loaded into takumi's persistent image store).
/// Takumi handles all layout, backgrounds, padding, gaps — no manual compositing.
fn render_with_stubs(
    tw: &str,
    children_spec: &[FakeNode],
    child_set: &ManagedSet<FakeNode>,
    ctx: &mut Ctx,
) -> Rendered {
    // Build stub JSON: each child becomes {"type":"image","src":"stub://<id>"}
    let stub_children: Vec<serde_json::Value> = children_spec.iter()
        .filter_map(|spec| child_set.get(&spec.id().to_string()))
        .zip(children_spec.iter())
        .map(|(state, spec)| {
            let r = state.rendered();
            serde_json::json!({
                "type": "image",
                "src": format!("stub://{}", spec.id()),
                "tw": format!("w-[{}px] h-[{}px] shrink-0", r.w, r.h)
            })
        })
        .collect();

    let scene = serde_json::json!({ "type": "container", "tw": tw, "children": stub_children });

    ctx.render_calls += 1;
    with_global_ctx(|global| {
        // Pre-load each child's pixels into the persistent image store so
        // takumi can blit them when it encounters the stub image nodes.
        for (spec, child_state) in children_spec.iter()
            .filter_map(|s| child_set.get(&s.id().to_string()).map(|st| (s, st)))
        {
            let r = child_state.rendered();
            let png = encode_png(&r.pixels, r.w, r.h);
            if let Ok(source) = ImageSource::from_bytes(&png) {
                global.persistent_image_store.insert(format!("stub://{}", spec.id()), source);
            }
        }

        let node = parse_layout(&scene).unwrap_or_else(|_| Node::container(vec![]));
        let opts = RenderOptions::builder()
            .global(global)
            .viewport(Viewport::new((None, None)))
            .node(node)
            .build();
        let img = takumi_render(opts).expect("render collection");
        let (w, h) = img.dimensions();
        Rendered { pixels: Arc::new(img.into_raw()), w, h }
    })
}

/// Collect child pixel Arcs in spec order (for dirty detection).
fn child_arcs(children_spec: &[FakeNode], child_set: &ManagedSet<FakeNode>) -> Vec<Arc<Vec<u8>>> {
    children_spec.iter()
        .filter_map(|s| child_set.get(&s.id().to_string()))
        .map(|st| Arc::clone(&st.rendered().pixels))
        .collect()
}

// ---------------------------------------------------------------------------
// Lifecycle
// ---------------------------------------------------------------------------

impl Lifecycle for FakeNode {
    type Key     = String;
    type State   = FakeNodeState;
    type Context = Ctx;
    type Output  = ();
    type Error   = anyhow::Error;

    fn key(&self) -> String { self.id().to_string() }

    fn enter(self, ctx: &mut Ctx, _: &mut ()) -> Result<FakeNodeState> {
        match self {
            FakeNode::Text { content, tw, .. } => {
                ctx.render_calls += 1;
                let json = serde_json::json!({"type":"text","text":&content,"tw":&tw});
                let (pixels, w, h) = render_json(&json);
                Ok(FakeNodeState::Text { r: Rendered { pixels, w, h }, content })
            }

            FakeNode::Image { color, width, height, .. } => {
                ctx.render_calls += 1;
                let json = serde_json::json!({"type":"container",
                    "tw": format!("w-[{}px] h-[{}px] bg-{}", width, height, color)});
                let (pixels, _, _) = render_json(&json);
                Ok(FakeNodeState::Image { r: Rendered { pixels, w: width, h: height } })
            }

            FakeNode::Collection { tw, children, .. } => {
                let mut child_set: ManagedSet<FakeNode> = ManagedSet::new();
                child_set.reconcile(children.clone(), ctx, &mut ());
                let arcs = child_arcs(&children, &child_set);
                let r = render_with_stubs(&tw, &children, &child_set, ctx);
                Ok(FakeNodeState::Collection { r, tw, children: child_set, last_child_arcs: arcs })
            }
        }
    }

    fn reconcile_self(self, state: &mut FakeNodeState, ctx: &mut Ctx, _: &mut ()) -> Result<()> {
        match (self, state) {
            (FakeNode::Text { content, tw, .. }, FakeNodeState::Text { r, content: old }) => {
                if content != *old {
                    ctx.render_calls += 1;
                    let json = serde_json::json!({"type":"text","text":&content,"tw":&tw});
                    let (pixels, w, h) = render_json(&json);
                    *r = Rendered { pixels, w, h };
                    *old = content;
                }
                Ok(())
            }

            (FakeNode::Image { color, width, height, .. }, FakeNodeState::Image { r }) => {
                if width != r.w || height != r.h {
                    ctx.render_calls += 1;
                    let json = serde_json::json!({"type":"container",
                        "tw": format!("w-[{}px] h-[{}px] bg-{}", width, height, color)});
                    let (pixels, _, _) = render_json(&json);
                    *r = Rendered { pixels, w: width, h: height };
                }
                Ok(())
            }

            (FakeNode::Collection { tw, children, .. },
             FakeNodeState::Collection { r, tw: old_tw, children: child_set, last_child_arcs }) =>
            {
                child_set.reconcile(children.clone(), ctx, &mut ());
                let new_arcs = child_arcs(&children, child_set);

                // Only re-render if at least one child's pixels changed.
                let dirty = new_arcs.len() != last_child_arcs.len()
                    || new_arcs.iter().zip(last_child_arcs.iter())
                               .any(|(a, b)| !Arc::ptr_eq(a, b));

                if dirty {
                    *r = render_with_stubs(&tw, &children, child_set, ctx);
                    *last_child_arcs = new_arcs;
                    *old_tw = tw;
                } else {
                    ctx.skipped += 1;
                }
                Ok(())
            }

            _ => Err(anyhow::anyhow!("node type mismatch")),
        }
    }

    fn exit(state: FakeNodeState, ctx: &mut Ctx, _: &mut ()) -> Result<()> {
        if let FakeNodeState::Collection { mut children, .. } = state {
            children.reconcile(vec![], ctx, &mut ());
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Scene
// ---------------------------------------------------------------------------

fn make_scene(clock: &str, cpu: &str) -> Vec<FakeNode> {
    vec![FakeNode::Collection {
        id: "bar".into(),
        tw: "flex flex-row items-center justify-between w-[400px] h-[24px] bg-gray-900".into(),
        children: vec![
            FakeNode::Collection {
                id: "left".into(),
                tw: "flex flex-row items-center gap-1".into(),
                children: vec![
                    FakeNode::Image { id: "logo".into(),      color: "blue-500".into(),  width: 16, height: 16 },
                    FakeNode::Text  { id: "workspace".into(), content: "1: term".into(), tw: "text-white text-xs whitespace-nowrap".into() },
                    FakeNode::Text  { id: "win-title".into(), content: "nvim main.rs".into(), tw: "text-gray-400 text-xs whitespace-nowrap".into() },
                ],
            },
            FakeNode::Collection {
                id: "center".into(),
                tw: "flex flex-row items-center".into(),
                children: vec![
                    FakeNode::Text { id: "clock".into(), content: clock.into(),
                        tw: "text-white text-xs font-mono whitespace-nowrap".into() },
                ],
            },
            FakeNode::Collection {
                id: "right".into(),
                tw: "flex flex-row items-center gap-1".into(),
                children: vec![
                    FakeNode::Text  { id: "cpu".into(),      content: cpu.into(),      tw: "text-white text-xs whitespace-nowrap".into() },
                    FakeNode::Text  { id: "mem".into(),      content: "MEM 4G".into(), tw: "text-white text-xs whitespace-nowrap".into() },
                    FakeNode::Image { id: "bat-icon".into(), color: "green-500".into(), width: 10, height: 10 },
                    FakeNode::Text  { id: "bat".into(),      content: "87%".into(),    tw: "text-white text-xs whitespace-nowrap".into() },
                ],
            },
        ],
    }]
}

fn make_full_scene_json(clock: &str, cpu: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "container",
        "tw": "flex flex-row items-center justify-between w-[400px] h-[24px] bg-gray-900",
        "children": [
            {"type":"container","tw":"flex flex-row items-center gap-1","children":[
                {"type":"container","tw":"w-[16px] h-[16px] bg-blue-500"},
                {"type":"text","text":"1: term",     "tw":"text-white text-xs whitespace-nowrap"},
                {"type":"text","text":"nvim main.rs","tw":"text-gray-400 text-xs whitespace-nowrap"}
            ]},
            {"type":"container","tw":"flex flex-row items-center","children":[
                {"type":"text","text":clock,"tw":"text-white text-xs font-mono whitespace-nowrap"}
            ]},
            {"type":"container","tw":"flex flex-row items-center gap-1","children":[
                {"type":"text","text":cpu,         "tw":"text-white text-xs whitespace-nowrap"},
                {"type":"text","text":"MEM 4G",    "tw":"text-white text-xs whitespace-nowrap"},
                {"type":"container","tw":"w-[10px] h-[10px] bg-green-500"},
                {"type":"text","text":"87%",       "tw":"text-white text-xs whitespace-nowrap"}
            ]}
        ]
    })
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

struct FrameStats { elapsed: Duration, render_calls: u32, skipped: u32 }

fn bench_full(frames: &[(String, String)]) -> Vec<FrameStats> {
    frames.iter().map(|(clock, cpu)| {
        let json = make_full_scene_json(clock, cpu);
        let t = Instant::now();
        with_global_ctx(|global| {
            let node = parse_layout(&json).unwrap_or_else(|_| Node::container(vec![]));
            let opts = RenderOptions::builder()
                .global(global)
                .viewport(Viewport::new((Some(400), Some(24))))
                .node(node)
                .build();
            takumi_render(opts).expect("full render");
        });
        FrameStats { elapsed: t.elapsed(), render_calls: 1, skipped: 0 }
    }).collect()
}

fn bench_incremental(frames: &[(String, String)]) -> Vec<FrameStats> {
    let mut ctx = Ctx::default();
    let mut set: ManagedSet<FakeNode> = ManagedSet::new();

    frames.iter().map(|(clock, cpu)| {
        let scene = make_scene(clock, cpu);
        ctx.render_calls = 0;
        ctx.skipped      = 0;
        let t = Instant::now();
        set.reconcile(scene, &mut ctx, &mut ());
        FrameStats { elapsed: t.elapsed(), render_calls: ctx.render_calls, skipped: ctx.skipped }
    }).collect()
}

// ---------------------------------------------------------------------------
// Save a frame's output for visual inspection
// ---------------------------------------------------------------------------

fn save_frame(pixels: &[u8], w: u32, h: u32, path: &str) {
    let img = ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(w, h, pixels.to_vec())
        .expect("invalid buffer");
    img.save(path).expect("save PNG");
    eprintln!("  saved {path}  ({w}×{h})");
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    eprintln!("Initialising takumi (loading fonts)...");
    init_global_ctx();

    let n = 10;
    let frames: Vec<(String, String)> = (0..n).map(|i| (
        format!("{:02}:{:02}:{:02}", 12, i / 60, i % 60),
        format!("CPU {}%", if i % 3 == 0 { i * 4 } else { 12 }),
    )).collect();

    eprintln!("Running {} frames...\n", n);

    println!("{:<14} {:>5}  {:>10}  {:>12}  {:>9}",
        "approach", "frame", "elapsed", "render_calls", "skipped");
    println!("{}", "-".repeat(58));

    let full = bench_full(&frames);
    let incr = bench_incremental(&frames);

    let mut full_total = Duration::ZERO;
    let mut incr_total = Duration::ZERO;

    for (i, (f, inc)) in full.iter().zip(incr.iter()).enumerate() {
        let note = if i == 0 { " ← cold" }
                   else if inc.render_calls > 4 { " ← clock+cpu" }
                   else { " ← clock only" };

        println!("full          #{:<3}  {:>8.1}ms  {:>12}  {:>9}{}",
            i, f.elapsed.as_secs_f64()*1000.0, f.render_calls, f.skipped, note);
        println!("incremental   #{:<3}  {:>8.1}ms  {:>12}  {:>9}",
            i, inc.elapsed.as_secs_f64()*1000.0, inc.render_calls, inc.skipped);
        println!();

        if i > 0 { full_total += f.elapsed; incr_total += inc.elapsed; }
    }

    println!("{}", "=".repeat(58));
    println!("TOTALS (frames 1-{}):", n - 1);
    println!("  full recompute :  {:>7.1}ms", full_total.as_secs_f64()*1000.0);
    println!("  incremental    :  {:>7.1}ms", incr_total.as_secs_f64()*1000.0);
    if incr_total.as_secs_f64() > 0.0 {
        println!("  speedup        :  {:.1}×", full_total.as_secs_f64()/incr_total.as_secs_f64());
    }

    // ------------------------------------------------------------------
    // Save frame 0 (cold) and frame 5 for visual comparison
    // ------------------------------------------------------------------
    eprintln!("\nSaving reference images...");

    // Full render — frame 0
    let json0 = make_full_scene_json(&frames[0].0, &frames[0].1);
    let (pixels0, w0, h0) = with_global_ctx(|global| {
        let node = parse_layout(&json0).unwrap_or_else(|_| Node::container(vec![]));
        let opts = RenderOptions::builder().global(global)
            .viewport(Viewport::new((Some(400), Some(24)))).node(node).build();
        let img = takumi_render(opts).expect("render");
        let (w, h) = img.dimensions();
        (img.into_raw(), w, h)
    });
    save_frame(&pixels0, w0, h0, "/tmp/poc_full_frame0.png");

    // Incremental — rebuild from frame 0 and save root pixels
    let mut ctx2 = Ctx::default();
    let mut set2: ManagedSet<FakeNode> = ManagedSet::new();
    set2.reconcile(make_scene(&frames[0].0, &frames[0].1), &mut ctx2, &mut ());
    if let Some(root) = set2.get(&"bar".to_string()) {
        let r = root.rendered();
        save_frame(&r.pixels, r.w, r.h, "/tmp/poc_incremental_frame0.png");
    }

    // Incremental — advance to frame 5
    for i in 1..=5 {
        set2.reconcile(make_scene(&frames[i].0, &frames[i].1), &mut ctx2, &mut ());
    }
    if let Some(root) = set2.get(&"bar".to_string()) {
        let r = root.rendered();
        save_frame(&r.pixels, r.w, r.h, "/tmp/poc_incremental_frame5.png");
    }

    let json5 = make_full_scene_json(&frames[5].0, &frames[5].1);
    let (pixels5, w5, h5) = with_global_ctx(|global| {
        let node = parse_layout(&json5).unwrap_or_else(|_| Node::container(vec![]));
        let opts = RenderOptions::builder().global(global)
            .viewport(Viewport::new((Some(400), Some(24)))).node(node).build();
        let img = takumi_render(opts).expect("render");
        let (w, h) = img.dimensions();
        (img.into_raw(), w, h)
    });
    save_frame(&pixels5, w5, h5, "/tmp/poc_full_frame5.png");

    eprintln!("\nCompare:");
    eprintln!("  full frame 0:        /tmp/poc_full_frame0.png");
    eprintln!("  incremental frame 0: /tmp/poc_incremental_frame0.png");
    eprintln!("  full frame 5:        /tmp/poc_full_frame5.png");
    eprintln!("  incremental frame 5: /tmp/poc_incremental_frame5.png");
}
