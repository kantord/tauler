use std::collections::{BTreeSet, HashMap, HashSet};
use std::time::{Duration, Instant};

use lru::LruCache;
use parley::fontique::GenericFamily;

use image::{ImageBuffer, Rgba};
use takumi::{
    layout::{node::Node, Viewport},
    rendering::{
        measure_layout as takumi_measure_layout, render as takumi_render, RenderOptions,
    },
    resources::image::ImageSource,
    GlobalContext,
};

use takumi_incr::*;
use optative::reconcile::Reconcile;
use optative::ManagedSet;
// ---------------------------------------------------------------------------
// GlobalContext factory
// ---------------------------------------------------------------------------

fn family_name_for_path(
    collection: &mut parley::fontique::Collection,
    path: &std::path::Path,
) -> Option<String> {
    use parley::fontique::SourceKind;
    let names: Vec<String> = collection.family_names().map(|s| s.to_string()).collect();
    for name in &names {
        if let Some(info) = collection.family_by_name(name) {
            for font in info.fonts() {
                if let SourceKind::Path(p) = &font.source().kind {
                    if p.as_ref() == path {
                        return Some(name.clone());
                    }
                }
            }
        }
    }
    None
}

fn load_targeted_fonts(ctx: &mut GlobalContext) {
    use parley::fontique::{Collection, CollectionOptions, SourceKind};
    let mut temp = Collection::new(CollectionOptions { shared: false, system_fonts: false });
    temp.load_system_fonts();
    let targeted = [GenericFamily::SansSerif, GenericFamily::Monospace, GenericFamily::Emoji];
    let mut paths: Vec<(GenericFamily, std::path::PathBuf)> = Vec::new();
    for &generic in &targeted {
        let Some(id) = temp.generic_families(generic).next() else { continue };
        let names: Vec<String> = temp.family_names().map(|s| s.to_string()).collect();
        let Some(name) = names.iter().find(|n| temp.family_by_name(n).map(|i| i.id()) == Some(id)) else { continue };
        let Some(family) = temp.family_by_name(name) else { continue };
        let Some(path) = family.fonts().iter().find_map(|font| match &font.source().kind {
            SourceKind::Path(p) => Some(p.as_ref().to_path_buf()),
            _ => None,
        }) else { continue };
        paths.push((generic, path));
    }
    if paths.is_empty() {
        ctx.font_context.collection.load_system_fonts();
        return;
    }
    for (_, path) in &paths {
        ctx.font_context.collection.load_fonts_from_paths(std::iter::once(path));
    }
    for (generic, path) in &paths {
        if let Some(name) = family_name_for_path(&mut ctx.font_context.collection, path) {
            if let Some(info) = ctx.font_context.collection.family_by_name(&name) {
                ctx.font_context.collection.set_generic_families(*generic, std::iter::once(info.id()));
            }
        }
    }
}

fn new_ctx() -> GlobalContext {
    let mut ctx = GlobalContext::default();
    load_targeted_fonts(&mut ctx);
    let font_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../assets/fonts/inter/InterVariable.ttf");
    ctx.font_context
        .collection
        .load_fonts_from_paths(std::iter::once(&font_path));
    if let Some(info) = ctx.font_context.collection.family_by_name("Inter Variable") {
        let id = info.id();
        ctx.font_context
            .collection
            .set_generic_families(GenericFamily::SansSerif, std::iter::once(id));
        ctx.font_context
            .collection
            .set_generic_families(GenericFamily::Monospace, std::iter::once(id));
    }
    let assets_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test-assets");
    if let Ok(entries) = std::fs::read_dir(&assets_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("png") {
                if let (Ok(bytes), Some(stem)) = (
                    std::fs::read(&path),
                    path.file_stem().and_then(|s| s.to_str()),
                ) {
                    if let Ok(src) = ImageSource::from_bytes(&bytes) {
                        ctx.persistent_image_store.insert(stem.to_string(), src);
                    }
                }
            }
        }
    }
    ctx
}

// ---------------------------------------------------------------------------
// Benchmark context factory (loads fonts; benchmark-only)
// ---------------------------------------------------------------------------

fn new_bench_ctx() -> Ctx {
    Ctx {
        changed_ids: Vec::new(),
        node_dims: HashMap::new(),
    }
}

// ---------------------------------------------------------------------------
// Image utilities (for HTML report)
// ---------------------------------------------------------------------------

fn encode_png(pixels: &[u8], w: u32, h: u32) -> Vec<u8> {
    let img = ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(w, h, pixels.to_vec()).expect("buf");
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png)
        .expect("png");
    buf.into_inner()
}

fn b64(data: &[u8]) -> String {
    const C: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let (b0, b1, b2) = (
            chunk[0] as u32,
            if chunk.len() > 1 { chunk[1] as u32 } else { 0 },
            if chunk.len() > 2 { chunk[2] as u32 } else { 0 },
        );
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(C[((n >> 18) & 63) as usize] as char);
        out.push(C[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            C[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            C[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

fn data_uri(pixels: &[u8], w: u32, h: u32) -> String {
    format!("data:image/png;base64,{}", b64(&encode_png(pixels, w, h)))
}

struct DiffResult {
    weighted: f64,
    img: Vec<u8>,
}

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
        let dr = (a[i] as i32 - b[i] as i32).unsigned_abs() as f64;
        let dg = (a[i + 1] as i32 - b[i + 1] as i32).unsigned_abs() as f64;
        let db = (a[i + 2] as i32 - b[i + 2] as i32).unsigned_abs() as f64;
        let m = (dr + dg + db + dr.max(dg).max(db)) / 4.0;
        let t = m / 255.0;
        weighted += t * t * t;
        let vis_alpha = (t.sqrt() * 255.0) as u8;
        if vis_alpha > 0 {
            img[i] = 255;
            img[i + 1] = 0;
            img[i + 2] = 255;
            img[i + 3] = vis_alpha;
        } else {
            img[i] = a[i] / 5;
            img[i + 1] = a[i + 1] / 5;
            img[i + 2] = a[i + 2] / 5;
            img[i + 3] = 255;
        }
    }
    DiffResult { weighted, img }
}

/// Build a pixel-mask (same layout as a full RGBA buffer) that is opaque only
/// within dirty tiles.  Used to restrict diff comparisons to the re-rendered
/// region so that skipped tiles don't contribute false positives.
fn dirty_mask(dirty: &HashSet<(u32, u32)>, w: u32, h: u32, tc: &TileConfig) -> Vec<bool> {
    let mut mask = vec![false; (w * h) as usize];
    for &(tx, ty) in dirty {
        let px = tx * tc.tile_size;
        let py = ty * tc.tile_size;
        let pw = tc.tile_size.min(w.saturating_sub(px));
        let ph = tc.tile_size.min(h.saturating_sub(py));
        for row in 0..ph {
            let base = ((py + row) * w + px) as usize;
            for col in 0..pw as usize {
                mask[base + col] = true;
            }
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
            img[i] = a[i] / 5;
            img[i + 1] = a[i + 1] / 5;
            img[i + 2] = a[i + 2] / 5;
            img[i + 3] = 255;
            continue;
        }
        let dr = (a[i] as i32 - b[i] as i32).unsigned_abs() as f64;
        let dg = (a[i + 1] as i32 - b[i + 1] as i32).unsigned_abs() as f64;
        let db = (a[i + 2] as i32 - b[i + 2] as i32).unsigned_abs() as f64;
        let m = (dr + dg + db + dr.max(dg).max(db)) / 4.0;
        let t = m / 255.0;
        weighted += t * t * t;
        let vis_alpha = (t.sqrt() * 255.0) as u8;
        if vis_alpha > 0 {
            img[i] = 255;
            img[i + 1] = 0;
            img[i + 2] = 255;
            img[i + 3] = vis_alpha;
        } else {
            img[i] = a[i] / 5;
            img[i + 1] = a[i + 1] / 5;
            img[i + 2] = a[i + 2] / 5;
            img[i + 3] = 255;
        }
    }
    DiffResult { weighted, img }
}

// ---------------------------------------------------------------------------
// Test suites (unchanged)
// ---------------------------------------------------------------------------

struct SuiteFrame {
    label: String,
    root: IncrNode,
}
struct TestSuite {
    name: &'static str,
    description: &'static str,
    frames: Vec<SuiteFrame>,
    perf_focused: bool,
    force_incremental: bool,
}

fn suite_simple_bar() -> TestSuite {
    let frames = (0..10)
        .map(|i| {
            let clock = format!("{}:{:02}:{:02}", 12, i / 60, i % 60);
            let cpu = format!("CPU {}%", if i % 3 == 0 { i * 4 } else { 12 });
            let label = if i == 0 {
                "cold".into()
            } else if i % 3 == 0 {
                format!("clock+cpu → {clock}")
            } else {
                format!("clock → {clock}")
            };
            let root = IncrNode::Container {
                id: "bar".into(),
                tw: "flex flex-row items-center justify-between w-[400px] h-[24px] bg-gray-900"
                    .into(),
                style: None,
                children: vec![
                    IncrNode::Container {
                        id: "left".into(),
                        tw: "flex flex-row items-center gap-1".into(),
                        style: None,
                        children: vec![
                            IncrNode::Container {
                                id: "logo".into(),
                                tw: "w-[16px] h-[16px] bg-blue-500".into(),
                                style: Some(serde_json::json!({"display":"inline-block"})),
                                children: vec![],
                            },
                            IncrNode::Text {
                                id: "ws".into(),
                                text: "1: term".into(),
                                tw: "text-white text-xs whitespace-nowrap".into(),
                                style: None,
                            },
                            IncrNode::Text {
                                id: "title".into(),
                                text: "nvim main.rs".into(),
                                tw: "text-gray-400 text-xs whitespace-nowrap".into(),
                                style: None,
                            },
                        ],
                    },
                    IncrNode::Container {
                        id: "center".into(),
                        tw: "flex flex-row items-center".into(),
                        style: None,
                        children: vec![IncrNode::Text {
                            id: "clock".into(),
                            text: clock.clone(),
                            tw: "text-white text-xs font-mono whitespace-nowrap".into(),
                            style: None,
                        }],
                    },
                    IncrNode::Container {
                        id: "right".into(),
                        tw: "flex flex-row items-center gap-1".into(),
                        style: None,
                        children: vec![
                            IncrNode::Text {
                                id: "cpu".into(),
                                text: cpu.clone(),
                                tw: "text-white text-xs whitespace-nowrap".into(),
                                style: None,
                            },
                            IncrNode::Text {
                                id: "mem".into(),
                                text: "MEM 4G".into(),
                                tw: "text-white text-xs whitespace-nowrap".into(),
                                style: None,
                            },
                            IncrNode::Text {
                                id: "bat".into(),
                                text: "87%".into(),
                                tw: "text-white text-xs whitespace-nowrap".into(),
                                style: None,
                            },
                        ],
                    },
                ],
            };
            SuiteFrame { label, root }
        })
        .collect();
    TestSuite {
        name: "Simple Status Bar",
        description: "Baseline — no effects. Clock ticks each frame, CPU every 3rd.",
        frames,
        perf_focused: true,
        force_incremental: false,
    }
}

fn suite_shadow_cards() -> TestSuite {
    let frames = (0..10).map(|i| {
        let count = i + 1;
        let msgs = ["Build complete","Tests passed","Deploy done","Lint clean","Type check ok"];
        let msg   = msgs[i % msgs.len()];
        let label = if i==0{"cold".into()}else{format!("notification #{count}")};
        let root = IncrNode::Container{id:"cards".into(),
            tw:"flex flex-row gap-4 p-4 bg-gray-100 w-[440px] h-[90px]".into(),style:None,children:vec![
            IncrNode::Container{id:"notif".into(),
                tw:"flex flex-col justify-between p-3 bg-white rounded-xl shadow-2xl w-[190px]".into(),style:None,children:vec![
                IncrNode::Text{id:"notif-title".into(),text:format!("{count} new"),tw:"text-gray-900 text-sm font-bold whitespace-nowrap".into(),style:None},
                IncrNode::Text{id:"notif-body".into(),text:msg.into(),tw:"text-gray-500 text-xs whitespace-nowrap".into(),style:None},
            ]},
            IncrNode::Container{id:"static-card".into(),
                tw:"flex flex-col justify-center items-center p-3 bg-white rounded-xl shadow-2xl w-[190px]".into(),style:None,children:vec![
                IncrNode::Text{id:"static-label".into(),text:"System OK".into(),tw:"text-green-600 text-sm font-bold whitespace-nowrap".into(),style:None},
                IncrNode::Text{id:"static-sub".into(),text:"All services running".into(),tw:"text-gray-500 text-xs whitespace-nowrap".into(),style:None},
            ]},
        ]};
        SuiteFrame{label,root}
    }).collect();
    TestSuite {
        name: "Shadow Cards",
        description: "Two rounded+shadow cards. Left changes each frame, right is fully static.",
        frames,
        perf_focused: false,
        force_incremental: true,
    }
}

fn suite_blurred_overlay() -> TestSuite {
    let frames = (0..10).map(|i| {
        let value = format!("{}°C", 42 + i);
        let alert = if i % 4 == 0 { format!("⚠ spike at {}s", i * 10) } else { "nominal".into() };
        let label = if i==0{"cold".into()} else if i%4==0{format!("value+alert → {value}")} else{format!("value → {value}")};
        let root = IncrNode::Container{id:"overlay".into(),
            tw:"flex flex-row items-center gap-4 px-4 w-[440px] h-[40px] bg-slate-900/80 rounded-2xl shadow-inner".into(),style:None,children:vec![
            IncrNode::Container{id:"badge".into(),
                tw:"flex items-center justify-center w-[32px] h-[32px] bg-blue-600 rounded-lg shadow-md".into(),style:None,children:vec![
                IncrNode::Text{id:"badge-icon".into(),text:"⚡".into(),tw:"text-white text-sm".into(),style:None},
            ]},
            IncrNode::Text{id:"temp".into(),text:value.clone(),tw:"text-white text-sm font-mono font-bold whitespace-nowrap".into(),style:None},
            IncrNode::Text{id:"label".into(),text:"GPU Temp".into(),tw:"text-slate-400 text-xs whitespace-nowrap".into(),style:None},
            IncrNode::Container{id:"status".into(),
                tw:"flex items-center ml-auto px-2 py-0.5 bg-slate-700 rounded-md".into(),style:None,children:vec![
                IncrNode::Text{id:"alert".into(),text:alert.clone(),tw:"text-yellow-300 text-xs whitespace-nowrap".into(),style:None},
            ]},
        ]};
        SuiteFrame{label,root}
    }).collect();
    TestSuite {
        name: "GPU Temp Monitor",
        description: "Rounded status pill. Temperature changes every frame; alert fires every 4th.",
        frames,
        perf_focused: false,
        force_incremental: true,
    }
}

fn suite_dense_metrics() -> TestSuite {
    let frames = (0..10)
        .map(|i| {
            let metrics = [
                (
                    "CPU",
                    format!("{}%", if i % 2 == 0 { 12 + i * 3 } else { 15 }),
                ),
                ("MEM", "4.2G".into()),
                ("GPU", format!("{}%", 60 + i * 2)),
                ("DISK", "42%".into()),
                ("NET↑", "1.2M".into()),
                ("TEMP", "62°C".into()),
            ];
            let label = if i == 0 {
                "cold".into()
            } else {
                format!("cpu={} gpu={}%", metrics[0].1, 60 + i * 2)
            };

            let cols: Vec<IncrNode> = metrics
                .iter()
                .map(|(name, val)| IncrNode::Container {
                    id: format!("col-{name}"),
                    tw: "flex flex-col items-center px-2 bg-gray-800 rounded-lg shadow-md".into(),
                    style: None,
                    children: vec![
                        IncrNode::Text {
                            id: format!("lbl-{name}"),
                            text: name.to_string(),
                            tw: "text-gray-400 text-[10px] whitespace-nowrap".into(),
                            style: None,
                        },
                        IncrNode::Text {
                            id: format!("val-{name}"),
                            text: val.clone(),
                            tw: "text-white text-xs font-mono font-bold whitespace-nowrap".into(),
                            style: None,
                        },
                    ],
                })
                .collect();

            let root = IncrNode::Container {
                id: "grid".into(),
                tw: "flex flex-row gap-1 p-1 bg-gray-900 w-[360px] h-[36px]".into(),
                style: None,
                children: cols,
            };

            SuiteFrame { label, root }
        })
        .collect();
    TestSuite {
        name: "Dense Metrics Grid",
        description:
            "6 shadow+rounded columns. CPU and GPU change each frame; the other 4 stay static.",
        frames,
        perf_focused: false,
        force_incremental: true,
    }
}

// ---------------------------------------------------------------------------
// Benchmark
// ---------------------------------------------------------------------------

struct FrameResult {
    label: String,
    full_time: Duration,
    incr_time: Duration,
    full_px: Vec<u8>,
    prev_full_px: Vec<u8>,
    incr_px: Vec<u8>,
    w: u32,
    h: u32,
    render_calls: u32,
    skipped: u32,
    cache_hits: u32,
    dirty_tiles: HashSet<(u32, u32)>,
    bailout_stage: Option<u8>,
}
struct SuiteResult {
    frames: Vec<FrameResult>,
    dpr: f32,
}

// ---------------------------------------------------------------------------
// OLS self-calibration
// ---------------------------------------------------------------------------

struct CalibrationResult {
    model: CostModel,
    r_squared: f64,
    n_samples: usize,
}

/// Fit cost model coefficients from (canvas_area_px², n_nodes, render_ms) samples via OLS.
/// Area is scaled by 1e4 internally to improve numerical conditioning.
/// Returns None if fewer than 4 samples or the normal equations are degenerate.
fn fit_cost_model(samples: &[(f64, f64, f64)]) -> Option<CalibrationResult> {
    let n = samples.len();
    if n < 4 {
        return None;
    }
    const SCALE: f64 = 1e4;
    let mut xtx = [[0.0f64; 3]; 3];
    let mut xty = [0.0f64; 3];
    for &(area, nodes, time) in samples {
        let x = [1.0, area / SCALE, nodes];
        for i in 0..3 {
            xty[i] += x[i] * time;
            for j in 0..3 {
                xtx[i][j] += x[i] * x[j];
            }
        }
    }
    let mut a = xtx;
    let mut b = xty;
    for col in 0..3 {
        let mut pivot = col;
        for r in (col + 1)..3 {
            if a[r][col].abs() > a[pivot][col].abs() {
                pivot = r;
            }
        }
        a.swap(col, pivot);
        b.swap(col, pivot);
        let p = a[col][col];
        if p.abs() < 1e-14 {
            return None;
        }
        for row in (col + 1)..3 {
            let f = a[row][col] / p;
            #[allow(clippy::needless_range_loop)]
            for k in col..3 {
                a[row][k] -= f * a[col][k];
            }
            b[row] -= f * b[col];
        }
    }
    let mut sol = [0.0f64; 3];
    for i in (0..3).rev() {
        sol[i] = b[i];
        for j in (i + 1)..3 {
            sol[i] -= a[i][j] * sol[j];
        }
        sol[i] /= a[i][i];
    }
    let (o_raw, k_area_raw, k_nodes_raw) = (sol[0], sol[1] / SCALE, sol[2]);
    let mean_t = samples.iter().map(|&(_, _, t)| t).sum::<f64>() / n as f64;
    let ss_tot: f64 = samples.iter().map(|&(_, _, t)| (t - mean_t).powi(2)).sum();
    let ss_res: f64 = samples
        .iter()
        .map(|&(area, nd, t)| (t - (o_raw + k_area_raw * area + k_nodes_raw * nd)).powi(2))
        .sum();
    let r_squared = if ss_tot > 1e-15 {
        (1.0 - ss_res / ss_tot).max(0.0)
    } else {
        0.0
    };
    Some(CalibrationResult {
        model: CostModel {
            o_fixed: o_raw.max(0.05),
            k_area: k_area_raw.max(1e-7),
            k_nodes: k_nodes_raw.max(0.001),
        },
        r_squared,
        n_samples: n,
    })
}



fn run_suite(
    suite: &TestSuite,
    cm: &CostModel,
    dpr: f32,
    cal_samples: &mut Vec<(f64, f64, f64)>,
    tc: &TileConfig,
    allow_bailout: bool,
) -> SuiteResult {
    let mut incr_ctx = new_bench_ctx();
    let incr_global = new_ctx();
    let mut incr_set: ManagedSet<IncrNode> = ManagedSet::new();
    let mut frame_buf: Vec<u8> = Vec::new();
    let mut prev_full: Vec<u8> = Vec::new();
    // Bboxes from the PREVIOUS frame's stub layout, used for moved-node detection
    // so we always compare stub vs stub and avoid false positives.
    let mut prev_stub_bboxes: HashMap<String, Rect> = HashMap::new();
    // LRU tile render cache: metadata_fp → tile_size×tile_size×4 bytes.
    // Capacity is derived from TILE_CACHE_MB so it auto-adjusts when tile_size changes.
    let mut tile_cache: LruCache<u64, Vec<u8>> = LruCache::new(tc.cache_cap);
    // Persistent tile→node map: for each tile the set of nodes whose shadow-
    // expanded bbox overlaps it.  Rebuilt each frame from current bboxes.
    // (Incremental update is a future optimisation — rebuild is fast enough.)
    let mut tile_node_map: HashMap<(u32, u32), BTreeSet<String>> = HashMap::new();

    let frames: Vec<FrameResult> = suite
        .frames
        .iter()
        .enumerate()
        .map(|(frame_idx, f)| {
            // ── Full render (fresh context, no caching) ──────────────────────────
            let root_incr = f.root.clone();
            let full_global = new_ctx();
            let t = Instant::now();
            let (full_px, w, h) = {
                let root_json = root_incr.to_json();
                let node = parse_layout(&root_json).unwrap_or_else(|_| Node::container(vec![]));
                let img = takumi_render(
                    RenderOptions::builder()
                        .global(&full_global)
                        .viewport(Viewport::new((None, None)).with_device_pixel_ratio(dpr))
                        .node(node)
                        .build(),
                )
                .expect("full render");
                let (w, h) = img.dimensions();
                (img.into_raw(), w, h)
            };
            let full_time = t.elapsed();

            // ── Bail-out Stage 1: canvas too small to benefit from incremental ────
            if allow_bailout && (w as u64 * h as u64) < BAILOUT_MIN_PIXELS {
                frame_buf = full_px.clone();
                let my_prev_full = std::mem::replace(&mut prev_full, full_px.clone());
                return FrameResult {
                    label: f.label.clone(),
                    full_time,
                    incr_time: full_time,
                    full_px,
                    prev_full_px: my_prev_full,
                    incr_px: frame_buf.clone(),
                    w,
                    h,
                    render_calls: 0,
                    skipped: 0,
                    cache_hits: 0,
                    dirty_tiles: HashSet::new(),
                    bailout_stage: Some(1),
                };
            }

            // ── Tile-based incremental render ─────────────────────────────────────
            incr_ctx.changed_ids.clear();
            let t = Instant::now();

            // 1. Reconcile — populates changed_ids
            incr_set.reconcile(vec![root_incr.clone()], &mut incr_ctx, &mut ());

            let cols = w.div_ceil(tc.tile_size);
            let rows = h.div_ceil(tc.tile_size);

            // No-op short-circuit: if nothing changed and frame_buf is already
            // populated, every tile is identical to last frame — skip all remaining
            // work.  Static frames (no clock tick, no focus change, etc.) cost
            // essentially nothing in the real pipeline.
            if incr_ctx.changed_ids.is_empty() && !frame_buf.is_empty() {
                let incr_time = t.elapsed();
                let incr_px = frame_buf.clone();
                let my_prev_full = std::mem::replace(&mut prev_full, full_px.clone());
                return FrameResult {
                    label: f.label.clone(),
                    full_time,
                    incr_time,
                    full_px,
                    prev_full_px: my_prev_full,
                    incr_px,
                    w,
                    h,
                    render_calls: 0,
                    skipped: cols * rows,
                    cache_hits: 0,
                    dirty_tiles: HashSet::new(),
                    bailout_stage: None,
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
            let node_map = build_node_map(&root_incr);

            // Step (a): update node_dims for changed leaves; track whether anything
            // that affects flex geometry actually changed.
            //   dims_changed        — a Text/Image node rendered to a different size
            //   collection_changed  — a Container tw changed (affects flex positions)
            // If neither is true the stub layout would produce identical positions to
            // last frame, so step (b) can be skipped entirely.
            let mut dims_changed = false;
            let mut collection_changed = false;
            for id in &incr_ctx.changed_ids {
                if let Some(&node) = node_map.get(id.as_str()) {
                    match node {
                        IncrNode::Text { .. } => {
                            let new_dims = measure_natural(node, &incr_global);
                            if incr_ctx.node_dims.get(id.as_str()).copied() != Some(new_dims) {
                                dims_changed = true;
                            }
                            incr_ctx.node_dims.insert(id.clone(), new_dims);
                        }
                        IncrNode::Image { .. } => {
                            let new_dims = measure_natural(node, &incr_global);
                            if incr_ctx.node_dims.get(id.as_str()).copied() != Some(new_dims) {
                                dims_changed = true;
                            }
                            incr_ctx.node_dims.insert(id.clone(), new_dims);
                        }
                        IncrNode::Container { children, .. } => {
                            if children.is_empty() {
                                // Leaf container (Image variant) — measure it
                                let new_dims = measure_natural(node, &incr_global);
                                if incr_ctx.node_dims.get(id.as_str()).copied() != Some(new_dims) {
                                    dims_changed = true;
                                }
                                incr_ctx.node_dims.insert(id.clone(), new_dims);
                            } else {
                                collection_changed = true;
                            }
                        }
                    }
                }
            }
            // Step (b): stub layout — skipped when positions are provably unchanged.
            // new_bboxes is Some only when we actually recompute; bboxes borrows from
            // it when Some, or from prev_stub_bboxes (old) when None.  This keeps
            // prev_stub_bboxes as the *previous* frame's positions until end-of-frame,
            // so the moved-node detection can diff new vs old correctly.
            // No HashMap clone when stub is skipped — avoids O(N) allocation per frame.
            let stub_recomputed = dims_changed || collection_changed;
            let new_bboxes: Option<HashMap<String, Rect>> = if stub_recomputed {
                let stub_json = stub_scene_json(&root_incr, &incr_ctx.node_dims);
                let node = parse_layout(&stub_json).unwrap_or_else(|_| Node::container(vec![]));
                let measured = takumi_measure_layout(
                    RenderOptions::builder()
                        .global(&incr_global)
                        .viewport(Viewport::new((None, None)).with_device_pixel_ratio(dpr))
                        .node(node)
                        .build(),
                )
                .expect("stub layout");
                let mut sb = HashMap::new();
                collect_bboxes(&measured, &root_incr, &mut sb);
                Some(sb)
            } else {
                None
            };
            let bboxes: &HashMap<String, Rect> = new_bboxes.as_ref().unwrap_or(&prev_stub_bboxes);

            // 3. Compute dirty tiles and update tile→node map.
            let mut dirty: HashSet<(u32, u32)> = HashSet::new();

            if frame_buf.len() != (w * h * 4) as usize {
                // First frame: full build of tile_node_map + all tiles dirty.
                frame_buf = vec![0u8; (w * h * 4) as usize];
                tile_node_map = build_tile_node_map(bboxes, cols, rows, tc);
                for ty in 0..rows {
                    for tx in 0..cols {
                        dirty.insert((tx, ty));
                    }
                }
            } else {
                // Incremental tile_node_map update — O(changed_nodes × tiles_per_node)
                // instead of O(total_nodes × tiles_per_node) for a full rebuild.
                //
                // Step 1: update entries for nodes whose content changed.
                for id in &incr_ctx.changed_ids {
                    if let Some(old_r) = prev_stub_bboxes.get(id.as_str()) {
                        for (tx, ty) in tiles_for_bbox(old_r, cols, rows, tc) {
                            if let Some(s) = tile_node_map.get_mut(&(tx, ty)) {
                                s.remove(id.as_str());
                            }
                        }
                        mark_dirty(old_r, tc.tile_size, w, h, &mut dirty, tc);
                    }
                    if let Some(new_r) = bboxes.get(id.as_str()) {
                        for (tx, ty) in tiles_for_bbox(new_r, cols, rows, tc) {
                            tile_node_map
                                .entry((tx, ty))
                                .or_default()
                                .insert(id.clone());
                        }
                        mark_dirty(new_r, tc.tile_size, w, h, &mut dirty, tc);
                    }
                }
                // Step 2: update entries for nodes that moved due to layout reflow
                // (not in changed_ids but bbox shifted — e.g. justify-between reflow).
                // Skipped entirely when stub layout was not recomputed: bboxes ==
                // prev_stub_bboxes so every delta is zero — O(N) loop with no effect.
                if stub_recomputed {
                    let changed_set: HashSet<&str> =
                        incr_ctx.changed_ids.iter().map(String::as_str).collect();
                    for (id, new_r) in bboxes {
                        if changed_set.contains(id.as_str()) {
                            continue;
                        }
                        if let Some(old_r) = prev_stub_bboxes.get(id.as_str()) {
                            if (new_r.x - old_r.x).abs() > 0.5 || (new_r.y - old_r.y).abs() > 0.5 {
                                for (tx, ty) in tiles_for_bbox(old_r, cols, rows, tc) {
                                    if let Some(s) = tile_node_map.get_mut(&(tx, ty)) {
                                        s.remove(id.as_str());
                                    }
                                }
                                for (tx, ty) in tiles_for_bbox(new_r, cols, rows, tc) {
                                    tile_node_map
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
            }

            // ── Bail-out Stage 2: too many dirty tiles — incremental can't help ───
            let total_tiles = cols * rows;
            if allow_bailout
                && !dirty.is_empty()
                && dirty.len() as f32 / total_tiles as f32 > BAILOUT_DIRTY_RATIO
            {
                // In production a Stage 2 bail-out does a fresh full render.
                // Time it now so incr_time reflects the true production cost.
                let fresh_node = {
                    let j = root_incr.to_json();
                    parse_layout(&j).unwrap_or_else(|_| Node::container(vec![]))
                };
                let _ = takumi_render(
                    RenderOptions::builder()
                        .global(&incr_global)
                        .viewport(Viewport::new((None, None)).with_device_pixel_ratio(dpr))
                        .node(fresh_node)
                        .build(),
                )
                .expect("stage-2 bail-out render");
                let incr_time = t.elapsed(); // pipeline overhead + fresh render
                frame_buf = full_px.clone();
                let my_prev_full = std::mem::replace(&mut prev_full, full_px.clone());
                if let Some(nb) = new_bboxes {
                    prev_stub_bboxes = nb;
                }
                return FrameResult {
                    label: f.label.clone(),
                    full_time,
                    incr_time,
                    full_px,
                    prev_full_px: my_prev_full,
                    incr_px: frame_buf.clone(),
                    w,
                    h,
                    render_calls: 0,
                    skipped: 0,
                    cache_hits: 0,
                    dirty_tiles: dirty,
                    bailout_stage: Some(2),
                };
            }

            // 4. Cache lookup — fingerprint each dirty tile upfront, stitch hits
            //    immediately and remove from dirty so they don't inflate band areas.
            let skipped = cols * rows - dirty.len() as u32; // tiles that were never dirty
            let fps: HashMap<(u32, u32), u64> = dirty
                .iter()
                .map(|&(tx, ty)| {
                    (
                        (tx, ty),
                        tile_fingerprint(tx, ty, &tile_node_map, bboxes, &node_map),
                    )
                })
                .collect();
            let mut cache_hits = 0u32;
            dirty.retain(|&(tx, ty)| match tile_cache.get(&fps[&(tx, ty)]).cloned() {
                Some(px) => {
                    stitch(
                        &mut frame_buf,
                        w,
                        h,
                        &px,
                        tc.tile_size,
                        tx * tc.tile_size,
                        ty * tc.tile_size,
                    );
                    cache_hits += 1;
                    false
                }
                None => true,
            });

            // 5. Categorical + spatial grouping with cost-model greedy merge.
            //    compute_candidates handles all frames uniformly — cold frame (all
            //    tiles dirty) and incremental alike.  No special-case sentinel needed.
            let candidates: Vec<RenderCandidate> = if dirty.is_empty() {
                vec![]
            } else {
                let raw = compute_candidates(&dirty, &tile_node_map, tc);
                greedy_merge_candidates(raw, cm, tc)
            };
            let render_calls = candidates.len() as u32;

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

                let scene = serde_json::json!({
                    "type": "container",
                    "style": { "display": "block", "position": "relative",
                        "width": canvas_w as f32, "height": canvas_h as f32,
                        "overflow": "hidden" },
                    "children": nodes
                });
                let node = parse_layout(&scene).unwrap_or_else(|_| Node::container(vec![]));
                let t_cand = Instant::now();
                let cand_px = takumi_render(
                    RenderOptions::builder()
                        .global(&incr_global)
                        .viewport(Viewport::new((None, None)).with_device_pixel_ratio(dpr))
                        .node(node)
                        .build(),
                )
                .expect("candidate render")
                .into_raw();
                if suite.perf_focused && frame_idx > 0 {
                    cal_samples.push((
                        canvas_w as f64 * canvas_h as f64,
                        nodes.len() as f64,
                        t_cand.elapsed().as_secs_f64() * 1000.0,
                    ));
                }

                for &(tx, ty) in &cand.tiles {
                    let px_x = tx * tc.tile_size;
                    let px_y = ty * tc.tile_size;
                    let off_x = tc.shadow_buf + (tx - cand.min_tx) * tc.tile_size;
                    let off_y = tc.shadow_buf + (ty - cand.min_ty) * tc.tile_size;
                    let tile_px =
                        crop_pixels(&cand_px, canvas_w, off_x, off_y, tc.tile_size, tc.tile_size);
                    stitch(&mut frame_buf, w, h, &tile_px, tc.tile_size, px_x, px_y);
                    tile_cache.put(fps[&(tx, ty)], tile_px);
                }
            }

            let incr_time = t.elapsed();
            let incr_px = frame_buf.clone();
            let my_prev_full = std::mem::replace(&mut prev_full, full_px.clone());
            let saved_dirty = dirty.clone();
            if let Some(nb) = new_bboxes {
                prev_stub_bboxes = nb;
            }

            FrameResult {
                label: f.label.clone(),
                full_time,
                incr_time,
                full_px,
                prev_full_px: my_prev_full,
                incr_px,
                w,
                h,
                render_calls,
                skipped,
                cache_hits,
                dirty_tiles: saved_dirty,
                bailout_stage: None,
            }
        })
        .collect();

    SuiteResult { frames, dpr }
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

fn html_report(suites: &[&TestSuite], results: &[SuiteResult], tc: &TileConfig) -> String {
    // ── Timing totals ────────────────────────────────────────────────────────
    let mut all_full = Duration::ZERO;
    let mut all_incr = Duration::ZERO;
    let mut perf_full = Duration::ZERO;
    let mut perf_incr = Duration::ZERO;
    for (suite, s) in suites.iter().zip(results.iter()) {
        for (i, f) in s.frames.iter().enumerate() {
            if i > 0 {
                all_full += f.full_time;
                all_incr += f.incr_time;
                if suite.perf_focused {
                    perf_full += f.full_time;
                    perf_incr += f.incr_time;
                }
            }
        }
    }
    let overall_speedup = all_full.as_secs_f64() / all_incr.as_secs_f64().max(1e-9);
    let perf_speedup = perf_full.as_secs_f64() / perf_incr.as_secs_f64().max(1e-9);

    // ── Per-suite passes ─────────────────────────────────────────────────────
    // Collect worst imperfect frames across all suites for the summary page.
    // Each entry: (weighted, suite_name, frame_idx, label, summary_snippet_html)
    let mut imperfect: Vec<(f64, &str, usize, String, String)> = Vec::new();

    let mut tab_btns = String::from(
        r#"<button class="tab-btn active" onclick="showTab('tab-summary',this)">Summary</button>"#,
    );
    let mut suite_tabs = String::new();
    let mut table_rows = String::new();

    for (si, (suite, result)) in suites.iter().zip(results.iter()).enumerate() {
        let tab_id = format!("tab-suite-{si}");
        tab_btns.push_str(&format!(
            r#"<button class="tab-btn" onclick="showTab('{tab_id}',this)">{} ({}×)</button>"#,
            suite.name, result.dpr
        ));

        let mut frames_html = String::new();
        let mut s_full = Duration::ZERO;
        let mut s_incr = Duration::ZERO;
        let mut n_perfect = 0u32;
        let mut n_total = 0u32;

        for (fi, f) in result.frames.iter().enumerate() {
            if fi > 0 {
                s_full += f.full_time;
                s_incr += f.incr_time;
            }
            n_total += 1;

            let full_uri = data_uri(&f.full_px, f.w, f.h);
            let speedup_f = f.full_time.as_secs_f64() / f.incr_time.as_secs_f64().max(1e-9);
            // Display size: 3× pixel-art scaling, capped so huge canvases stay scrollable
            let pw = (f.w * 3).clamp(120, 900);
            let ph = (f.h * 3).clamp(36, 600);

            // Restrict correctness measurement to dirty tiles only.
            let mask = dirty_mask(&f.dirty_tiles, f.w, f.h, tc);
            let chg_w = if f.prev_full_px.len() == f.full_px.len() {
                diff_masked(&f.prev_full_px, &f.full_px, &mask, f.w, f.h).weighted
            } else {
                (f.dirty_tiles.len() * (tc.tile_size * tc.tile_size) as usize) as f64
            };
            // Full-frame change image (for the Δ column) uses unmasked diff so
            // the viewer can see what actually changed regardless of dirty tiles.
            let chg_diff = if f.prev_full_px.len() == f.full_px.len() {
                Some(diff(&f.prev_full_px, &f.full_px, f.w, f.h))
            } else {
                None
            };

            let (d, incr_uri, diff_uri, chg_uri) =
                if !f.incr_px.is_empty() && f.incr_px.len() == f.full_px.len() {
                    let d = diff_masked(&f.full_px, &f.incr_px, &mask, f.w, f.h);
                    let du = data_uri(&d.img, f.w, f.h);
                    let iu = data_uri(&f.incr_px, f.w, f.h);
                    let cu = chg_diff
                        .as_ref()
                        .map(|c| data_uri(&c.img, f.w, f.h))
                        .unwrap_or_default();
                    (Some(d), iu, du, cu)
                } else {
                    (None, String::new(), String::new(), String::new())
                };

            let ratio = d
                .as_ref()
                .map(|d| d.weighted / chg_w.max(1.0))
                .unwrap_or(0.0);
            let perfect = ratio < PERFECT_THRESHOLD;
            if perfect {
                n_perfect += 1;
            }

            let badge = if perfect {
                r#"<span class="ok">✓</span>"#
            } else {
                r#"<span class="diff">≠</span>"#
            };
            let diff_stat = d
                .as_ref()
                .map(|d| {
                    format!(
                        "err={:.1} / chg={:.0} = {:.1}%",
                        d.weighted,
                        chg_w,
                        ratio * 100.0
                    )
                })
                .unwrap_or_default();

            let chg_col = if !chg_uri.is_empty() {
                format!(
                    r#"<div><div class="cap">Δ Change</div><img src="{chg_uri}" style="width:{pw}px;height:{ph}px;image-rendering:pixelated"></div>"#,
                    chg_uri = chg_uri,
                    pw = pw,
                    ph = ph
                )
            } else {
                String::new()
            };

            let bailout_html = match f.bailout_stage {
                Some(1) => r#" <span class="bailout" title="Stage 1 bail-out: canvas too small">[S1]</span>"#,
                Some(2) => r#" <span class="bailout" title="Stage 2 bail-out: too many dirty tiles">[S2]</span>"#,
                _ => "",
            };
            frames_html.push_str(&format!(r#"
            <div class="frame {cls}">
              <div class="fhdr"><strong>Frame {fi}</strong> — {lbl} {badge}{bo}
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
                lbl = f.label, badge = badge, bo = bailout_html,
                ft = f.full_time.as_secs_f64() * 1000.0, it = f.incr_time.as_secs_f64() * 1000.0,
                sp = speedup_f, rc = f.render_calls, sk = f.skipped,
                fu = full_uri, iu = incr_uri, du = diff_uri,
                pw = pw, ph = ph, ds = diff_stat, chg_col = chg_col,
            ));

            // Collect for worst-20 (compact thumbnail in summary), keyed by ratio
            if d.is_some() && !perfect {
                {
                    let tw = f.w.min(240);
                    let th = (f.h * tw / f.w.max(1)).clamp(20, 320);
                    let snippet = format!(
                        r#"
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
                        sn = suite.name,
                        fi = fi,
                        lbl = f.label,
                        badge = badge,
                        ft = f.full_time.as_secs_f64() * 1000.0,
                        it = f.incr_time.as_secs_f64() * 1000.0,
                        sp = speedup_f,
                        ds = diff_stat,
                        fu = full_uri,
                        iu = incr_uri,
                        du = diff_uri,
                        tw = tw,
                        th = th,
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
        suite_tabs.push_str(&format!(
            r#"
        <div id="{tab_id}" class="tab-content" style="display:none">
          <h2>{name} <span class="speedup">{ss:.1}× speedup</span></h2>
          <p class="desc">{desc}</p>
          {frames}
        </div>"#,
            tab_id = tab_id,
            name = suite.name,
            ss = ss,
            desc = suite.description,
            frames = frames_html,
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
    let summary_tab = format!(
        r#"
    <div id="tab-summary" class="tab-content">
      <div class="hero">
        <div><div class="l">Overall speedup (1× DPR, all suites)</div><div class="v">{sp:.1}×</div></div>
        <div><div class="l">Perf-focused speedup (1× + 2× DPR)</div><div class="v">{ps:.1}×</div></div>
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
        ps = perf_speedup,
        ft = all_full.as_secs_f64() * 1000.0,
        it = all_incr.as_secs_f64() * 1000.0,
        rows = table_rows,
        worst = worst_html,
    );

    format!(
        r#"<!DOCTYPE html><html lang="en"><head><meta charset="utf-8">
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
        tab_btns = tab_btns,
        summary = summary_tab,
        suite_tabs = suite_tabs,
    )
}

// ---------------------------------------------------------------------------
// Realistic sidebar suite — mirrors actual tauler layout
// ---------------------------------------------------------------------------

fn suite_realistic_sidebar() -> TestSuite {
    let ws_data: &[(&str, &str, Option<&str>)] = &[
        ("1", "term", Some("main")),
        ("2", "browser", None),
        ("3", "costae", Some("partial-rendering")),
        ("4", "slack", None),
        ("5", "docs", Some("arch-notes")),
        ("6", "api", Some("v2-refactor")),
        ("7", "fe", Some("dashboard")),
        ("8", "debug", None),
        ("9", "infra", Some("tf-migration")),
        ("10", "mail", None),
        ("11", "music", None),
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

        let ws_cards: Vec<IncrNode> = ws_data.iter().enumerate().map(|(j, (key, name, sub))| {
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

            let mut lbl_children = vec![IncrNode::Text {
                id: format!("ws-{j}-name"), text:name.to_string(), tw: name_tw.into(), style: None,
            }];
            if let Some(s) = sub {
                lbl_children.push(IncrNode::Text {
                    id: format!("ws-{j}-sub"), text:s.to_string(),
                    tw: "text-[11px] text-gray-500 truncate".into(),
                    style: None,
                });
            }

            IncrNode::Container {
                id: format!("ws-{j}"), tw: card_tw.into(),
                style: None,
                children: vec![IncrNode::Container {
                    id: format!("ws-{j}-inner"), tw: "flex flex-row items-center gap-2 w-full".into(),
                    style: None,
                    children: vec![
                        IncrNode::Container {
                            id: format!("ws-{j}-badge"), tw: badge_tw.into(),
                            style: None,
                            children: vec![IncrNode::Text {
                                id: format!("ws-{j}-key"), text:key.to_string(),
                                tw: "text-[12px] text-white font-bold".into(),
                                style: None,
                            }],
                        },
                        IncrNode::Container {
                            id: format!("ws-{j}-lbl"), tw: "flex flex-col min-w-0 flex-1".into(),
                            style: None,
                            children: lbl_children,
                        },
                    ],
                }],
            }
        }).collect();

        let root = IncrNode::Container {
            id: "sidebar".into(),
            tw: "flex flex-col w-[300px] h-[2500px] px-4 py-4 bg-gray-900".into(),
            style: None,
            children: vec![
                // Workspace list fills the top (flex-1 pushes bottom cards down)
                IncrNode::Container {
                    id: "ws-area".into(),
                    tw: "flex-1 flex flex-col w-full".into(),
                    style: None,
                    children: vec![IncrNode::Container {
                        id: "ws-list".into(),
                        tw: "flex flex-col gap-2 w-full pt-4".into(),
                        style: None,
                        children: ws_cards,
                    }],
                },
                // Bottom info cards
                IncrNode::Container {
                    id: "bottom".into(),
                    tw: "flex flex-col gap-[10px] w-full".into(),
                    style: None,
                    children: vec![
                        // GitHub WIP (fully static)
                        IncrNode::Container {
                            id: "gh-card".into(),
                            tw: "flex flex-col gap-1 px-3 py-2 bg-gray-800 rounded-xl w-full".into(),
                            style: None,
                            children: vec![
                                IncrNode::Container { id: "gh-hdr".into(),
                                    tw: "flex flex-row items-baseline".into(),
                                    style: None,
                                    children: vec![
                                        IncrNode::Text { id: "gh-ttl".into(),  text:"GITHUB WIP".into(), tw: "flex-1 text-[10px] text-gray-400".into(), style: None },
                                        IncrNode::Text { id: "gh-pr-h".into(), text:"PR".into(),         tw: "w-[24px] text-right text-[8px] text-gray-400".into(), style: None },
                                        IncrNode::Text { id: "gh-tsk-h".into(),text:"tsk".into(),        tw: "w-[24px] text-right text-[8px] text-gray-400".into(), style: None },
                                    ],
                                },
                                IncrNode::Container { id: "gh-r1".into(),
                                    tw: "flex flex-row items-baseline".into(),
                                    style: None,
                                    children: vec![
                                        IncrNode::Text { id: "gh-n1".into(), text:"costae".into(), tw: "flex-1 text-[11px] text-white".into(), style: None },
                                        IncrNode::Text { id: "gh-p1".into(), text:"3".into(),       tw: "w-[24px] text-right text-[11px] text-white".into(), style: None },
                                        IncrNode::Text { id: "gh-t1".into(), text:"—".into(),       tw: "w-[24px] text-right text-[11px] text-gray-400".into(), style: None },
                                    ],
                                },
                                IncrNode::Container { id: "gh-r2".into(),
                                    tw: "flex flex-row items-baseline".into(),
                                    style: None,
                                    children: vec![
                                        IncrNode::Text { id: "gh-n2".into(), text:"takumi".into(), tw: "flex-1 text-[11px] text-white".into(), style: None },
                                        IncrNode::Text { id: "gh-p2".into(), text:"1".into(),       tw: "w-[24px] text-right text-[11px] text-white".into(), style: None },
                                        IncrNode::Text { id: "gh-t2".into(), text:"2".into(),       tw: "w-[24px] text-right text-[11px] text-white".into(), style: None },
                                    ],
                                },
                            ],
                        },
                        // Weather (static in this suite)
                        IncrNode::Container {
                            id: "wx-card".into(),
                            tw: "flex flex-col gap-1 px-3 py-2 bg-gray-800 rounded-xl w-full".into(),
                            style: None,
                            children: vec![
                                IncrNode::Container { id: "wx-r1".into(),
                                    tw: "flex flex-row items-baseline justify-between".into(),
                                    style: None,
                                    children: vec![
                                        IncrNode::Text { id: "wx-temp".into(),  text:"21°C".into(),       tw: "text-[15px] text-white font-bold".into(), style: None },
                                        IncrNode::Text { id: "wx-feels".into(), text:"feels 19°C".into(), tw: "text-[10px] text-gray-400".into(), style: None },
                                    ],
                                },
                                IncrNode::Container { id: "wx-r2".into(),
                                    tw: "flex flex-row justify-between".into(),
                                    style: None,
                                    children: vec![
                                        IncrNode::Text { id: "wx-cond".into(), text:"Partly cloudy".into(), tw: "text-[10px] text-gray-400".into(), style: None },
                                        IncrNode::Text { id: "wx-rh".into(),   text:"RH 62%".into(),        tw: "text-[10px] text-gray-400".into(), style: None },
                                    ],
                                },
                            ],
                        },
                        // Claude usage (pct and progress bar change every 2 frames)
                        IncrNode::Container {
                            id: "claude-card".into(),
                            tw: "flex flex-col gap-1 px-3 py-2 bg-gray-800 rounded-xl w-full".into(),
                            style: None,
                            children: vec![
                                IncrNode::Text { id: "claude-lbl".into(), text:"Claude · main".into(), tw: "text-[10px] text-gray-400".into(), style: None },
                                IncrNode::Container { id: "claude-row".into(),
                                    tw: "flex flex-row items-baseline justify-between".into(),
                                    style: None,
                                    children: vec![
                                        IncrNode::Text { id: "claude-pct".into(),  text:format!("{claude_pct}%"), tw: "text-[15px] text-white font-bold".into(), style: None },
                                        IncrNode::Text { id: "claude-rst".into(),  text:"resets 2h".into(),       tw: "text-[10px] text-gray-400".into(), style: None },
                                    ],
                                },
                                IncrNode::Container {
                                    id: "claude-prog".into(),
                                    tw: "w-full h-[4px] bg-gray-700 rounded-full".into(),
                                    style: None,
                                    children: vec![IncrNode::Container {
                                        id: "claude-fill".into(),
                                        tw: format!("w-[{}px] h-[4px] bg-green-400", ((claude_pct * 200 / 100) as u32).min(200)),
                                        style: Some(serde_json::json!({"display":"inline-block"})),
                                        children: vec![],
                                    }],
                                },
                            ],
                        },
                        // DateTime (time changes every frame)
                        IncrNode::Container {
                            id: "dt-card".into(),
                            tw: "flex flex-row gap-[10px] px-3 py-2 bg-gray-800 rounded-xl w-full".into(),
                            style: None,
                            children: vec![
                                IncrNode::Container { id: "dt-date".into(),
                                    tw: "flex-1 flex flex-col gap-1".into(),
                                    style: None,
                                    children: vec![
                                        IncrNode::Text { id: "dt-dl".into(), text:"DATE".into(),   tw: "text-[10px] text-gray-400".into(), style: None },
                                        IncrNode::Text { id: "dt-dv".into(), text:"Apr 30".into(), tw: "text-[14px] text-white".into(), style: None },
                                    ],
                                },
                                IncrNode::Container { id: "dt-time".into(),
                                    tw: "flex-1 flex flex-col gap-1".into(),
                                    style: None,
                                    children: vec![
                                        IncrNode::Text { id: "dt-tl".into(), text:"TIME".into(),        tw: "text-[10px] text-gray-400".into(), style: None },
                                        IncrNode::Text { id: "dt-tv".into(), text:time_str.clone(),     tw: "text-[14px] text-white font-mono".into(), style: None },
                                    ],
                                },
                            ],
                        },
                    ],
                },
            ],
        };

        SuiteFrame { label, root }
    }).collect();

    TestSuite {
        name: "Realistic Sidebar",
        description: "300×2500 px sidebar: workspace list (focus cycles), GitHub WIP, weather, Claude usage, datetime. Most tiles static.",
        frames,
        perf_focused: true,
        force_incremental: false,
    }
}

// ---------------------------------------------------------------------------
// Shrink-bug regression suite
// ---------------------------------------------------------------------------

fn suite_shrink_bug() -> TestSuite {
    // A single text node with no siblings cycles between wide and narrow values.
    // No reflow occurs (nothing moves), so the stale-old-bbox bug causes the
    // right-side pixels of the wide text to persist after it shrinks.
    let frames = [
        "WWWWWWWWWWWWWWWWWWWWWWWWW",
        "W",
        "WWWWWWWWWWWWWWWWWWWWWWWWW",
        "W",
    ]
    .iter()
    .enumerate()
    .map(|(i, &text)| {
        let root = IncrNode::Container {
            id: "bar".into(),
            tw: "w-[400px] h-[24px] bg-blue-900 flex items-center".into(),
            style: None,
            children: vec![IncrNode::Text {
                id: "label".into(),
                text: text.into(),
                tw: "text-white text-xs font-mono whitespace-nowrap".into(),
                style: None,
            }],
        };
        SuiteFrame {
            label: format!("frame {i}: «{text}»"),
            root,
        }
    })
    .collect();
    TestSuite {
        name: "Shrink Bug",
        description: "Single text node, no siblings. Wide→narrow transition should erase the right portion — stale pixels here prove the old-bbox bug.",
        frames,
        perf_focused: false,
        force_incremental: true,
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
        let root = IncrNode::Container {
            id: "canvas".into(),
            tw: "flex flex-col w-[400px] h-[80px] bg-gray-900".into(),
            style: None,
            children: vec![
                IncrNode::Container {
                    id: "header".into(),
                    tw: "flex flex-row items-center h-[20px] px-2".into(),
                    style: None,
                    children: vec![
                        IncrNode::Text { id: "title".into(), text:"Ball Track".into(),
                            tw: "text-gray-500 text-[10px] whitespace-nowrap".into(), style: None },
                        IncrNode::Text { id: "pos-lbl".into(), text:format!("x={bx} sz={sz}"),
                            tw: "ml-2 text-gray-400 text-[10px] font-mono whitespace-nowrap".into(), style: None },
                    ],
                },
                IncrNode::Container {
                    id: "track".into(),
                    tw: "flex flex-row items-center flex-1 px-2".into(),
                    style: None,
                    children: vec![
                        IncrNode::Container {
                            id: "spacer".into(),
                            tw: format!("flex-shrink-0 w-[{bx}px] h-[2px] bg-gray-700"),
                            style: None,
                            children: vec![],
                        },
                        IncrNode::Container {
                            id: "ball".into(),
                            tw: format!("flex-shrink-0 w-[{sz}px] h-[{sz}px] rounded-full bg-orange-500 shadow-lg"),
                            style: None,
                            children: vec![],
                        },
                    ],
                },
            ],
        };
        SuiteFrame { label: format!("frame {i}: x={bx} sz={sz}"), root }
    }).collect();
    TestSuite {
        name: "Moving Ball",
        description: "400×80 px. Ball slides L→R; size pulses simultaneously. Tests relocating + resizing node in the same frame.",
        frames,
        perf_focused: false,
        force_incremental: true,
    }
}

fn suite_tile_crossing() -> TestSuite {
    let frames = (0..10).map(|i| {
        let bx = i as u32 * TILE_SIZE;
        let root = IncrNode::Container {
            id: "canvas".into(),
            tw: "flex flex-row items-center w-[320px] h-[64px] bg-gray-900".into(),
            style: None,
            children: vec![
                IncrNode::Container {
                    id: "spacer".into(),
                    tw: format!("flex-shrink-0 w-[{bx}px] h-[2px] bg-gray-700"),
                    style: None,
                    children: vec![],
                },
                IncrNode::Container {
                    id: "block".into(),
                    tw: "flex-shrink-0 w-[32px] h-[32px] bg-cyan-400 rounded shadow-sm flex items-center justify-center".into(),
                    style: None,
                    children: vec![IncrNode::Text {
                        id: "n".into(),
                        text:format!("{i}"),
                        tw: "text-gray-900 text-xs font-bold".into(),
                        style: None,
                    }],
                },
            ],
        };
        SuiteFrame { label: format!("frame {i}: tile-x={}", bx / TILE_SIZE), root }
    }).collect();
    TestSuite {
        name: "Tile Crossing",
        description: "320×64 px. Block advances exactly one tile (32px) per frame. Stresses dirty-tile marking at exact tile boundaries.",
        frames,
        perf_focused: false,
        force_incremental: true,
    }
}

fn suite_panel_focus() -> TestSuite {
    let frames = (0..10).map(|i| {
        let active = i % 3;
        let count = i + 1;
        let root = IncrNode::Container {
            id: "canvas".into(),
            tw: "flex flex-row gap-3 p-4 w-[460px] h-[120px] bg-gray-950".into(),
            style: None,
            children: (0usize..3).map(|idx| {
                let is_active = idx == active;
                IncrNode::Container {
                    id: format!("panel-{idx}"),
                    tw: if is_active {
                        "flex flex-col p-3 bg-blue-800 rounded-xl shadow-xl w-[130px] border-2 border-blue-400".into()
                    } else {
                        "flex flex-col p-3 bg-gray-800 rounded-xl shadow-md w-[130px] border border-gray-600".into()
                    },
                    style: None,
                    children: vec![
                        IncrNode::Text { id: format!("p{idx}-title"),
                            text:["Alpha", "Beta", "Gamma"][idx].into(),
                            tw: format!("text-[11px] font-bold {} whitespace-nowrap",
                                if is_active { "text-blue-100" } else { "text-gray-300" }),
                            style: None },
                        IncrNode::Text { id: format!("p{idx}-val"),
                            text:if idx == 0 { format!("{count}") } else { "—".into() },
                            tw: "text-[22px] font-bold text-white".into(),
                            style: None },
                    ],
                }
            }).collect(),
        };
        SuiteFrame { label: format!("frame {i}: active={active} count={count}"), root }
    }).collect();
    TestSuite {
        name: "Panel Focus Cycle",
        description: "460×120 px. Active panel highlight cycles L→M→R. Counter increments each frame. Tests simultaneous bg-color + content changes.",
        frames,
        perf_focused: false,
        force_incremental: true,
    }
}

fn suite_diagonal_scatter() -> TestSuite {
    let colors = [
        "red", "orange", "yellow", "green", "cyan", "blue", "indigo", "purple", "pink",
    ];
    let frames = (0..10).map(|i| {
        let hot = i % 9;
        let rows: Vec<IncrNode> = (0usize..3).map(|r| IncrNode::Container {
            id: format!("row-{r}"),
            tw: "flex flex-row gap-2".into(),
            style: None,
            children: (0usize..3).map(|c| {
                let idx = r * 3 + c;
                let is_hot = idx == hot;
                IncrNode::Container {
                    id: format!("cell-{idx}"),
                    tw: if is_hot {
                        format!("w-[72px] h-[72px] bg-{}-400 rounded-lg shadow-lg flex items-center justify-center", colors[idx])
                    } else {
                        format!("w-[72px] h-[72px] bg-{}-900 rounded flex items-center justify-center", colors[idx])
                    },
                    style: None,
                    children: vec![IncrNode::Text {
                        id: format!("cell-{idx}-lbl"),
                        text:if is_hot { "●".into() } else { "○".into() },
                        tw: format!("text-{}-{} text-sm font-bold", colors[idx], if is_hot { "100" } else { "600" }),
                        style: None,
                    }],
                }
            }).collect(),
        }).collect();
        let root = IncrNode::Container {
            id: "canvas".into(),
            tw: "flex flex-col gap-2 w-[248px] h-[248px] p-2 bg-gray-950".into(),
            style: None,
            children: rows,
        };
        SuiteFrame { label: format!("frame {i}: hot=cell-{hot}"), root }
    }).collect();
    TestSuite {
        name: "Diagonal Scatter",
        description: "248×248 px. 3×3 grid; one 'hot' cell cycles through all 9 positions per frame. Tests spatially scattered single-cell updates.",
        frames,
        perf_focused: false,
        force_incremental: true,
    }
}

fn suite_notification_badge() -> TestSuite {
    let frames = (0..12).map(|i| {
        let count = i + 1;
        let two_digit = count >= 10;
        let root = IncrNode::Container {
            id: "widget".into(),
            tw: "flex flex-row items-center gap-3 w-[240px] h-[72px] px-4 py-3 bg-gray-900 rounded-xl".into(),
            style: None,
            children: vec![
                IncrNode::Container {
                    id: "icon".into(),
                    tw: "flex-shrink-0 w-[48px] h-[48px] bg-blue-600 rounded-xl shadow-md flex items-center justify-center".into(),
                    style: None,
                    children: vec![IncrNode::Text {
                        id: "icon-lbl".into(),
                        text:"✉".into(),
                        tw: "text-white text-[20px]".into(),
                        style: None,
                    }],
                },
                IncrNode::Container {
                    id: "content".into(),
                    tw: "flex flex-col gap-1".into(),
                    style: None,
                    children: vec![
                        IncrNode::Text { id: "app-name".into(), text:"Messages".into(),
                            tw: "text-[12px] text-white font-semibold whitespace-nowrap".into(), style: None },
                        IncrNode::Container {
                            id: "badge-row".into(),
                            tw: "flex flex-row items-center gap-2".into(),
                            style: None,
                            children: vec![
                                IncrNode::Container {
                                    id: "badge".into(),
                                    tw: format!("flex items-center justify-center {} h-[18px] bg-red-500 rounded-full",
                                        if two_digit { "min-w-[28px]" } else { "min-w-[18px]" }),
                                    style: None,
                                    children: vec![IncrNode::Text {
                                        id: "badge-n".into(),
                                        text:format!("{count}"),
                                        tw: "text-white text-[10px] font-bold px-1 whitespace-nowrap".into(),
                                        style: None,
                                    }],
                                },
                                IncrNode::Text { id: "badge-lbl".into(), text:"unread".into(),
                                    tw: "text-[10px] text-gray-400 whitespace-nowrap".into(), style: None },
                            ],
                        },
                    ],
                },
            ],
        };
        SuiteFrame { label: format!("frame {i}: {count} unread"), root }
    }).collect();
    TestSuite {
        name: "Notification Badge",
        description:
            "240×72 px. Badge counter 1→12; container widens at 2 digits. App icon is fully static.",
        frames,
        perf_focused: false,
        force_incremental: true,
    }
}

fn suite_progress_fill() -> TestSuite {
    let frames = (0..10)
        .map(|i| {
            let pct: u32 = match i {
                0..=7 => (i as f64 / 7.0 * 100.0) as u32,
                8 => 30,
                _ => 0,
            };
            let fill_w = (pct * 320 / 100).min(320);
            let complete = pct >= 100;
            let root = IncrNode::Container {
                id: "card".into(),
                tw: "flex flex-col gap-2 w-[360px] h-[60px] px-3 py-2 bg-gray-800 rounded-xl"
                    .into(),
                style: None,
                children: vec![
                    IncrNode::Container {
                        id: "header".into(),
                        tw: "flex flex-row items-baseline justify-between".into(),
                        style: None,
                        children: vec![
                            IncrNode::Text {
                                id: "label".into(),
                                text: if complete {
                                    "Complete!"
                                } else {
                                    "Downloading…"
                                }
                                .into(),
                                tw: "text-[11px] text-gray-400 whitespace-nowrap".into(),
                                style: None,
                            },
                            IncrNode::Text {
                                id: "pct".into(),
                                text: format!("{pct}%"),
                                tw: "text-[11px] text-white font-mono whitespace-nowrap".into(),
                                style: None,
                            },
                        ],
                    },
                    IncrNode::Container {
                        id: "bar-bg".into(),
                        tw: "w-full h-[8px] bg-gray-700 rounded-full overflow-hidden".into(),
                        style: None,
                        children: vec![IncrNode::Container {
                            id: "bar-fill".into(),
                            tw: format!(
                                "w-[{fill_w}px] h-[8px] bg-{}",
                                if complete { "green-400" } else { "blue-500" }
                            ),
                            style: Some(serde_json::json!({"display":"inline-block"})),
                            children: vec![],
                        }],
                    },
                ],
            };
            SuiteFrame {
                label: format!("frame {i}: {pct}%"),
                root,
            }
        })
        .collect();
    TestSuite {
        name: "Progress Fill",
        description: "360×60 px. Image bar grows 0→100%; color flips to green at completion; frames 8-9 reset. Tests Image node resize + color change.",
        frames,
        perf_focused: false,
        force_incremental: true,
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

        let root = IncrNode::Container {
            id: "canvas".into(),
            tw: "flex flex-col w-[500px] h-[260px] p-4 bg-gray-950 gap-3".into(),
            style: None,
            children: vec![
                // Header: title + frame counter + current phase
                IncrNode::Container { id: "hdr".into(), tw: "flex flex-row items-baseline gap-2".into(),
                    style: None,
                    children: vec![
                        IncrNode::Text { id: "hdr-title".into(), text:"Keyframe Animation".into(),
                            tw: "text-[12px] text-gray-300 font-bold whitespace-nowrap".into(), style: None },
                        IncrNode::Text { id: "hdr-frame".into(), text:format!("{i:02}/20"),
                            tw: "text-[10px] text-gray-500 font-mono whitespace-nowrap".into(), style: None },
                        IncrNode::Text { id: "hdr-phase".into(), text:phase.into(),
                            tw: "ml-auto text-[11px] text-yellow-300 font-bold whitespace-nowrap".into(), style: None },
                    ],
                },
                // Bounce row
                IncrNode::Container { id: "bounce-row".into(),
                    tw: "flex flex-row items-start gap-3 h-[60px]".into(),
                    style: None,
                    children: vec![
                        IncrNode::Text { id: "bounce-lbl".into(), text:"Bounce".into(),
                            tw: "w-[46px] text-[10px] text-gray-500 whitespace-nowrap pt-1 flex-shrink-0".into(), style: None },
                        IncrNode::Container { id: "bounce-track".into(),
                            tw: "flex-1 h-[60px] bg-gray-900 rounded overflow-hidden".into(),
                            style: None,
                            children: vec![IncrNode::Container { id: "bounce-col".into(),
                                tw: "flex flex-col pl-2".into(),
                                style: None,
                                children: vec![
                                    IncrNode::Container { id: "bounce-spacer".into(),
                                        tw: format!("flex-shrink-0 w-[20px] h-[{bounce_y}px]"),
                                        style: None,
                                        children: vec![] },
                                    IncrNode::Container { id: "bounce-ball".into(),
                                        tw: "flex-shrink-0 w-[20px] h-[20px] rounded-full bg-blue-400 shadow-md".into(),
                                        style: None,
                                        children: vec![] },
                                ],
                            }],
                        },
                    ],
                },
                // Slide row
                IncrNode::Container { id: "slide-row".into(),
                    tw: "flex flex-row items-center gap-3 h-[32px]".into(),
                    style: None,
                    children: vec![
                        IncrNode::Text { id: "slide-lbl".into(), text:"Slide".into(),
                            tw: "w-[46px] text-[10px] text-gray-500 whitespace-nowrap flex-shrink-0".into(), style: None },
                        IncrNode::Container { id: "slide-track".into(),
                            tw: "flex-1 h-[8px] bg-gray-800 rounded-full flex flex-row items-center".into(),
                            style: None,
                            children: vec![
                                IncrNode::Container { id: "slide-spacer".into(),
                                    tw: format!("flex-shrink-0 w-[{slide_x}px] h-[8px]"),
                                    style: None,
                                    children: vec![] },
                                IncrNode::Container { id: "slide-thumb".into(),
                                    tw: "flex-shrink-0 w-[12px] h-[12px] rounded-full bg-white shadow-sm".into(),
                                    style: None,
                                    children: vec![] },
                            ],
                        },
                    ],
                },
                // Pulse row
                IncrNode::Container { id: "pulse-row".into(),
                    tw: "flex flex-row items-center gap-3 h-[44px]".into(),
                    style: None,
                    children: vec![
                        IncrNode::Text { id: "pulse-lbl".into(), text:"Pulse".into(),
                            tw: "w-[46px] text-[10px] text-gray-500 whitespace-nowrap flex-shrink-0".into(), style: None },
                        IncrNode::Container { id: "pulse-box".into(),
                            tw: format!("flex-shrink-0 w-[{pulse_sz}px] h-[{pulse_sz}px] bg-{pulse_color} rounded shadow-md"),
                            style: None,
                            children: vec![] },
                    ],
                },
                // Phase indicator row: 4 segments, active one highlighted
                IncrNode::Container { id: "phase-row".into(),
                    tw: "flex flex-row items-center gap-3 h-[24px]".into(),
                    style: None,
                    children: vec![
                        IncrNode::Text { id: "phase-lbl".into(), text:"Phase".into(),
                            tw: "w-[46px] text-[10px] text-gray-500 whitespace-nowrap flex-shrink-0".into(), style: None },
                        IncrNode::Container { id: "phase-bar".into(),
                            tw: "flex-1 flex flex-row gap-1 h-[16px]".into(),
                            style: None,
                            children: ["IDLE","RISING","PEAK","FALLING"].iter().map(|&p| IncrNode::Container {
                                id: format!("phase-seg-{p}"),
                                tw: if p == phase {
                                    "flex-1 h-full bg-yellow-400 rounded-sm".into()
                                } else {
                                    "flex-1 h-full bg-gray-700 rounded-sm".into()
                                },
                                style: None,
                                children: vec![],
                            }).collect(),
                        },
                    ],
                },
            ],
        };
        SuiteFrame {
            label: format!("frame {i:02}: bounce={bounce_y} slide={slide_x} pulse={pulse_sz} phase={phase}"),
            root,
        }
    }).collect();
    TestSuite {
        name: "Keyframe Animation",
        description: "500×260 px. 20 frames: bouncing ball, sliding thumb, pulsing colored box, 4-phase indicator. Each element follows an independent curve.",
        frames,
        perf_focused: false,
        force_incremental: true,
    }
}

// ---------------------------------------------------------------------------
// Notification panel — realistic timed-update suite
// ---------------------------------------------------------------------------

fn suite_notification_panel() -> TestSuite {
    let spinner = ["|", "/", "—", "\\"];
    let notifs = [
        ("System", "Software update available"),
        ("Messages", "3 unread from Alice"),
        ("Build", "costae release passed"),
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

        let notif_items: Vec<IncrNode> = notifs.iter().enumerate().map(|(idx, (app, msg))| {
            let hot = idx == active;
            IncrNode::Container {
                id: format!("notif-{idx}"),
                tw: if hot {
                    "flex flex-row items-center gap-2 px-3 py-2 bg-blue-950 rounded-lg".into()
                } else {
                    "flex flex-row items-center gap-2 px-3 py-2".into()
                },
                style: None,
                children: vec![
                    IncrNode::Container {
                        id: format!("notif-{idx}-dot"),
                        tw: if hot {
                            "flex-shrink-0 w-[8px] h-[8px] rounded-full bg-blue-400".into()
                        } else {
                            "flex-shrink-0 w-[8px] h-[8px] rounded-full bg-gray-700".into()
                        },
                        style: None,
                        children: vec![],
                    },
                    IncrNode::Container {
                        id: format!("notif-{idx}-body"),
                        tw: "flex flex-col".into(),
                        style: None,
                        children: vec![
                            IncrNode::Text {
                                id: format!("notif-{idx}-app"),
                                text:app.to_string(),
                                tw: format!("text-[11px] font-bold whitespace-nowrap {}",
                                    if hot { "text-blue-300" } else { "text-gray-500" }),
                                style: None,
                            },
                            IncrNode::Text {
                                id: format!("notif-{idx}-msg"),
                                text:msg.to_string(),
                                tw: format!("text-[10px] whitespace-nowrap {}",
                                    if hot { "text-gray-200" } else { "text-gray-600" }),
                                style: None,
                            },
                        ],
                    },
                ],
            }
        }).collect();

        let root = IncrNode::Container {
            id: "panel".into(),
            tw: "flex flex-col w-[400px] h-[200px] bg-gray-900 rounded-xl p-3 gap-2".into(),
            style: None,
            children: vec![
                // Header: static title + badge (every 4th) + spinner (every frame)
                IncrNode::Container {
                    id: "hdr".into(),
                    tw: "flex flex-row items-center gap-2 h-[24px]".into(),
                    style: None,
                    children: vec![
                        IncrNode::Text { id: "title".into(), text:"NOTIFICATIONS".into(),
                            tw: "text-[10px] text-gray-400 font-bold whitespace-nowrap".into(), style: None },
                        IncrNode::Container { id: "hdr-gap".into(), tw: "flex-1".into(), style: None, children: vec![] },
                        IncrNode::Container {
                            id: "badge".into(),
                            tw: "flex-shrink-0 flex items-center justify-center w-[18px] h-[18px] bg-red-500 rounded-full".into(),
                            style: None,
                            children: vec![IncrNode::Text {
                                id: "badge-n".into(), text:format!("{count}"),
                                tw: "text-white text-[10px] font-bold".into(),
                                style: None,
                            }],
                        },
                        IncrNode::Text {
                            id: "spin".into(), text:spin.into(),
                            tw: "ml-auto text-blue-400 text-[14px] font-mono whitespace-nowrap".into(),
                            style: None,
                        },
                    ],
                },
                // Notification list — one item highlights on a 2-frame cycle
                IncrNode::Container {
                    id: "notif-list".into(),
                    tw: "flex flex-col gap-1".into(),
                    style: None,
                    children: notif_items,
                },
                // Slide track — thumb bounces L↔R every frame
                IncrNode::Container {
                    id: "slide-row".into(),
                    tw: "flex flex-row items-center gap-2 h-[20px]".into(),
                    style: None,
                    children: vec![
                        IncrNode::Text { id: "slide-lbl".into(), text:"activity".into(),
                            tw: "w-[46px] text-[10px] text-gray-600 whitespace-nowrap flex-shrink-0".into(), style: None },
                        IncrNode::Container {
                            id: "slide-track".into(),
                            tw: "flex-1 h-[4px] bg-gray-800 rounded-full flex flex-row items-center overflow-hidden".into(),
                            style: None,
                            children: vec![
                                IncrNode::Container {
                                    id: "slide-spacer".into(),
                                    tw: format!("flex-shrink-0 w-[{slide_x}px] h-[4px]"),
                                    style: None,
                                    children: vec![],
                                },
                                IncrNode::Container {
                                    id: "slide-thumb".into(),
                                    tw: "flex-shrink-0 w-[8px] h-[8px] rounded-full bg-blue-400".into(),
                                    style: None,
                                    children: vec![],
                                },
                            ],
                        },
                    ],
                },
            ],
        };

        SuiteFrame {
            label: format!("frame {i:02}: spin={spin} notif={active} count={count} slide={slide_x}"),
            root,
        }
    }).collect();

    TestSuite {
        name: "Notification Panel",
        description: "400×200 px. Spinner every frame; active notification rotates every 2; badge count every 4; slide thumb bounces L↔R. Most content static.",
        frames,
        perf_focused: true,
        force_incremental: false,
    }
}

// ---------------------------------------------------------------------------
// Compositing overlay — backdrop-blur, opacity, semi-transparent backgrounds
// ---------------------------------------------------------------------------

// Compositing correctness test with genuine content behind the glass.
//
// Layout: the canvas is `relative`.  Eight metric cards fill the full area in
// normal flow (left half + right half).  A frosted-glass panel (`absolute`,
// `backdrop-blur-md`, `bg-white/10`) is positioned over the RIGHT HALF only.
//
// Left half:  cards visible directly — values change every frame (dirty tiles).
// Right half: same cards visible THROUGH the static glass panel — the blur
//             must sample the freshly-rendered card pixels each frame.
//
// If the glass-panel tiles are served from a stale cache entry, the blurred
// ---------------------------------------------------------------------------
// Two-region — greedy merge exercise
// ---------------------------------------------------------------------------

/// 440×200 px.  Left panel (clock + 4 metrics) updates every frame.  Right
/// panel (system log) updates every 4th frame.  The 80 px gap between the
/// panels is wider than MERGE_THRESHOLD (2 tiles = 64 px), so on frames where
/// both panels are dirty two genuinely separate candidates are produced and the
/// greedy merge must decide whether to combine them.
fn suite_two_region() -> TestSuite {
    let log_entries = [
        "INFO  kernel: usb 1-1 attached",
        "WARN  sshd: auth failed 10.0.0.5",
        "INFO  systemd: nginx started",
        "ERR   disk: I/O error /dev/sda",
        "INFO  net: link up eth0 1G",
        "WARN  oom: killed process 1842",
        "INFO  cron: mail.daily started",
    ];
    let frames = (0..25)
        .map(|i| {
            let time = format!("14:30:{i:02}");
            let cpu = format!("CPU  {}%", 20 + (i * 7) % 60);
            let mem = format!("MEM  {:.1}G", 3.0 + (i as f32 * 0.13).sin() * 0.8);
            let net = format!("NET  {} MB/s", i * 3 % 100);
            let log = log_entries[(i / 4) % log_entries.len()];
            let age = format!("{}s ago", (i % 4) * 10);
            let root = IncrNode::Container {
                id: "canvas".into(),
                tw: "relative w-[440px] h-[200px] bg-gray-950".into(),
                style: None,
                children: vec![
                    // Left panel — every frame
                    IncrNode::Container {
                        id: "left".into(),
                        tw: "absolute bottom-[8px] left-[8px] w-[180px] h-[184px] \
                             bg-gray-900 rounded-xl p-3 flex flex-col gap-2"
                            .into(),
                        style: None,
                        children: vec![
                            IncrNode::Text {
                                id: "time".into(),
                                text: time.clone(),
                                tw: "text-white text-sm font-mono font-bold whitespace-nowrap"
                                    .into(),
                                style: None,
                            },
                            IncrNode::Text {
                                id: "cpu".into(),
                                text: cpu,
                                tw: "text-green-400 text-xs font-mono whitespace-nowrap".into(),
                                style: None,
                            },
                            IncrNode::Text {
                                id: "mem".into(),
                                text: mem,
                                tw: "text-blue-400 text-xs font-mono whitespace-nowrap".into(),
                                style: None,
                            },
                            IncrNode::Text {
                                id: "net".into(),
                                text: net,
                                tw: "text-yellow-400 text-xs font-mono whitespace-nowrap".into(),
                                style: None,
                            },
                        ],
                    },
                    // Right panel — every 4th frame  (left=252 = 440-180-8)
                    IncrNode::Container {
                        id: "right".into(),
                        tw: "absolute bottom-[8px] left-[252px] w-[180px] h-[184px] \
                             bg-gray-900 rounded-xl p-3 flex flex-col gap-2"
                            .into(),
                        style: None,
                        children: vec![
                            IncrNode::Text {
                                id: "log-hdr".into(),
                                text: "System Log".into(),
                                tw: "text-gray-400 text-[10px] font-bold whitespace-nowrap".into(),
                                style: None,
                            },
                            IncrNode::Text {
                                id: "log-entry".into(),
                                text: log.into(),
                                tw: "text-green-300 text-[9px] font-mono whitespace-nowrap".into(),
                                style: None,
                            },
                            IncrNode::Text {
                                id: "log-age".into(),
                                text: age,
                                tw: "text-gray-600 text-[9px] font-mono whitespace-nowrap".into(),
                                style: None,
                            },
                        ],
                    },
                ],
            };
            SuiteFrame {
                label: format!("f{i}: t={time} log={}", i / 4),
                root,
            }
        })
        .collect();
    TestSuite {
        name: "Two Region",
        description: "440×200 px. Left panel (clock+metrics) dirty every frame; right panel \
                      (system log) dirty every 4th frame. 80 px gap forces two separate \
                      candidates when both are active — primary exercise for the greedy merge.",
        frames,
        perf_focused: false,
        force_incremental: true,
    }
}

// ---------------------------------------------------------------------------
// Kanban board — structural changes, movement, compositing overlay
// ---------------------------------------------------------------------------

/// 440×300 px kanban with three columns.  Cards appear, disappear, and move
/// between columns at keyframe boundaries (ManagedSet structural changes).
/// A semi-transparent ball with `backdrop-blur-md` and `shadow-2xl` bounces
/// over the board every frame — its absolute position changes each frame,
/// exercising compositing + moved-node detection simultaneously.
fn suite_kanban() -> TestSuite {
    use std::f32::consts::PI;

    // (id, title, left-border colour)
    let task_def: &[(&str, &str, &str)] = &[
        ("A", "Implement auth", "border-red-400"),
        ("B", "Fix login bug", "border-purple-400"),
        ("C", "Add dark mode", "border-yellow-400"),
        ("D", "Write unit tests", "border-green-400"),
        ("E", "Perf audit", "border-orange-400"),
        ("F", "Code review", "border-blue-400"),
    ];

    let make_card = |id: &str| -> IncrNode {
        let &(_, title, border) = task_def.iter().find(|(i, _, _)| *i == id).unwrap();
        IncrNode::Container {
            id: format!("card-{id}"),
            tw: format!(
                "flex flex-col gap-[2px] px-2 py-[6px] bg-gray-800 rounded-lg \
                 border-l-2 {border} shadow-md"
            ),
            style: None,
            children: vec![
                IncrNode::Text {
                    id: format!("card-{id}-t"),
                    text: title.into(),
                    tw: "text-white text-[10px] font-bold whitespace-nowrap".into(),
                    style: None,
                },
                IncrNode::Text {
                    id: format!("card-{id}-i"),
                    text: format!("#{id}"),
                    tw: "text-gray-500 text-[9px] font-mono whitespace-nowrap".into(),
                    style: None,
                },
            ],
        }
    };

    let make_col = |col_id: &str, title: &str, cards: Vec<IncrNode>| -> IncrNode {
        let n = cards.len();
        let mut children = vec![IncrNode::Container {
            id: format!("{col_id}-hdr"),
            tw: "flex flex-row items-center justify-between px-1".into(),
            style: None,
            children: vec![
                IncrNode::Text {
                    id: format!("{col_id}-title"),
                    text: title.into(),
                    tw: "text-gray-300 text-[10px] font-bold whitespace-nowrap".into(),
                    style: None,
                },
                IncrNode::Text {
                    id: format!("{col_id}-n"),
                    text: n.to_string(),
                    tw: "text-gray-500 text-[10px] font-mono whitespace-nowrap".into(),
                    style: None,
                },
            ],
        }];
        children.extend(cards);
        IncrNode::Container {
            id: col_id.into(),
            tw: "flex flex-col w-[132px] h-[284px] bg-gray-900 rounded-xl \
                 p-[6px] gap-[6px]"
                .into(),
            style: None,
            children,
        }
    };

    // Column assignment keyframes: (start_frame, todo, wip, done)
    #[allow(clippy::type_complexity)]
    let phases: &[(usize, &[&str], &[&str], &[&str])] = &[
        (0, &["A", "B", "C"], &["D"], &[]),
        (8, &["B", "C"], &["A", "D"], &[]),
        (14, &["C"], &["A", "B"], &["D"]),
        (20, &["C"], &["B"], &["A", "D"]),
        (25, &["C", "E"], &["B"], &["A", "D"]),
        (30, &["E"], &["B", "C"], &["A", "D"]),
        (35, &["E"], &["C"], &["A", "B", "D"]),
    ];

    let frames = (0..40)
        .map(|i| {
            // current column assignment
            let &(_, todo_ids, wip_ids, done_ids) = phases
                .iter()
                .rev()
                .find(|&&(start, _, _, _)| i >= start)
                .unwrap();

            // Ball: bounces diagonally across the board.
            // Use bottom-[N] + left-[N] (top doesn't work in takumi).
            let t = i as f32;
            let ball_x = (180.0 + 150.0 * (t * 0.18 * PI).sin()) as u32;
            // ball_y_top: distance from canvas top (0..220 for 80px ball in 300px canvas)
            let ball_y_top = (80.0 + 90.0 * (t * 0.27 * PI).sin().abs()) as u32;
            let ball_bottom = 300u32.saturating_sub(ball_y_top + 80);

            let root = IncrNode::Container {
                id: "canvas".into(),
                tw: "relative flex flex-row gap-[8px] p-[8px] \
                     w-[440px] h-[300px] bg-gray-950"
                    .into(),
                style: None,
                children: vec![
                    make_col(
                        "col-todo",
                        "TODO",
                        todo_ids.iter().map(|&id| make_card(id)).collect(),
                    ),
                    make_col(
                        "col-wip",
                        "IN PROGRESS",
                        wip_ids.iter().map(|&id| make_card(id)).collect(),
                    ),
                    make_col(
                        "col-done",
                        "DONE",
                        done_ids.iter().map(|&id| make_card(id)).collect(),
                    ),
                    // Bouncing overlay ball — position changes every frame
                    IncrNode::Container {
                        id: "ball".into(),
                        tw: format!(
                            "absolute bottom-[{ball_bottom}px] left-[{ball_x}px] \
                             w-[80px] h-[80px] rounded-full opacity-60 bg-gray-400 \
                             backdrop-blur-md shadow-2xl border border-gray-300"
                        ),
                        style: None,
                        children: vec![],
                    },
                ],
            };
            SuiteFrame {
                label: format!(
                    "f{i}: todo={} wip={} done={} ball=({ball_x},{ball_bottom})",
                    todo_ids.len(),
                    wip_ids.len(),
                    done_ids.len()
                ),
                root,
            }
        })
        .collect();

    TestSuite {
        name: "Kanban Board",
        description: "440×300 px. Three columns; cards appear, disappear, and move between \
                      columns at keyframes (ManagedSet structural changes + moved-node detection). \
                      A backdrop-blur-md semi-transparent ball with shadow-2xl bounces across \
                      the board every frame — compositing + absolute-position stress test.",
        frames,
        perf_focused: true,
        force_incremental: false,
    }
}

/// values on the right will lag one or more frames behind the crisp left half —
/// immediately visible in the diff image.
///
/// An opacity-pulsing badge in the bottom-left also cycles each frame to exercise
/// `opacity-*` dirty marking independently of the blur.
fn suite_compositing_overlay() -> TestSuite {
    let frames = (0..10)
        .map(|i| {
            // Metric values that change every frame — these are what the blur samples.
            let metrics = [
                ("CPU", format!("{}%", 12 + i * 7)),
                ("GPU", format!("{}%", 60 + i * 3)),
                ("MEM", format!("{}.{}G", 3 + i / 3, i % 3)),
                ("NET↑", format!("{}M", i * 13)),
                ("TEMP", format!("{}°C", 55 + i * 2)),
                ("DISK", format!("{}%", 40 + i)),
                ("FPS", format!("{}", 60 - i * 2)),
                ("BAT", format!("{}%", 90 - i * 3)),
            ];

            let badge_opacity = if i % 2 == 0 {
                "opacity-100"
            } else {
                "opacity-20"
            };
            let spinner = ["|", "/", "—", "\\"][i % 4];

            // 8 metric cards in a 4×2 grid filling the canvas — all change every frame.
            let cards: Vec<IncrNode> = metrics
                .iter()
                .map(|(name, val)| IncrNode::Container {
                    id: format!("card-{name}"),
                    tw: "flex flex-col items-center justify-center w-[96px] h-[64px] \
                     bg-gray-800 rounded-lg shadow-lg"
                        .into(),
                    style: None,
                    children: vec![
                        IncrNode::Text {
                            id: format!("card-{name}-lbl"),
                            text: name.to_string(),
                            tw: "text-gray-400 text-[10px] whitespace-nowrap".into(),
                            style: None,
                        },
                        IncrNode::Text {
                            id: format!("card-{name}-val"),
                            text: val.clone(),
                            tw: "text-white text-sm font-mono font-bold whitespace-nowrap".into(),
                            style: None,
                        },
                    ],
                })
                .collect();

            let root = IncrNode::Container {
                id: "canvas".into(),
                // relative so the absolute glass panel is positioned within this container.
                tw: "relative w-[440px] h-[160px] bg-gray-950".into(),
                style: None,
                children: vec![
                    // All 8 metric cards in a 4×2 flex-wrap grid — fill the full canvas.
                    IncrNode::Container {
                        id: "grid".into(),
                        tw: "flex flex-row flex-wrap gap-[8px] p-[8px] w-[440px] h-[160px]".into(),
                        style: None,
                        children: cards,
                    },
                    // Semi-transparent overlay — ABSOLUTE, covers right half only (x=220 to x=432).
                    // STATIC: tw never changes after frame 0.  The cards underneath change every
                    // frame; the overlay must composite correctly over freshly-rendered card tiles.
                    IncrNode::Container {
                        id: "glass".into(),
                        tw: "absolute bottom-[8px] left-[220px] w-[212px] h-[144px] \
                         opacity-40 bg-blue-400 backdrop-blur-md mix-blend-screen \
                         rounded-xl border border-blue-300 shadow-xl \
                         flex flex-col items-center justify-center gap-1"
                            .into(),
                        style: None,
                        children: vec![
                            IncrNode::Text {
                                id: "glass-lbl".into(),
                                text: "Overlay (static)".into(),
                                tw: "text-white text-xs font-bold whitespace-nowrap".into(),
                                style: None,
                            },
                            IncrNode::Text {
                                id: "glass-hint".into(),
                                text: "opacity-70".into(),
                                tw: "text-white text-[10px] whitespace-nowrap".into(),
                                style: None,
                            },
                        ],
                    },
                    // Opacity-pulsing badge — bottom-left, changes every frame independently.
                    IncrNode::Container {
                        id: "badge".into(),
                        tw: format!(
                            "absolute bottom-[8px] left-[8px] flex items-center \
                                 justify-center gap-1 px-2 h-[18px] bg-yellow-400 \
                                 rounded {badge_opacity}"
                        ),
                        style: None,
                        children: vec![
                            IncrNode::Text {
                                id: "spin".into(),
                                text: spinner.into(),
                                tw: "text-black text-[10px] font-mono".into(),
                                style: None,
                            },
                            IncrNode::Text {
                                id: "badge-lbl".into(),
                                text: "LIVE".into(),
                                tw: "text-black text-[10px] font-bold".into(),
                                style: None,
                            },
                        ],
                    },
                ],
            };
            SuiteFrame {
                label: format!(
                    "frame {i}: cpu={} gpu={} badge={badge_opacity}",
                    metrics[0].1, metrics[1].1
                ),
                root,
            }
        })
        .collect();

    TestSuite {
        name: "Compositing Overlay",
        description: "440×160 px. 8 metric cards fill the canvas (all change every frame). \
                       Static semi-transparent overlay (absolute, bg-black/50) covers the right \
                       half. Left half: cards visible directly. Right half: cards visible through \
                       the dark overlay. Opacity badge pulses bottom-left. Tests that the static \
                       overlay composites correctly over freshly re-rendered card tiles.",
        frames,
        perf_focused: false,
        force_incremental: true,
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
        ("System", "Software update available", "blue"),
        ("Messages", "3 unread from Alice", "purple"),
        ("Build", "costae release passed", "green"),
        ("Monitor", "CPU spike: 94% for 30s", "red"),
        ("Sync", "14 files synced", "teal"),
        ("Calendar", "Meeting in 15 min", "orange"),
    ];

    let frames = (0..20)
        .map(|i| {
            let scroll_y = i as u32 * 2; // 0, 2, 4 … 38 px total

            // Monitor thumbnail cycles between two images every 5 frames.
            let monitor_src = if (i / 5) % 2 == 0 {
                "great-wave"
            } else {
                "water-lilies"
            };

            let items: Vec<IncrNode> = notif_data
                .iter()
                .enumerate()
                .map(|(idx, (app, msg, color))| {
                    // Items 1 (Messages), 3 (Monitor), 5 (Calendar) get a 48×48 Photo thumbnail.
                    let photo_src: Option<&str> = match idx {
                        1 => Some("pearl-earring"),
                        3 => Some(monitor_src),
                        5 => Some("starry-night"),
                        _ => None,
                    };

                    let mut children: Vec<IncrNode> = Vec::new();

                    if let Some(src) = photo_src {
                        children.push(IncrNode::Image {
                            id: format!("item-{idx}-thumb"),
                            src: src.to_string(),
                            width: Some(48.0),
                            height: Some(48.0),
                            tw: String::new(),
                            style: None,
                        });
                    }

                    // Dot is smaller (6×6) for photo items, same for plain items.
                    children.push(IncrNode::Container {
                        id: format!("item-{idx}-dot"),
                        tw: format!("flex-shrink-0 w-[6px] h-[6px] rounded-full bg-{color}-400"),
                        style: None,
                        children: vec![],
                    });

                    children.push(IncrNode::Container {
                        id: format!("item-{idx}-body"),
                        tw: "flex flex-col".into(),
                        style: None,
                        children: vec![
                            IncrNode::Text {
                                id: format!("item-{idx}-app"),
                                text: app.to_string(),
                                tw: format!(
                                    "text-[11px] font-bold text-{color}-300 whitespace-nowrap"
                                ),
                                style: None,
                            },
                            IncrNode::Text {
                                id: format!("item-{idx}-msg"),
                                text: msg.to_string(),
                                tw: "text-[10px] text-gray-400 whitespace-nowrap".into(),
                                style: None,
                            },
                        ],
                    });

                    IncrNode::Container {
                        id: format!("item-{idx}"),
                        tw: format!(
                            "flex flex-row items-center gap-2 px-3 py-2 bg-{color}-950 rounded-lg"
                        ),
                        style: None,
                        children,
                    }
                })
                .collect();

            // scroll-content shifts up via negative margin-top; overflow-hidden clips the top.
            let content_tw = if scroll_y == 0 {
                "flex flex-col gap-1".into()
            } else {
                format!("flex flex-col gap-1 mt-[-{scroll_y}px]")
            };

            let root = IncrNode::Container {
                id: "panel".into(),
                tw: "flex flex-col w-[400px] h-[200px] bg-gray-900 rounded-xl p-3 gap-2".into(),
                style: None,
                children: vec![
                    // Header — static except for scroll position readout
                    IncrNode::Container {
                        id: "hdr".into(),
                        tw: "flex flex-row items-center h-[24px]".into(),
                        style: None,
                        children: vec![
                            IncrNode::Text {
                                id: "hdr-title".into(),
                                text: "NOTIFICATIONS".into(),
                                tw: "text-[10px] text-gray-400 font-bold whitespace-nowrap".into(),
                                style: None,
                            },
                            IncrNode::Text {
                                id: "hdr-pos".into(),
                                text: format!("↕ {scroll_y}px"),
                                tw: "ml-auto text-[10px] text-gray-600 font-mono whitespace-nowrap"
                                    .into(),
                                style: None,
                            },
                        ],
                    },
                    // Clipped scroll viewport — overflow-hidden clips scrolled-past content
                    IncrNode::Container {
                        id: "scroll-win".into(),
                        tw: "flex-1 overflow-hidden".into(),
                        style: None,
                        children: vec![IncrNode::Container {
                            id: "scroll-content".into(),
                            tw: content_tw,
                            style: None,
                            children: items,
                        }],
                    },
                ],
            };

            SuiteFrame {
                label: format!("frame {i:02}: scroll={scroll_y}px"),
                root,
            }
        })
        .collect();

    TestSuite {
        name: "Scroll List",
        description: "400×200 px. 6 items scroll 2px/frame via negative margin-top inside overflow-hidden. Every item moves every frame → all viewport tiles dirty → no incremental savings expected.",
        frames,
        perf_focused: false,
        force_incremental: true,
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn print_suite_results(result: &SuiteResult, tc: &TileConfig) {
    let mut s_full = Duration::ZERO;
    let mut s_incr = Duration::ZERO;
    for (i, f) in result.frames.iter().enumerate() {
        let d = if !f.incr_px.is_empty() && f.incr_px.len() == f.full_px.len() {
            // Restrict both error and changed-area to dirty tiles only.
            // Skipped tiles are untouched by the incremental renderer and must
            // not contribute false positives from layout-reflow-induced AA drift.
            let mask = dirty_mask(&f.dirty_tiles, f.w, f.h, tc);
            let chg_w = if f.prev_full_px.len() == f.full_px.len() {
                diff_masked(&f.prev_full_px, &f.full_px, &mask, f.w, f.h).weighted
            } else {
                (f.dirty_tiles.len() * (tc.tile_size * tc.tile_size) as usize) as f64
            };
            let err = diff_masked(&f.full_px, &f.incr_px, &mask, f.w, f.h);
            let ratio = err.weighted / chg_w.max(1.0);
            if ratio < PERFECT_THRESHOLD {
                "✓".into()
            } else {
                format!(
                    "≠{:.1}% (err={:.1}/chg={:.0})",
                    ratio * 100.0,
                    err.weighted,
                    chg_w
                )
            }
        } else {
            "?".into()
        };
        let bailout_tag = match f.bailout_stage {
            Some(1) => " [S1]",
            Some(2) => " [S2]",
            _ => "",
        };
        println!(
            "  [{: >2}] full={:.1}ms incr={:.1}ms ×{:.1} rendered={} hits={} skip={} {}{}  {}",
            i,
            f.full_time.as_secs_f64() * 1000.0,
            f.incr_time.as_secs_f64() * 1000.0,
            f.full_time.as_secs_f64() / f.incr_time.as_secs_f64().max(1e-9),
            f.render_calls,
            f.cache_hits,
            f.skipped,
            d,
            bailout_tag,
            f.label
        );
        if i > 0 {
            s_full += f.full_time;
            s_incr += f.incr_time;
        }
    }
    println!(
        "  → suite speedup: {:.1}×\n",
        s_full.as_secs_f64() / s_incr.as_secs_f64().max(1e-9)
    );
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
        suite_compositing_overlay(),
        suite_two_region(),
        suite_kanban(),
    ];

    let tc = TileConfig::new(TILE_SIZE);
    let mut cal_samples: Vec<(f64, f64, f64)> = Vec::new();
    let cm = CostModel::default();
    let mut suite_refs: Vec<&TestSuite> = Vec::new();
    let mut results: Vec<SuiteResult> = Vec::new();

    for suite in &suites_defs {
        let dprs: &[f32] = if suite.perf_focused { &[1.0, 2.0] } else { &[1.0] };
        for &dpr in dprs {
            let dpr_label = if suite.perf_focused {
                format!(" ({}×)", dpr)
            } else {
                String::new()
            };
            eprintln!(
                "Running suite: {}{} ({} frames, tile={}px)...",
                suite.name, dpr_label, suite.frames.len(), tc.tile_size
            );
            // force_incremental=true suites run with allow_bailout=false (forced pipeline,
            // for correctness). force_incremental=false suites use allow_bailout=true
            // (heuristics active, for performance).
            let result = run_suite(suite, &cm, dpr, &mut cal_samples, &tc, !suite.force_incremental);
            print_suite_results(&result, &tc);
            suite_refs.push(suite);
            results.push(result);
        }
    }

    // ── Heuristic pass for force_incremental=true suites ────────────────────
    // For suites that ran with allow_bailout=false (forced pipeline), run them
    // again with allow_bailout=true so we can report bail-out timing alongside
    // the correctness results.  Stored as Option<SuiteResult> — None for suites
    // that already ran with heuristics (force_incremental=false).
    let mut heuristic_results: Vec<Option<SuiteResult>> = Vec::new();
    for (suite_idx, suite) in suites_defs.iter().enumerate() {
        if suite.force_incremental {
            let dprs: &[f32] = if suite.perf_focused { &[1.0, 2.0] } else { &[1.0] };
            for &dpr in dprs {
                eprintln!(
                    "Heuristic pass: {}{} ({} frames)...",
                    suite.name,
                    if suite.perf_focused { format!(" ({}×)", dpr) } else { String::new() },
                    suite.frames.len()
                );
                let hr = run_suite(suite, &cm, dpr, &mut vec![], &tc, true);
                // Print bail-out summary
                let bailouts: Vec<_> = hr.frames.iter()
                    .enumerate()
                    .filter_map(|(i, f)| f.bailout_stage.map(|s| (i, s)))
                    .collect();
                if bailouts.is_empty() {
                    eprintln!("  → no bail-outs fired");
                } else {
                    for (i, stage) in &bailouts {
                        eprintln!("  → frame {} bailed out at Stage {}", i, stage);
                    }
                }
                let _ = suite_idx; // reference to suppress unused warning
                heuristic_results.push(Some(hr));
            }
        } else {
            heuristic_results.push(None);
        }
    }

    // ── OLS recalibration ────────────────────────────────────────────────────
    let cal_result = fit_cost_model(&cal_samples);
    match &cal_result {
        Some(c) => eprintln!(
            "\nOLS calibration ({} samples, R²={:.3}):\n  \
             O_FIXED_MS = {:.4}  K_AREA = {:.3e}  K_NODES = {:.4}\n  \
             Update the three const lines near the top of main.rs if these differ significantly.",
            c.n_samples, c.r_squared,
            c.model.o_fixed, c.model.k_area, c.model.k_nodes
        ),
        None => eprintln!("\nOLS calibration: insufficient samples ({}).", cal_samples.len()),
    }

    let path = "/tmp/poc_report.html";
    std::fs::write(path, html_report(&suite_refs, &results, &tc)).expect("write report");
    eprintln!("Report: file://{path}");

    // ── Tile size sweep ──────────────────────────────────────────────────────
    // Run only perf-focused suites at both DPRs for each tile size.
    let perf_suites: Vec<&TestSuite> = suites_defs.iter().filter(|s| s.perf_focused).collect();
    let _sweep_cm = cal_result.map(|c| c.model).unwrap_or_default();

    struct SweepRow {
        tile_size: u32,
        overall_speedup: f64,
        perf_speedup: f64,
        n_samples: usize,
        r_squared: f64,
    }
    let mut sweep_rows: Vec<SweepRow> = Vec::new();

    eprintln!("\nTILE SIZE SWEEP (perf-focused suites, 1× + 2× DPR, per-tile-size OLS calibration)");
    for &tile_size in &[24u32, 32, 48, 64] {
        let sweep_tc = TileConfig::new(tile_size);

        // Pass 1: collect calibration samples with default model.
        let mut sweep_cal: Vec<(f64, f64, f64)> = Vec::new();
        for suite in &perf_suites {
            for &dpr in &[1.0f32, 2.0] {
                run_suite(suite, &CostModel::default(), dpr, &mut sweep_cal, &sweep_tc, true);
            }
        }
        let calibrated_cm = fit_cost_model(&sweep_cal)
            .map(|c| c.model)
            .unwrap_or_default();
        let (n_samples, r_squared) = fit_cost_model(&sweep_cal)
            .map(|c| (c.n_samples, c.r_squared))
            .unwrap_or((sweep_cal.len(), 0.0));

        // Pass 2: benchmark with the tile-size-specific calibrated model.
        let mut all_full = std::time::Duration::ZERO;
        let mut all_incr = std::time::Duration::ZERO;
        for suite in &perf_suites {
            for &dpr in &[1.0f32, 2.0] {
                eprintln!(
                    "  sweep tile={} suite={} dpr={}×...",
                    tile_size, suite.name, dpr
                );
                let result = run_suite(suite, &calibrated_cm, dpr, &mut vec![], &sweep_tc, true);
                for (i, f) in result.frames.iter().enumerate() {
                    if i > 0 {
                        all_full += f.full_time;
                        all_incr += f.incr_time;
                    }
                }
            }
        }

        let overall_speedup = all_full.as_secs_f64() / all_incr.as_secs_f64().max(1e-9);
        let perf_speedup = overall_speedup;
        sweep_rows.push(SweepRow { tile_size, overall_speedup, perf_speedup, n_samples, r_squared });
    }

    println!("\nTILE SIZE SWEEP (perf-focused suites, 1× + 2× DPR)");
    println!("{:>9} | {:>15} | {:>12} | {:>22}", "tile_size", "overall_speedup", "perf_speedup", "n_samples (OLS R²)");
    println!("{}", "-".repeat(65));
    for row in &sweep_rows {
        println!(
            "{:>9} | {:>14.1}× | {:>11.1}× | {:>9} (R²={:.3})",
            row.tile_size, row.overall_speedup, row.perf_speedup, row.n_samples, row.r_squared
        );
    }

    // Dump PNG frames for visual inspection (1× DPR only to keep output manageable)
    std::fs::create_dir_all("/tmp/poc_frames").unwrap();
    for (suite, sr) in suite_refs.iter().zip(results.iter()).filter(|(_, r)| r.dpr == 1.0) {
        let sname = suite.name.replace(' ', "_").to_lowercase();
        for (fi, f) in sr.frames.iter().enumerate() {
            if f.full_px.is_empty() {
                continue;
            }
            let base = format!("/tmp/poc_frames/{sname}_f{fi:02}");
            std::fs::write(format!("{base}_full.png"), encode_png(&f.full_px, f.w, f.h)).unwrap();
            if !f.incr_px.is_empty() {
                std::fs::write(format!("{base}_incr.png"), encode_png(&f.incr_px, f.w, f.h))
                    .unwrap();
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
        let result = run_suite(suite, &cm, 1.0, &mut vec![], &TileConfig::new(TILE_SIZE), false);
        let f = &result.frames[frame_idx];
        insta::with_settings!({ snapshot_path => snapshot_dir(), prepend_module_to_snapshot => false }, {
            assert_snapshot(&format!("{name}__full_f{frame_idx}"),  &f.full_px, f.w, f.h);
            assert_snapshot(&format!("{name}__incr_f{frame_idx}"),  &f.incr_px, f.w, f.h);
        });
    }

    /// Run `suite` once, snapshot full and incr pixels for multiple frame indices.
    fn run_and_snapshot(suite: &TestSuite, frame_indices: &[usize], name: &str) {
        let cm = CostModel::default();
        let result = run_suite(suite, &cm, 1.0, &mut vec![], &TileConfig::new(TILE_SIZE), false);
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
        run_and_snapshot(
            &suite_progress_fill(),
            &[3, 8],
            "reg_image_resize_dirty_region",
        );
    }

    /// Guards that a moved node clears its old tile position.
    ///
    /// The cyan block advances exactly one tile (32 px) per frame.  Frame 1
    /// moves it from tile-col 0 → 1; the left tile must be cleared and the
    /// right tile filled — no ghost at the old position.
    #[test]
    fn reg_moved_node_clears_old_position() {
        assert_render_snapshots(
            &suite_tile_crossing(),
            1,
            "reg_moved_node_clears_old_position",
        );
    }

    /// Guards that removed nodes leave no ghost pixels.
    ///
    /// Inline 3-frame mini-suite:
    ///   Frame 0: node-a (left, blue) + node-b (right, orange) — cold frame
    ///   Frame 1: only node-a — node-b removed; its old position must be cleared
    ///   Frame 2: node-a + node-c (same slot, purple) — old node-b area fully replaced
    #[test]
    fn reg_structure_change_no_ghost() {
        let mk_frame = |label: &str, children: Vec<IncrNode>| -> SuiteFrame {
            let root = IncrNode::Container {
                id: "canvas".into(),
                tw: "flex flex-row items-center w-[320px] h-[48px] bg-gray-900".into(),
                style: None,
                children,
            };
            SuiteFrame {
                label: label.into(),
                root,
            }
        };
        let node_a = || IncrNode::Container {
            id: "node-a".into(),
            tw: "w-[48px] h-[32px] bg-blue-500 rounded flex items-center justify-center".into(),
            style: None,
            children: vec![],
        };
        let node_b = || IncrNode::Container {
            id: "node-b".into(),
            tw: "ml-auto w-[48px] h-[32px] bg-orange-400 rounded flex items-center justify-center"
                .into(),
            style: None,
            children: vec![],
        };
        let node_c = || IncrNode::Container {
            id: "node-c".into(),
            tw: "ml-auto w-[48px] h-[32px] bg-purple-400 rounded flex items-center justify-center"
                .into(),
            style: None,
            children: vec![],
        };
        let suite = TestSuite {
            name: "Structure Change No Ghost",
            description: "node-b removed in frame 1, node-c added in frame 2 — no ghost pixels",
            frames: vec![
                mk_frame("cold: a+b", vec![node_a(), node_b()]),
                mk_frame("remove b", vec![node_a()]),
                mk_frame("add c", vec![node_a(), node_c()]),
            ],
            perf_focused: false,
            force_incremental: true,
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
        run_and_snapshot(
            &suite_progress_fill(),
            &[1, 4, 7, 8, 9],
            "test_rounded_clip_all_widths",
        );
    }

    /// Confirms ml-auto positions hdr-phase correctly across all phase transitions.
    ///
    /// Frames 5 (IDLE→RISING), 10 (RISING→PEAK), 15 (PEAK→FALLING).
    /// The phase label must always appear at the far-right edge of the header.
    #[test]
    fn test_ml_auto_all_phases() {
        run_and_snapshot(
            &suite_keyframe_animation(),
            &[5, 10, 15],
            "test_ml_auto_all_phases",
        );
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
        run_and_snapshot(
            &suite_panel_focus(),
            &[4, 8],
            "golden_workspace_focus_change",
        );
    }

    /// Golden snapshot of active notification rotating through the panel list.
    ///
    /// Frame 0 (cold, System highlighted), 2 (Messages), 4 (Build).
    /// Spinner also advances each frame.
    #[test]
    fn golden_notification_rotation() {
        run_and_snapshot(
            &suite_notification_panel(),
            &[0, 2, 4],
            "golden_notification_rotation",
        );
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
