use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use takumi::layout::{Viewport, node::Node};
use takumi::rendering::{RenderOptions, render as takumi_render, measure_layout as takumi_measure_layout};

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
    Collection { r: Rendered, tw: String, children: ManagedSet<FakeNode> },
}

impl FakeNodeState {
    fn rendered(&self) -> &Rendered {
        match self { Self::Text { r, .. } | Self::Image { r, .. } | Self::Collection { r, .. } => r }
    }
}

// ---------------------------------------------------------------------------
// Shared context — just metrics, GlobalContext accessed via with_global_ctx
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Ctx { render_calls: u32, stub_calls: u32 }

// ---------------------------------------------------------------------------
// Takumi helpers
// ---------------------------------------------------------------------------

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

/// Stub layout: given a container's tw and the sizes of its children, return
/// each child's (x, y) position using takumi for layout (no rasterization).
fn stub_positions(container_tw: &str, child_sizes: &[(u32, u32)]) -> Vec<(u32, u32)> {
    let stubs: Vec<serde_json::Value> = child_sizes.iter()
        .map(|(w, h)| serde_json::json!({
            "type": "container",
            "tw": format!("w-[{}px] h-[{}px] shrink-0", w, h)
        }))
        .collect();

    let scene = serde_json::json!({
        "type": "container",
        "tw": container_tw,
        "children": stubs
    });

    with_global_ctx(|global| {
        let node = parse_layout(&scene).unwrap_or_else(|_| Node::container(vec![]));
        let opts = RenderOptions::builder()
            .global(global)
            .viewport(Viewport::new((None, None)))
            .node(node)
            .build();
        let measured = takumi_measure_layout(opts).expect("measure_layout");
        measured.children.iter()
            .map(|c| (c.transform[4] as u32, c.transform[5] as u32))
            .collect()
    })
}

/// Blit src RGBA pixels into dst at (ox, oy). dst has stride dst_w * 4.
fn blit(dst: &mut [u8], dst_w: u32, dst_h: u32, src: &[u8], src_w: u32, src_h: u32, ox: u32, oy: u32) {
    for row in 0..src_h {
        for col in 0..src_w {
            let dx = ox + col;
            let dy = oy + row;
            if dx >= dst_w || dy >= dst_h { continue; }
            let si = ((row * src_w + col) * 4) as usize;
            let di = ((dy * dst_w + dx) * 4) as usize;
            if si + 4 > src.len() || di + 4 > dst.len() { continue; }
            let a = src[si + 3] as u32;
            if a == 0 { continue; }
            if a == 255 {
                dst[di..di + 4].copy_from_slice(&src[si..si + 4]);
            } else {
                for c in 0..3 {
                    dst[di + c] = ((src[si + c] as u32 * a + dst[di + c] as u32 * (255 - a)) / 255) as u8;
                }
                dst[di + 3] = (a + dst[di + 3] as u32 * (255 - a) / 255) as u8;
            }
        }
    }
}

/// Composite children at stub-computed positions into a single RGBA image.
fn composite(container_tw: &str, children_spec: &[FakeNode], child_set: &ManagedSet<FakeNode>, ctx: &mut Ctx) -> Rendered {
    let results: Vec<&Rendered> = children_spec.iter()
        .filter_map(|s| child_set.get(&s.id().to_string()))
        .map(|state| state.rendered())
        .collect();

    if results.is_empty() { return Rendered { pixels: Arc::new(vec![]), w: 0, h: 0 }; }

    let sizes: Vec<(u32, u32)> = results.iter().map(|r| (r.w, r.h)).collect();

    ctx.stub_calls += 1;
    let positions = stub_positions(container_tw, &sizes);

    let total_w = positions.iter().zip(&sizes).map(|((x, _), (w, _))| x + w).max().unwrap_or(0);
    let total_h = positions.iter().zip(&sizes).map(|((_, y), (_, h))| y + h).max().unwrap_or(0);

    let mut output = vec![0u8; (total_w * total_h * 4) as usize];
    for (r, (ox, oy)) in results.iter().zip(&positions) {
        blit(&mut output, total_w, total_h, &r.pixels, r.w, r.h, *ox, *oy);
    }

    Rendered { pixels: Arc::new(output), w: total_w, h: total_h }
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
                let json = serde_json::json!({"type": "text", "text": &content, "tw": &tw});
                let (pixels, w, h) = render_json(&json);
                Ok(FakeNodeState::Text { r: Rendered { pixels, w, h }, content })
            }

            FakeNode::Image { color, width, height, .. } => {
                ctx.render_calls += 1;
                let json = serde_json::json!({"type": "container",
                    "tw": format!("w-[{}px] h-[{}px] bg-{}", width, height, color)});
                let (pixels, _, _) = render_json(&json);
                Ok(FakeNodeState::Image { r: Rendered { pixels, w: width, h: height } })
            }

            FakeNode::Collection { tw, children, .. } => {
                let mut child_set: ManagedSet<FakeNode> = ManagedSet::new();
                child_set.reconcile(children.clone(), ctx, &mut ());
                let r = composite(&tw, &children, &child_set, ctx);
                Ok(FakeNodeState::Collection { r, tw, children: child_set })
            }
        }
    }

    fn reconcile_self(self, state: &mut FakeNodeState, ctx: &mut Ctx, _: &mut ()) -> Result<()> {
        match (self, state) {
            (FakeNode::Text { content, tw, .. }, FakeNodeState::Text { r, content: old }) => {
                if content != *old {
                    ctx.render_calls += 1;
                    let json = serde_json::json!({"type": "text", "text": &content, "tw": &tw});
                    let (pixels, w, h) = render_json(&json);
                    *r = Rendered { pixels, w, h };
                    *old = content;
                }
                Ok(())
            }

            (FakeNode::Image { color, width, height, .. }, FakeNodeState::Image { r }) => {
                if width != r.w || height != r.h {
                    ctx.render_calls += 1;
                    let json = serde_json::json!({"type": "container",
                        "tw": format!("w-[{}px] h-[{}px] bg-{}", width, height, color)});
                    let (pixels, _, _) = render_json(&json);
                    *r = Rendered { pixels, w: width, h: height };
                }
                Ok(())
            }

            (FakeNode::Collection { tw, children, .. }, FakeNodeState::Collection { r, children: child_set, tw: old_tw }) => {
                child_set.reconcile(children.clone(), ctx, &mut ());
                *r = composite(&tw, &children, child_set, ctx);
                *old_tw = tw;
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
                    FakeNode::Image  { id: "logo".into(),         color: "blue-500".into(),  width: 16, height: 16 },
                    FakeNode::Text   { id: "workspace".into(),    content: "1: term".into(), tw: "text-white text-xs whitespace-nowrap".into() },
                    FakeNode::Text   { id: "win-title".into(),    content: "nvim main.rs".into(), tw: "text-gray-400 text-xs whitespace-nowrap".into() },
                ],
            },
            FakeNode::Collection {
                id: "center".into(),
                tw: "flex flex-row items-center".into(),
                children: vec![
                    FakeNode::Text { id: "clock".into(), content: clock.into(), tw: "text-white text-xs font-mono whitespace-nowrap".into() },
                ],
            },
            FakeNode::Collection {
                id: "right".into(),
                tw: "flex flex-row items-center gap-1".into(),
                children: vec![
                    FakeNode::Text  { id: "cpu".into(),     content: cpu.into(),   tw: "text-white text-xs whitespace-nowrap".into() },
                    FakeNode::Text  { id: "mem".into(),     content: "MEM 4G".into(), tw: "text-white text-xs whitespace-nowrap".into() },
                    FakeNode::Image { id: "bat-icon".into(),color: "green-500".into(), width: 10, height: 10 },
                    FakeNode::Text  { id: "bat".into(),     content: "87%".into(), tw: "text-white text-xs whitespace-nowrap".into() },
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
            {"type": "container", "tw": "flex flex-row items-center gap-1", "children": [
                {"type": "container", "tw": "w-[16px] h-[16px] bg-blue-500"},
                {"type": "text", "text": "1: term",     "tw": "text-white text-xs whitespace-nowrap"},
                {"type": "text", "text": "nvim main.rs","tw": "text-gray-400 text-xs whitespace-nowrap"}
            ]},
            {"type": "container", "tw": "flex flex-row items-center", "children": [
                {"type": "text", "text": clock, "tw": "text-white text-xs font-mono whitespace-nowrap"}
            ]},
            {"type": "container", "tw": "flex flex-row items-center gap-1", "children": [
                {"type": "text", "text": cpu,         "tw": "text-white text-xs whitespace-nowrap"},
                {"type": "text", "text": "MEM 4G",    "tw": "text-white text-xs whitespace-nowrap"},
                {"type": "container", "tw": "w-[10px] h-[10px] bg-green-500"},
                {"type": "text", "text": "87%",       "tw": "text-white text-xs whitespace-nowrap"}
            ]}
        ]
    })
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

struct FrameStats { elapsed: Duration, render_calls: u32, stub_calls: u32 }

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
        FrameStats { elapsed: t.elapsed(), render_calls: 1, stub_calls: 0 }
    }).collect()
}

fn bench_incremental(frames: &[(String, String)]) -> Vec<FrameStats> {
    let mut ctx = Ctx::default();
    let mut set: ManagedSet<FakeNode> = ManagedSet::new();

    frames.iter().map(|(clock, cpu)| {
        let scene = make_scene(clock, cpu);
        ctx.render_calls = 0;
        ctx.stub_calls   = 0;

        let t = Instant::now();
        set.reconcile(scene, &mut ctx, &mut ());
        let elapsed = t.elapsed();

        FrameStats { elapsed, render_calls: ctx.render_calls, stub_calls: ctx.stub_calls }
    }).collect()
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

    println!("{:<14} {:>5}  {:>10}  {:>12}  {:>11}",
        "approach", "frame", "elapsed", "render_calls", "stub_calls");
    println!("{}", "-".repeat(60));

    let full  = bench_full(&frames);
    let incr  = bench_incremental(&frames);

    let mut full_total = Duration::ZERO;
    let mut incr_total = Duration::ZERO;
    let mut full_renders = 0u32;
    let mut incr_renders = 0u32;

    for (i, (f, inc)) in full.iter().zip(incr.iter()).enumerate() {
        let note = if i == 0 { " ← cold" }
                   else if inc.render_calls > 1 { " ← clock+cpu" }
                   else { " ← clock only" };

        println!("full          #{:<3}  {:>8.1}ms  {:>12}  {:>11}{}",
            i, f.elapsed.as_secs_f64() * 1000.0, f.render_calls, f.stub_calls, note);
        println!("incremental   #{:<3}  {:>8.1}ms  {:>12}  {:>11}",
            i, inc.elapsed.as_secs_f64() * 1000.0, inc.render_calls, inc.stub_calls);
        println!();

        if i > 0 {
            full_total   += f.elapsed;
            incr_total   += inc.elapsed;
            full_renders += f.render_calls;
            incr_renders += inc.render_calls;
        }
    }

    println!("{}", "=".repeat(60));
    println!("TOTALS  (frames 1-{}, excluding cold frame 0)", n - 1);
    println!("  full recompute :  {:>7.1}ms  ({} takumi render calls)", full_total.as_secs_f64() * 1000.0, full_renders);
    println!("  incremental    :  {:>7.1}ms  ({} takumi render calls + {} stub layouts)",
        incr_total.as_secs_f64() * 1000.0, incr_renders, incr.iter().skip(1).map(|f| f.stub_calls).sum::<u32>());
    if incr_total.as_secs_f64() > 0.0 {
        println!("  speedup        :  {:.1}×", full_total.as_secs_f64() / incr_total.as_secs_f64());
    }
}
