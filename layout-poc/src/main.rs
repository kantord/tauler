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
// Image helpers
// ---------------------------------------------------------------------------

fn encode_png(pixels: &[u8], w: u32, h: u32) -> Vec<u8> {
    let img = ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(w, h, pixels.to_vec())
        .expect("invalid pixel buffer");
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png).expect("PNG encode");
    buf.into_inner()
}

fn base64_encode(data: &[u8]) -> String {
    const C: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(C[((n >> 18) & 63) as usize] as char);
        out.push(C[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 { C[((n >> 6) & 63) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { C[(n & 63) as usize] as char } else { '=' });
    }
    out
}

fn pixels_to_data_uri(pixels: &[u8], w: u32, h: u32) -> String {
    format!("data:image/png;base64,{}", base64_encode(&encode_png(pixels, w, h)))
}

// ---------------------------------------------------------------------------
// Pixel diff
// ---------------------------------------------------------------------------

struct DiffResult {
    different_pixels: u32,
    total_pixels: u32,
    mean_abs_diff: f64,
    max_channel_diff: u8,
    diff_image: Vec<u8>,   // RGBA, same dims as input
}

fn pixel_diff(a: &[u8], b: &[u8], w: u32, h: u32) -> DiffResult {
    assert_eq!(a.len(), b.len());
    let mut different = 0u32;
    let mut total_diff = 0u64;
    let mut max_diff = 0u8;
    let mut diff_img = vec![0u8; a.len()];

    for i in (0..a.len()).step_by(4) {
        let dr = (a[i]   as i32 - b[i]   as i32).unsigned_abs() as u8;
        let dg = (a[i+1] as i32 - b[i+1] as i32).unsigned_abs() as u8;
        let db = (a[i+2] as i32 - b[i+2] as i32).unsigned_abs() as u8;
        let m  = dr.max(dg).max(db);
        max_diff = max_diff.max(m);

        if m > 1 {
            different += 1;
            total_diff += (dr as u64 + dg as u64 + db as u64);
            // Highlight differing pixels in bright magenta
            diff_img[i]   = 255;
            diff_img[i+1] = 0;
            diff_img[i+2] = 255;
            diff_img[i+3] = 255;
        } else {
            // Show matching pixels as a dim version of the full render
            diff_img[i]   = a[i]   / 5;
            diff_img[i+1] = a[i+1] / 5;
            diff_img[i+2] = a[i+2] / 5;
            diff_img[i+3] = 255;
        }
    }

    let n = (w * h) as f64;
    DiffResult {
        different_pixels: different,
        total_pixels: w * h,
        mean_abs_diff: if n > 0.0 { total_diff as f64 / (n * 3.0) } else { 0.0 },
        max_channel_diff: max_diff,
        diff_image: diff_img,
    }
}

// ---------------------------------------------------------------------------
// Takumi helpers
// ---------------------------------------------------------------------------

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

fn render_with_stubs(
    tw: &str,
    children_spec: &[FakeNode],
    child_set: &ManagedSet<FakeNode>,
    ctx: &mut Ctx,
) -> Rendered {
    let stub_children: Vec<serde_json::Value> = children_spec.iter()
        .filter_map(|spec| child_set.get(&spec.id().to_string()).map(|st| (spec, st)))
        .map(|(spec, state)| {
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
        for spec in children_spec.iter() {
            if let Some(state) = child_set.get(&spec.id().to_string()) {
                let r = state.rendered();
                let png = encode_png(&r.pixels, r.w, r.h);
                if let Ok(source) = ImageSource::from_bytes(&png) {
                    global.persistent_image_store.insert(format!("stub://{}", spec.id()), source);
                }
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
                let dirty = new_arcs.len() != last_child_arcs.len()
                    || new_arcs.iter().zip(last_child_arcs.iter()).any(|(a, b)| !Arc::ptr_eq(a, b));
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
                    FakeNode::Text  { id: "cpu".into(),      content: cpu.into(),       tw: "text-white text-xs whitespace-nowrap".into() },
                    FakeNode::Text  { id: "mem".into(),      content: "MEM 4G".into(),  tw: "text-white text-xs whitespace-nowrap".into() },
                    FakeNode::Image { id: "bat-icon".into(), color: "green-500".into(), width: 10, height: 10 },
                    FakeNode::Text  { id: "bat".into(),      content: "87%".into(),     tw: "text-white text-xs whitespace-nowrap".into() },
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
// Benchmark — runs full and incremental per-frame, collects pixels
// ---------------------------------------------------------------------------

struct FrameResult {
    idx:          usize,
    clock:        String,
    cpu:          String,
    full_time:    Duration,
    incr_time:    Duration,
    full_pixels:  Vec<u8>,
    incr_pixels:  Vec<u8>,
    w:            u32,
    h:            u32,
    render_calls: u32,
    skipped:      u32,
}

fn run_all(frames: &[(String, String)]) -> Vec<FrameResult> {
    let mut ctx = Ctx::default();
    let mut set: ManagedSet<FakeNode> = ManagedSet::new();

    frames.iter().enumerate().map(|(idx, (clock, cpu))| {
        // Full render
        let json = make_full_scene_json(clock, cpu);
        let t = Instant::now();
        let (full_pixels, w, h) = with_global_ctx(|global| {
            let node = parse_layout(&json).unwrap_or_else(|_| Node::container(vec![]));
            let opts = RenderOptions::builder().global(global)
                .viewport(Viewport::new((Some(400), Some(24)))).node(node).build();
            let img = takumi_render(opts).expect("full render");
            let (w, h) = img.dimensions();
            (img.into_raw(), w, h)
        });
        let full_time = t.elapsed();

        // Incremental
        ctx.render_calls = 0;
        ctx.skipped      = 0;
        let t = Instant::now();
        set.reconcile(make_scene(clock, cpu), &mut ctx, &mut ());
        let incr_time = t.elapsed();

        let incr_pixels = set.get(&"bar".to_string())
            .map(|s| (*s.rendered().pixels).clone())
            .unwrap_or_default();

        FrameResult {
            idx, clock: clock.clone(), cpu: cpu.clone(),
            full_time, incr_time,
            full_pixels, incr_pixels,
            w, h,
            render_calls: ctx.render_calls,
            skipped: ctx.skipped,
        }
    }).collect()
}

// ---------------------------------------------------------------------------
// HTML report
// ---------------------------------------------------------------------------

fn generate_report(results: &[FrameResult]) -> String {
    let full_total: Duration  = results.iter().skip(1).map(|r| r.full_time).sum();
    let incr_total: Duration  = results.iter().skip(1).map(|r| r.incr_time).sum();
    let speedup = full_total.as_secs_f64() / incr_total.as_secs_f64().max(1e-9);

    let mut frames_html = String::new();
    for r in results {
        let (fw, fh) = (r.w, r.h);
        let iw = if r.incr_pixels.is_empty() { 0 } else { r.w };
        let ih = if r.incr_pixels.is_empty() { 0 } else { r.h };

        let full_uri = pixels_to_data_uri(&r.full_pixels, fw, fh);

        let (diff, diff_uri, incr_uri) = if !r.incr_pixels.is_empty() && iw == fw && ih == fh {
            let d = pixel_diff(&r.full_pixels, &r.incr_pixels, fw, fh);
            let diff_uri = pixels_to_data_uri(&d.diff_image, fw, fh);
            let incr_uri = pixels_to_data_uri(&r.incr_pixels, iw, ih);
            (Some(d), diff_uri, incr_uri)
        } else {
            (None, String::new(), String::new())
        };

        let perfect = diff.as_ref().map(|d| d.different_pixels == 0).unwrap_or(false);
        let badge = if perfect {
            r#"<span class="badge ok">✓ pixel-perfect</span>"#
        } else {
            r#"<span class="badge diff">≠ differences</span>"#
        };

        let cold_tag = if r.idx == 0 { " <em>(cold)</em>" } else { "" };
        let stats_row = if let Some(d) = &diff {
            format!(
                "<tr><td>Different pixels</td><td>{} / {} ({:.2}%)</td></tr>\
                 <tr><td>Mean channel diff</td><td>{:.2}</td></tr>\
                 <tr><td>Max channel diff</td><td>{}</td></tr>",
                d.different_pixels, d.total_pixels,
                d.different_pixels as f64 / d.total_pixels as f64 * 100.0,
                d.mean_abs_diff, d.max_channel_diff
            )
        } else {
            "<tr><td colspan='2'>dimension mismatch</td></tr>".into()
        };

        let diff_col = if !diff_uri.is_empty() {
            format!(r#"<div class="img-box"><h3>Diff (magenta = differs)</h3>
                <img src="{diff_uri}" style="width:{pw}px;height:{ph}px;image-rendering:pixelated">
            </div>"#,
            pw = fw * 3, ph = fh * 3)
        } else { String::new() };

        frames_html.push_str(&format!(r#"
        <section class="frame {cls}">
          <h2>Frame {idx} — {clock} · {cpu}{cold}{badge}</h2>
          <div class="images">
            <div class="img-box">
              <h3>Full render <small>{ft:.1}ms</small></h3>
              <img src="{full_uri}" style="width:{pw}px;height:{ph}px;image-rendering:pixelated">
            </div>
            <div class="img-box">
              <h3>Incremental <small>{it:.1}ms · {rc} calls · {sk} skipped</small></h3>
              <img src="{incr_uri}" style="width:{pw}px;height:{ph}px;image-rendering:pixelated">
            </div>
            {diff_col}
          </div>
          <table class="stats">
            <tr><td>Full time</td><td>{ft:.2}ms</td></tr>
            <tr><td>Incremental time</td><td>{it:.2}ms</td></tr>
            <tr><td>Speedup (this frame)</td><td>{fs:.1}×</td></tr>
            {stats_row}
          </table>
        </section>"#,
            cls   = if perfect { "perfect" } else { "imperfect" },
            idx   = r.idx,
            clock = r.clock,
            cpu   = r.cpu,
            cold  = cold_tag,
            badge = badge,
            ft    = r.full_time.as_secs_f64() * 1000.0,
            it    = r.incr_time.as_secs_f64() * 1000.0,
            rc    = r.render_calls,
            sk    = r.skipped,
            fs    = r.full_time.as_secs_f64() / r.incr_time.as_secs_f64().max(1e-9),
            pw    = fw * 3,
            ph    = fh * 3,
            full_uri = full_uri,
            incr_uri = incr_uri,
            diff_col = diff_col,
            stats_row = stats_row,
        ));
    }

    format!(r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Partial Rendering PoC</title>
<style>
  body {{ font-family: system-ui, sans-serif; background: #111; color: #eee; padding: 2rem; }}
  h1   {{ color: #fff; }}
  h2   {{ font-size: 1rem; margin: 0 0 0.5rem; }}
  h3   {{ font-size: 0.8rem; color: #aaa; margin: 0 0 0.3rem; font-weight: normal; }}
  small{{ color: #888; }}
  .summary {{ border-collapse: collapse; margin-bottom: 2rem; font-size: 1.1rem; }}
  .summary td {{ padding: 0.3rem 1.5rem 0.3rem 0; }}
  .summary .v {{ color: #4fc; font-weight: bold; }}
  .frame   {{ background: #1a1a1a; border: 1px solid #333; border-radius: 8px;
               padding: 1rem; margin-bottom: 1.5rem; }}
  .frame.perfect   {{ border-color: #2a4; }}
  .frame.imperfect {{ border-color: #a42; }}
  .images  {{ display: flex; gap: 1.5rem; flex-wrap: wrap; margin-bottom: 0.75rem; }}
  .img-box img {{ display: block; border: 1px solid #444; border-radius: 3px; }}
  .stats   {{ border-collapse: collapse; font-size: 0.8rem; color: #ccc; }}
  .stats td {{ padding: 0.1rem 1.2rem 0.1rem 0; }}
  .badge   {{ display: inline-block; padding: 0.1rem 0.5rem; border-radius: 4px;
               font-size: 0.75rem; font-weight: bold; margin-left: 0.5rem; }}
  .badge.ok   {{ background: #1a3; color: #afa; }}
  .badge.diff {{ background: #420; color: #faa; }}
  em {{ color: #888; font-size: 0.85rem; }}
</style>
</head>
<body>
<h1>Partial Rendering PoC — Results</h1>
<table class="summary">
  <tr><td>Full recompute total (frames 1+)</td><td class="v">{ft:.1}ms</td></tr>
  <tr><td>Incremental total (frames 1+)</td>  <td class="v">{it:.1}ms</td></tr>
  <tr><td>Speedup</td>                         <td class="v">{sp:.1}×</td></tr>
</table>
{frames}
</body></html>"#,
        ft = full_total.as_secs_f64() * 1000.0,
        it = incr_total.as_secs_f64() * 1000.0,
        sp = speedup,
        frames = frames_html,
    )
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

    eprintln!("Running {} frames...", n);
    let results = run_all(&frames);

    // Console summary
    println!("{:<14} {:>5}  {:>10}  {:>12}  {:>9}  {}",
        "approach", "frame", "elapsed", "render_calls", "skipped", "diff");
    println!("{}", "-".repeat(72));

    let mut full_total = Duration::ZERO;
    let mut incr_total = Duration::ZERO;

    for r in &results {
        let diff_summary = if !r.incr_pixels.is_empty() && r.w > 0 {
            let d = pixel_diff(&r.full_pixels, &r.incr_pixels, r.w, r.h);
            if d.different_pixels == 0 {
                "✓ pixel-perfect".to_string()
            } else {
                format!("≠ {} px differ (max Δ={})", d.different_pixels, d.max_channel_diff)
            }
        } else { "n/a".into() };

        println!("full          #{:<3}  {:>8.1}ms  {:>12}  {:>9}",
            r.idx, r.full_time.as_secs_f64()*1000.0, 1, 0);
        println!("incremental   #{:<3}  {:>8.1}ms  {:>12}  {:>9}  {}",
            r.idx, r.incr_time.as_secs_f64()*1000.0, r.render_calls, r.skipped,
            diff_summary);
        println!();

        if r.idx > 0 {
            full_total += r.full_time;
            incr_total += r.incr_time;
        }
    }

    println!("{}", "=".repeat(72));
    println!("TOTALS (frames 1-{}):", n - 1);
    println!("  full recompute :  {:>7.1}ms", full_total.as_secs_f64()*1000.0);
    println!("  incremental    :  {:>7.1}ms", incr_total.as_secs_f64()*1000.0);
    println!("  speedup        :  {:.1}×", full_total.as_secs_f64()/incr_total.as_secs_f64().max(1e-9));

    // HTML report
    let report_path = "/tmp/poc_report.html";
    eprintln!("\nGenerating HTML report...");
    let html = generate_report(&results);
    std::fs::write(report_path, html).expect("write report");
    eprintln!("Report: file://{report_path}");
}
