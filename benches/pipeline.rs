//! What a frame costs, and how much of that is tauler rather than takumi.
//!
//! The question this exists to answer: when a value changes and the bar redraws,
//! where does the time go? Every measurement here is against the same synthetic
//! layout (`fixtures/synthetic/`), and every one of them ends in the same ten
//! pictures, so the numbers are comparable to each other by construction.
//!
//! `draw_only` is the floor — the ten frames rasterized with no pipeline in
//! front of them, which is the best any amount of work upstream could achieve.
//! Everything else is that floor plus a stage, so the difference between two
//! rows is what the stage between them costs.
//!
//! Nothing here needs a display server. The presenter and the display backend
//! are the only parts left out, and they are the parts a `SurfaceCommand`
//! already isolates.

use std::collections::HashMap;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use tauler::jsx::JsxEvaluator;
use tauler::layout::{parse_root_node, SurfaceKind, SurfaceSpec};

/// How many distinct states the bar cycles through.
///
/// Ten rather than one because a single state would sit in the frame cache and
/// measure a hash lookup. Ten rather than a hundred because the cache holds six
/// — past that every draw is a miss, which is a different question.
const STATES: usize = 10;

fn fixture_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("benches/fixtures/synthetic")
}

fn evaluator() -> JsxEvaluator {
    let dir = fixture_dir();
    let source = std::fs::read_to_string(dir.join("layout.jsx")).expect("fixture layout");
    let ctx = serde_json::json!({
        "output": "DP-1", "dpi": 140.0, "screen_width": 2560, "screen_height": 1440
    });
    JsxEvaluator::new(&source, ctx, Some(dir.as_path())).expect("fixture compiles")
}

/// Ten stream snapshots, differing the way a dragged control differs: one value
/// moving, everything else still.
fn snapshots() -> Vec<HashMap<(String, Option<String>), String>> {
    (0..STATES)
        .map(|i| {
            let mut m = HashMap::new();
            m.insert(
                ("/bin/sh".to_string(), Some("echo load".to_string())),
                serde_json::json!({ "one": i * 9, "five": i * 4 }).to_string(),
            );
            m
        })
        .collect()
}

/// The panels one snapshot produces, at their physical size.
fn panels_of(spec: Vec<SurfaceSpec>) -> Vec<SurfaceSpec> {
    spec.into_iter()
        .filter(|s| s.kind == SurfaceKind::Panel)
        .collect()
}

fn draw(spec: &SurfaceSpec) {
    let w = (spec.width as f32 * spec.dpr).round() as u32;
    let h = (spec.height as f32 * spec.dpr).round() as u32;
    std::hint::black_box(tauler::render_frame_keyed(
        &spec.content,
        w,
        h,
        spec.dpr,
        None,
    ));
}

fn benches(c: &mut Criterion) {
    tauler::init_global_ctx(tauler::config::FontConfig::default());
    let ev = evaluator();
    let snaps = snapshots();

    // The ten trees, evaluated once up front so the draw-only floor pays for
    // nothing but drawing.
    let states: Vec<Vec<SurfaceSpec>> = snaps
        .iter()
        .map(|s| {
            let out = ev.eval(s).expect("eval");
            panels_of(parse_root_node(&out.layout).expect("root parses"))
        })
        .collect();

    let mut group = c.benchmark_group("frame");
    group.sample_size(30);

    // The floor: no JavaScript, no diffing, just pixels.
    group.bench_function(BenchmarkId::new("draw_only", STATES), |b| {
        let mut i = 0usize;
        b.iter(|| {
            for spec in &states[i % STATES] {
                draw(spec);
            }
            i += 1;
        })
    });

    // Everything the loop does before it knows what to draw.
    group.bench_function(BenchmarkId::new("eval_only", STATES), |b| {
        let mut i = 0usize;
        b.iter(|| {
            std::hint::black_box(ev.eval(&snaps[i % STATES]).expect("eval"));
            i += 1;
        })
    });

    // Eval plus turning the tree into surface specs.
    group.bench_function(BenchmarkId::new("eval_and_parse", STATES), |b| {
        let mut i = 0usize;
        b.iter(|| {
            let out = ev.eval(&snaps[i % STATES]).expect("eval");
            std::hint::black_box(parse_root_node(&out.layout).expect("root parses"));
            i += 1;
        })
    });

    // The whole thing a changed value costs, short of the presenter.
    group.bench_function(BenchmarkId::new("eval_to_pixels", STATES), |b| {
        let mut i = 0usize;
        b.iter(|| {
            let out = ev.eval(&snaps[i % STATES]).expect("eval");
            for spec in panels_of(parse_root_node(&out.layout).expect("root parses")) {
                draw(&spec);
            }
            i += 1;
        })
    });

    group.finish();
}

criterion_group!(pipeline, benches);
criterion_main!(pipeline);
