//! What each pipeline stage costs in instructions and allocations, not seconds.
//!
//! Wall-clock benches (`pipeline`, `store`) move with the machine's load and clock;
//! these numbers do not, so a change that makes a stage do more work shows up as a
//! larger number on any machine, and the figures can be compared across commits and
//! across CI runs.
//!
//! Two kinds of number, both deterministic:
//!
//! - **allocations, bytes allocated, peak live bytes**: counted in-process by a global
//!   allocator wrapper, around one run of the stage. `peak` is the high-water mark of
//!   live heap above what the stage started with.
//! - **instructions (`Ir`)**: counted by callgrind, which collects only while the
//!   stage's own call (`tauler_cost_measured`) is on the stack, so process start-up and
//!   the warm-up run are not in the figure.
//!
//! Every stage is run once as a warm-up (fonts, glyph caches, QuickJS start-up) and
//! once measured, so the figure is the steady-state cost of the stage.
//!
//! ## Running
//!
//! `just cost` prints the full table. It needs `valgrind`; without it, `cargo bench
//! --bench cost` prints the allocation columns alone, and `cargo bench --bench cost --
//! <stage>` runs one stage. Reading the table: compare a row with the same row from
//! another commit. Allocation columns repeat exactly and `Ir` to within 0.5%; treat a
//! change of more than about 2% in `Ir`, or any change in the allocation columns, as real.
//!
//! The allocation columns count the Rust heap only. QuickJS allocates through libc
//! `malloc`, so the JavaScript heap is invisible to them (`store_read_all` shows no
//! more allocations than `store_read_none`); `Ir` covers that work.
//!
//! The stages mirror `pipeline.rs` (same synthetic fixture) plus the frame-cache hit
//! path and the Store reads of `store.rs`. Nothing here needs a display server.

use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

use serde_json::{json, Value};
use tauler::jsx::JsxEvaluator;
use tauler::layout::{parse_root_node, SurfaceKind, SurfaceSpec};
use tauler::render::cache::FrameCache;
use tauler::render::worker::RenderRequest;

static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note_alloc(layout.size());
        System.alloc(layout)
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note_alloc(layout.size());
        System.alloc_zeroed(layout)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Relaxed);
        System.dealloc(ptr, layout)
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // A resize is one allocation of the new size and a free of the old one.
        LIVE.fetch_sub(layout.size(), Relaxed);
        note_alloc(new_size);
        System.realloc(ptr, layout, new_size)
    }
}

fn note_alloc(size: usize) {
    ALLOCS.fetch_add(1, Relaxed);
    BYTES.fetch_add(size, Relaxed);
    let live = LIVE.fetch_add(size, Relaxed) + size;
    PEAK.fetch_max(live, Relaxed);
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// The one function callgrind collects inside (`--toggle-collect`); the allocation
/// counters are read around it too, so both columns cover exactly the same work.
#[no_mangle]
#[inline(never)]
pub fn tauler_cost_measured(stage: &mut dyn FnMut()) {
    stage();
}

fn measure(mut stage: impl FnMut()) -> (usize, usize, usize) {
    stage(); // warm-up
    ALLOCS.store(0, Relaxed);
    BYTES.store(0, Relaxed);
    PEAK.store(LIVE.load(Relaxed), Relaxed);
    let base = LIVE.load(Relaxed);
    tauler_cost_measured(&mut stage);
    (
        ALLOCS.load(Relaxed),
        BYTES.load(Relaxed),
        PEAK.load(Relaxed).saturating_sub(base),
    )
}

const STATES: usize = 10;

fn fixture_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("benches/fixtures/synthetic")
}

fn layout_evaluator() -> JsxEvaluator {
    let dir = fixture_dir();
    let source = std::fs::read_to_string(dir.join("layout.jsx")).expect("fixture layout");
    let ctx =
        json!({ "output": "DP-1", "dpi": 140.0, "screen_width": 2560, "screen_height": 1440 });
    JsxEvaluator::new(&source, ctx, Some(&dir.join("layout.jsx"))).expect("fixture compiles")
}

type Snapshot = HashMap<(String, Option<String>), String>;

fn snapshot(i: usize) -> Snapshot {
    let mut m = HashMap::new();
    m.insert(
        ("/bin/sh".to_string(), Some("echo load".to_string())),
        json!({ "one": i * 9, "five": i * 4 }).to_string(),
    );
    m
}

fn panels(ev: &JsxEvaluator, snap: &Snapshot) -> Vec<SurfaceSpec> {
    let out = ev.eval(snap).expect("eval");
    parse_root_node(&out.layout)
        .expect("root parses")
        .into_iter()
        .filter(|s| s.kind == SurfaceKind::Panel)
        .collect()
}

fn request(spec: &SurfaceSpec) -> RenderRequest {
    RenderRequest {
        id: "bench".into(),
        content: spec.content.clone(),
        width: (spec.width as f32 * spec.dpr).round() as u32,
        height: (spec.height as f32 * spec.dpr).round() as u32,
        dpr: spec.dpr,
        backdrop: None,
        trace: None,
    }
}

/// A Store of twenty keys, some nested, the shape `store.rs` calls `realistic` in
/// miniature.
fn store_evaluator(render: &str) -> JsxEvaluator {
    let source = format!(
        "import {{ store }} from \"tauler\";\nconst s = store({{}});\nexport default () => {{ {render} }};"
    );
    let ctx =
        json!({ "output": "DP-1", "dpi": 140.0, "screen_width": 2560, "screen_height": 1440 });
    let ev = JsxEvaluator::new(&source, ctx, None).expect("layout compiles");
    {
        let state = ev.state_handle();
        let mut state = state.lock().unwrap();
        for k in 0..20 {
            let v: Value = if k % 2 == 0 {
                json!(k * 7)
            } else {
                json!({ "name": format!("item-{k}"), "tags": ["a", "b", "c"], "n": k })
            };
            state.set("layout.jsx#s", &format!("k{k}"), v);
        }
    }
    ev
}

const STAGES: &[&str] = &[
    "eval",
    "parse",
    "rasterize",
    "cache_hit",
    "eval_to_pixels",
    "store_read_none",
    "store_read_all",
];

fn run_stage(name: &str) -> (usize, usize, usize) {
    let streams: Snapshot = HashMap::new();
    match name {
        "eval" => {
            let ev = layout_evaluator();
            let snaps: Vec<_> = (0..STATES).map(snapshot).collect();
            for s in &snaps {
                ev.eval(s).expect("eval"); // every state seen once before the measured one
            }
            measure(|| {
                std::hint::black_box(ev.eval(&snaps[3]).expect("eval"));
            })
        }
        "parse" => {
            let ev = layout_evaluator();
            let out = ev.eval(&snapshot(3)).expect("eval");
            measure(|| {
                std::hint::black_box(parse_root_node(&out.layout).expect("root parses"));
            })
        }
        "rasterize" => {
            let ev = layout_evaluator();
            let reqs: Vec<_> = panels(&ev, &snapshot(3)).iter().map(request).collect();
            measure(|| {
                for r in &reqs {
                    std::hint::black_box(tauler::render_frame_keyed(
                        &r.content, r.width, r.height, r.dpr, None,
                    ));
                }
            })
        }
        "cache_hit" => {
            let ev = layout_evaluator();
            let reqs: Vec<_> = panels(&ev, &snapshot(3)).iter().map(request).collect();
            let mut cache = FrameCache::new();
            for r in &reqs {
                cache.frame(r); // fill: the measured pass only hits
            }
            measure(|| {
                for r in &reqs {
                    std::hint::black_box(cache.frame(r));
                }
            })
        }
        "eval_to_pixels" => {
            let ev = layout_evaluator();
            let snap = snapshot(3);
            measure(|| {
                for spec in panels(&ev, &snap) {
                    let r = request(&spec);
                    std::hint::black_box(tauler::render_frame_keyed(
                        &r.content, r.width, r.height, r.dpr, None,
                    ));
                }
            })
        }
        "store_read_none" => {
            let ev = store_evaluator("return <div />;");
            measure(|| {
                std::hint::black_box(ev.eval(&streams).expect("eval"));
            })
        }
        "store_read_all" => {
            let ev = store_evaluator(
                "for (const k of Object.keys(s)) JSON.stringify(s[k]); return <div />;",
            );
            measure(|| {
                std::hint::black_box(ev.eval(&streams).expect("eval"));
            })
        }
        other => panic!("unknown stage {other}; stages: {STAGES:?}"),
    }
}

fn main() {
    tauler::init_global_ctx(tauler::config::FontConfig::default());
    // `cargo bench` appends flags such as `--bench`; the only argument that matters is a stage name.
    let wanted: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| !a.starts_with('-'))
        .collect();
    let stages: Vec<&str> = if wanted.is_empty() {
        STAGES.to_vec()
    } else {
        wanted.iter().map(String::as_str).collect()
    };
    // One machine-readable line per stage: `stage allocs bytes peak`.
    for stage in stages {
        let (allocs, bytes, peak) = run_stage(stage);
        println!("{stage} {allocs} {bytes} {peak}");
    }
}
