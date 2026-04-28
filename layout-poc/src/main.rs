use std::time::{Duration, Instant};
use std::thread;

use anyhow::Result;
use rand::Rng;
use taffy::prelude::*;

use costae::managed_set::{Lifecycle, ManagedSet};
use costae::managed_set::reconcile::Reconcile;

// ---------------------------------------------------------------------------
// Scene description — pure data, the "desired state" for each frame
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum FakeNode {
    Text       { id: String, content: String },
    Image      { id: String, width: u32, height: u32 },
    Collection { id: String, children: Vec<FakeNode> },
}

impl FakeNode {
    fn id(&self) -> &str {
        match self { Self::Text { id, .. } | Self::Image { id, .. } | Self::Collection { id, .. } => id }
    }
}

impl std::fmt::Display for FakeNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.id())
    }
}

// ---------------------------------------------------------------------------
// Persistent state — kept alive between frames
// ---------------------------------------------------------------------------

struct TextState       { taffy_id: NodeId, content: String }
struct ImageState      { taffy_id: NodeId, width: u32, height: u32 }
struct CollectionState { taffy_id: NodeId, children: ManagedSet<FakeNode> }

enum FakeNodeState {
    Text(TextState),
    Image(ImageState),
    Collection(CollectionState),
}

// ---------------------------------------------------------------------------
// Shared context — the taffy tree plus metrics, threaded through reconciliation
// ---------------------------------------------------------------------------

struct TaffyCtx {
    tree:        TaffyTree<()>,
    parent_id:   Option<NodeId>,
    root_id:     Option<NodeId>,
    shape_calls: u32,
    shape_time:  Duration,
}

impl TaffyCtx {
    fn new() -> Self {
        Self {
            tree:        TaffyTree::new(),
            parent_id:   None,
            root_id:     None,
            shape_calls: 0,
            shape_time:  Duration::ZERO,
        }
    }

    /// Simulates text shaping: sleeps for a random duration, records the cost.
    fn simulate_shaping(&mut self) {
        let ms = rand::thread_rng().gen_range(2u64..8);
        let t = Instant::now();
        thread::sleep(Duration::from_millis(ms));
        self.shape_calls += 1;
        self.shape_time += t.elapsed();
    }

    fn attach_to_parent(&mut self, child: NodeId) {
        if let Some(parent) = self.parent_id {
            let _ = self.tree.add_child(parent, child);
        }
    }
}

// ---------------------------------------------------------------------------
// Lifecycle — Collection is the only supervisor; Text and Image are leaves
// ---------------------------------------------------------------------------

impl Lifecycle for FakeNode {
    type Key     = String;
    type State   = FakeNodeState;
    type Context = TaffyCtx;
    type Output  = ();
    type Error   = anyhow::Error;

    fn key(&self) -> String { self.id().to_string() }

    fn enter(self, ctx: &mut TaffyCtx, _: &mut ()) -> Result<FakeNodeState> {
        match self {
            FakeNode::Text { content, .. } => {
                ctx.simulate_shaping();
                let taffy_id = ctx.tree.new_leaf(Style {
                    size: Size { width: length(200.0), height: length(20.0) },
                    ..Default::default()
                })?;
                ctx.attach_to_parent(taffy_id);
                Ok(FakeNodeState::Text(TextState { taffy_id, content }))
            }

            FakeNode::Image { width, height, .. } => {
                let taffy_id = ctx.tree.new_leaf(Style {
                    size: Size { width: length(width as f32), height: length(height as f32) },
                    ..Default::default()
                })?;
                ctx.attach_to_parent(taffy_id);
                Ok(FakeNodeState::Image(ImageState { taffy_id, width, height }))
            }

            FakeNode::Collection { children, .. } => {
                let taffy_id = ctx.tree.new_leaf(Style {
                    display: Display::Flex,
                    flex_direction: FlexDirection::Row,
                    size: Size { width: length(1920.0), height: length(40.0) },
                    ..Default::default()
                })?;
                ctx.attach_to_parent(taffy_id);

                // This collection is the root if it has no parent.
                if ctx.parent_id.is_none() {
                    ctx.root_id = Some(taffy_id);
                }

                // Recurse: reconcile children with this collection as their parent.
                let prev_parent = ctx.parent_id.replace(taffy_id);
                let mut child_set: ManagedSet<FakeNode> = ManagedSet::new();
                child_set.reconcile(children, ctx, &mut ());
                ctx.parent_id = prev_parent;

                Ok(FakeNodeState::Collection(CollectionState { taffy_id, children: child_set }))
            }
        }
    }

    fn reconcile_self(self, state: &mut FakeNodeState, ctx: &mut TaffyCtx, _: &mut ()) -> Result<()> {
        match (self, state) {
            (FakeNode::Text { content, .. }, FakeNodeState::Text(s)) => {
                if content != s.content {
                    // Content changed — re-shape and mark taffy node dirty.
                    ctx.simulate_shaping();
                    ctx.tree.mark_dirty(s.taffy_id)?;
                    s.content = content;
                }
                // Unchanged text: no shaping, no mark_dirty — taffy skips this node.
                Ok(())
            }

            (FakeNode::Image { width, height, .. }, FakeNodeState::Image(s)) => {
                if width != s.width || height != s.height {
                    ctx.tree.set_style(s.taffy_id, Style {
                        size: Size { width: length(width as f32), height: length(height as f32) },
                        ..Default::default()
                    })?;
                    ctx.tree.mark_dirty(s.taffy_id)?;
                    s.width = width;
                    s.height = height;
                }
                Ok(())
            }

            (FakeNode::Collection { children, .. }, FakeNodeState::Collection(s)) => {
                // Supervisor: drive child reconciliation, then let taffy propagate
                // dirty state up from any changed children.
                let prev_parent = ctx.parent_id.replace(s.taffy_id);
                s.children.reconcile(children, ctx, &mut ());
                ctx.parent_id = prev_parent;
                Ok(())
            }

            _ => Err(anyhow::anyhow!("node type mismatch in reconcile_self")),
        }
    }

    fn exit(state: FakeNodeState, ctx: &mut TaffyCtx, _: &mut ()) -> Result<()> {
        match state {
            FakeNodeState::Text(s)  => { ctx.tree.remove(s.taffy_id)?; }
            FakeNodeState::Image(s) => { ctx.tree.remove(s.taffy_id)?; }
            FakeNodeState::Collection(mut s) => {
                // Exit children before removing the container.
                s.children.reconcile(vec![], ctx, &mut ());
                ctx.tree.remove(s.taffy_id)?;
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Scene builder — a realistic status-bar layout
// ---------------------------------------------------------------------------

fn make_scene(clock: &str, cpu: &str) -> Vec<FakeNode> {
    vec![FakeNode::Collection {
        id: "bar".into(),
        children: vec![
            FakeNode::Collection {
                id: "left".into(),
                children: vec![
                    FakeNode::Image { id: "logo".into(),         width: 24, height: 24 },
                    FakeNode::Text  { id: "workspace".into(),    content: "1: term".into() },
                    FakeNode::Text  { id: "window-title".into(), content: "nvim ~/main.rs".into() },
                ],
            },
            FakeNode::Collection {
                id: "center".into(),
                children: vec![
                    FakeNode::Text { id: "clock".into(), content: clock.into() },
                ],
            },
            FakeNode::Collection {
                id: "right".into(),
                children: vec![
                    FakeNode::Text  { id: "cpu".into(),          content: cpu.into() },
                    FakeNode::Text  { id: "mem".into(),          content: "MEM 4.2G".into() },
                    FakeNode::Text  { id: "net".into(),          content: "↑ 1.2MB/s".into() },
                    FakeNode::Image { id: "battery-icon".into(), width: 16, height: 16 },
                    FakeNode::Text  { id: "battery".into(),      content: "87%".into() },
                    FakeNode::Text  { id: "volume".into(),       content: "VOL 70%".into() },
                    FakeNode::Image { id: "tray-1".into(),       width: 16, height: 16 },
                    FakeNode::Image { id: "tray-2".into(),       width: 16, height: 16 },
                ],
            },
        ],
    }]
}

// ---------------------------------------------------------------------------
// Benchmark runs
// ---------------------------------------------------------------------------

struct FrameResult {
    elapsed:     Duration,
    shape_calls: u32,
    shape_time:  Duration,
}

/// Full recompute: fresh TaffyTree and ManagedSet every frame.
/// Every node is entered → every Text node triggers simulate_shaping().
fn run_full_recompute(frames: &[(String, String)]) -> Vec<FrameResult> {
    frames.iter().map(|(clock, cpu)| {
        let scene = make_scene(clock, cpu);
        let mut ctx = TaffyCtx::new();
        let mut set: ManagedSet<FakeNode> = ManagedSet::new();

        let t = Instant::now();
        set.reconcile(scene, &mut ctx, &mut ());
        let root = ctx.root_id.expect("root node must be set after reconcile");
        ctx.tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
        let elapsed = t.elapsed();

        FrameResult { elapsed, shape_calls: ctx.shape_calls, shape_time: ctx.shape_time }
    }).collect()
}

/// Incremental: one TaffyTree and ManagedSet, kept alive across all frames.
/// Only nodes whose content changed call simulate_shaping() and mark_dirty().
fn run_incremental(frames: &[(String, String)]) -> Vec<FrameResult> {
    let mut ctx = TaffyCtx::new();
    let mut set: ManagedSet<FakeNode> = ManagedSet::new();

    frames.iter().map(|(clock, cpu)| {
        let scene = make_scene(clock, cpu);
        ctx.shape_calls = 0;
        ctx.shape_time  = Duration::ZERO;

        let t = Instant::now();
        set.reconcile(scene, &mut ctx, &mut ());
        let root = ctx.root_id.expect("root node must be set after reconcile");
        ctx.tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
        let elapsed = t.elapsed();

        FrameResult { elapsed, shape_calls: ctx.shape_calls, shape_time: ctx.shape_time }
    }).collect()
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    let n_frames = 20;

    // Frame 0: initial render (same for both approaches).
    // Frames 1-N: clock ticks every frame, CPU updates every 5 frames.
    let frames: Vec<(String, String)> = (0..n_frames).map(|i| {
        let clock = format!("{:02}:{:02}:{:02}", 12, i / 60, i % 60);
        let cpu   = format!("CPU {}%", if i % 5 == 0 { i * 3 } else { 12 });
        (clock, cpu)
    }).collect();

    println!("Running {} frames ({} nodes in scene)\n", n_frames, 13);
    println!("{:<12} {:>10} {:>12} {:>12}   {}",
        "approach", "frame", "shape_calls", "shape_time", "notes");
    println!("{}", "-".repeat(70));

    let full   = run_full_recompute(&frames);
    let incremental = run_incremental(&frames);

    let mut full_total      = Duration::ZERO;
    let mut inc_total       = Duration::ZERO;
    let mut full_shapes     = 0u32;
    let mut inc_shapes      = 0u32;

    for (i, (f, inc)) in full.iter().zip(incremental.iter()).enumerate() {
        let note = if i == 0 { "← cold (both identical)" }
                   else if i % 5 == 0 { "← clock + cpu changed" }
                   else { "← clock only changed" };

        println!("full      #{:<3} {:>8.1}ms {:>12} {:>10.1}ms   {}",
            i, f.elapsed.as_secs_f64() * 1000.0, f.shape_calls,
            f.shape_time.as_secs_f64() * 1000.0, note);
        println!("incremental #{:<3} {:>8.1}ms {:>12} {:>10.1}ms",
            i, inc.elapsed.as_secs_f64() * 1000.0, inc.shape_calls,
            inc.shape_time.as_secs_f64() * 1000.0);
        println!();

        if i > 0 {
            full_total  += f.elapsed;
            inc_total   += inc.elapsed;
            full_shapes += f.shape_calls;
            inc_shapes  += inc.shape_calls;
        }
    }

    println!("{}", "=".repeat(70));
    println!("TOTALS (frames 1-{}, excluding cold frame 0)", n_frames - 1);
    println!("  full recompute : {:>8.1}ms  ({} shape calls)",
        full_total.as_secs_f64() * 1000.0, full_shapes);
    println!("  incremental    : {:>8.1}ms  ({} shape calls)",
        inc_total.as_secs_f64() * 1000.0, inc_shapes);
    println!("  speedup        : {:.1}×",
        full_total.as_secs_f64() / inc_total.as_secs_f64());
    println!("  shape calls    : {:.1}× fewer",
        full_shapes as f64 / inc_shapes as f64);
}
