//! What one Store costs an `eval`, by what the render does with it (issue #271).
//!
//! A Store's values stay in Rust (`src/state.rs`) and a render reaches them one
//! top-level key at a time, so the cost of an eval depends on what the render reads
//! and on the shape of what it reads, not on how much the Store holds. The cases
//! vary the things that change conversion cost: key count, nesting depth, value
//! types, string length, and arrays of objects.
//!
//! `read_none` is the per-tick cost of a Store nothing changed and nothing read;
//! `read_all` is a render that walks every key. The figures quoted in `src/state.rs`
//! and ADR 0051 are these, shape `realistic`.
//!
//! `eval_with_bulk_copy` is what those figures are compared with: the cost of
//! `globals`, which Stores replaced. The product no longer has that copy, so the group
//! does it here (see [`bench_bulk_copy`]).
//!
//! `eval_with_local` is what a Store declared inside a function costs, and what a layout
//! with none pays for the machinery (`tauler-core/src/local_store.rs` quotes it).
//!
//! Nothing here needs a display server.

use std::collections::HashMap;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use serde_json::{json, Map, Value};
use tauler::jsx::JsxEvaluator;

/// A tiny deterministic generator, so every run measures the same data.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

/// A scalar of a varying type: number, float, bool, null, or a short string.
fn scalar(rng: &mut Lcg) -> Value {
    match rng.next() % 5 {
        0 => json!(rng.next() % 10_000),
        1 => json!(rng.next() as f64 / 7.0),
        2 => json!(rng.next().is_multiple_of(2)),
        3 => Value::Null,
        _ => json!(format!("value-{}", rng.next() % 100_000)),
    }
}

/// A tree `depth` levels deep, each level `fanout` wide, leaves mixed scalars.
/// Interior levels alternate between objects and arrays.
fn tree(rng: &mut Lcg, depth: usize, fanout: usize) -> Value {
    if depth == 0 {
        return scalar(rng);
    }
    if depth.is_multiple_of(2) {
        Value::Object(
            (0..fanout)
                .map(|i| (format!("k{i}"), tree(rng, depth - 1, fanout)))
                .collect(),
        )
    } else {
        Value::Array((0..fanout).map(|_| tree(rng, depth - 1, fanout)).collect())
    }
}

fn flat(rng: &mut Lcg, keys: usize) -> Map<String, Value> {
    (0..keys)
        .map(|i| (format!("key{i}"), scalar(rng)))
        .collect()
}

/// A single chain of objects, `depth` deep, with a few scalars at each level.
fn deep(rng: &mut Lcg, depth: usize) -> Map<String, Value> {
    let mut inner: Value = json!({ "leaf": scalar(rng) });
    for level in 0..depth {
        let mut m = flat(rng, 3);
        m.insert(format!("level{level}"), inner);
        inner = Value::Object(m);
    }
    match inner {
        Value::Object(m) => m,
        _ => unreachable!(),
    }
}

/// The shapes a layout's state holds in practice: a few flags, a map of workspace
/// name to unread count, and a list of notification-like records.
fn realistic(rng: &mut Lcg) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("desk".into(), json!(true));
    m.insert("mode".into(), json!("focus"));
    m.insert(
        "unread".into(),
        Value::Object(
            (0..12)
                .map(|i| (format!("ws{i}"), json!(rng.next() % 9)))
                .collect(),
        ),
    );
    m.insert(
        "notifications".into(),
        Value::Array(
            (0..40)
                .map(|i| {
                    json!({
                        "id": i,
                        "app": format!("app-{}", rng.next() % 8),
                        "summary": "x".repeat(20 + (rng.next() % 60) as usize),
                        "urgent": rng.next().is_multiple_of(4),
                        "actions": ["open", "dismiss"],
                    })
                })
                .collect(),
        ),
    );
    m
}

fn cases() -> Vec<(&'static str, Map<String, Value>)> {
    let mut rng = Lcg(0x5eed);
    let one =
        |k: &str, v: Value| -> Map<String, Value> { std::iter::once((k.to_string(), v)).collect() };
    vec![
        ("empty", Map::new()),
        ("realistic", realistic(&mut rng)),
        ("flat_20", flat(&mut rng, 20)),
        ("flat_2000", flat(&mut rng, 2000)),
        ("deep_20", deep(&mut rng, 20)),
        ("deep_200", deep(&mut rng, 200)),
        ("tree_d4_f8", one("t", tree(&mut rng, 4, 8))),
        ("tree_d6_f5", one("t", tree(&mut rng, 6, 5))),
        (
            "long_strings_50x10k",
            (0..50)
                .map(|i| (format!("s{i}"), json!("é".repeat(10_000))))
                .collect(),
        ),
        (
            "array_5000_numbers",
            one("a", Value::Array((0..5000).map(|i| json!(i)).collect())),
        ),
        (
            "array_1000_objects",
            one(
                "a",
                Value::Array(
                    (0..1000)
                        .map(|i| json!({ "id": i, "name": format!("n{i}"), "on": i % 2 == 0 }))
                        .collect(),
                ),
            ),
        ),
    ]
}

/// The render of each read pattern over one Store `s`.
const STORE_PATTERNS: &[(&str, &str)] = &[
    // The render reads nothing.
    ("read_none", "return <div />;"),
    // The render reads one primitive (`__p`, added to every shape).
    ("read_one_primitive", "s.__p; return <div />;"),
    // Every top-level key is handed out and nothing looks inside: the reads plus the
    // compare of every nested value handed out, without the render's own walk.
    (
        "touch_all",
        "for (const k of Object.keys(s)) s[k]; return <div />;",
    ),
    // The render reads everything.
    (
        "read_all",
        "for (const k of Object.keys(s)) JSON.stringify(s[k]); return <div />;",
    ),
    // One nested value is changed in place every render, so one compare fails and one
    // value is written back. Shapes with no nested value write a primitive instead.
    (
        "read_all_write_one_in_place",
        "for (const k of Object.keys(s)) JSON.stringify(s[k]); \
         const n = (globalThis.__n = (globalThis.__n ?? 0) + 1); \
         const k = Object.keys(s).find((k) => typeof s[k] === 'object' && s[k] !== null); \
         if (k) { const v = s[k]; if (Array.isArray(v)) v[0] = n; else v.__w = n; } \
         else s.__p = n; \
         return <div />;",
    ),
    // The same change made by assigning a new value: a top-level write, and the new
    // value is compared once more after the render.
    (
        "read_all_replace_one",
        "for (const k of Object.keys(s)) JSON.stringify(s[k]); \
         const n = (globalThis.__n = (globalThis.__n ?? 0) + 1); \
         const k = Object.keys(s).find((k) => typeof s[k] === 'object' && s[k] !== null); \
         if (k) { const v = s[k]; s[k] = Array.isArray(v) ? [n, ...v.slice(1)] : { ...v, __w: n }; } \
         else s.__p = n; \
         return <div />;",
    ),
];

/// A layout file cannot seed a Store's contents from Rust, so the data is set on the
/// store the evaluator was built with, under the identity the compile pass gives `s`.
const IDENTITY: &str = "layout.jsx#s";

fn store_evaluator(render: &str, data: &Map<String, Value>) -> JsxEvaluator {
    let source = format!(
        "import {{ store }} from \"tauler\";\nconst s = store({{}});\nexport default () => {{ {render} }};"
    );
    let ctx =
        json!({ "output": "DP-1", "dpi": 140.0, "screen_width": 2560, "screen_height": 1440 });
    let evaluator = JsxEvaluator::new(&source, ctx, None).expect("layout compiles");
    {
        let state = evaluator.state_handle();
        let mut state = state.lock().unwrap();
        state.set(IDENTITY, "__p", json!(1));
        for (key, value) in data {
            state.set(IDENTITY, key, value.clone());
        }
    }
    evaluator
}

/// One eval per iteration, for every read pattern over every shape.
fn bench_store(c: &mut Criterion) {
    let streams = HashMap::new();
    for (pattern, render) in STORE_PATTERNS {
        let mut group = c.benchmark_group(format!("eval_with_store/{pattern}"));
        for (name, data) in cases() {
            let evaluator = store_evaluator(render, &data);
            group.bench_with_input(BenchmarkId::from_parameter(name), &(), |b, ()| {
                b.iter(|| std::hint::black_box(evaluator.eval(&streams).expect("eval")));
            });
        }
        group.finish();
    }
}

/// What the walk of `read_all` is over a plain object named `globals`.
const BULK_COPY_PATTERNS: &[(&str, &str)] = &[
    // Counterpart of `eval_with_store/read_none`.
    ("read_none", "(function () {})"),
    // Counterpart of `eval_with_store/read_all`.
    (
        "read_all",
        "(function () { for (const k of Object.keys(globals)) JSON.stringify(globals[k]); })",
    ),
];

/// What `globals` cost an eval, for the same shapes as `eval_with_store`.
///
/// Before every render the removed code cloned the whole map, converted the clone into
/// a JavaScript value with `rquickjs_serde::to_value` and set it as the global
/// `globals`; after the render it read that global and converted it back with
/// `rquickjs_serde::from_value`. One iteration here does the same steps around the same
/// eval of a layout that declares no Store.
///
/// `JsxEvaluator` gives a bench no access to its JavaScript context, so the two
/// conversions and the walk run in a context this bench builds, with the same `rquickjs`
/// and `rquickjs_serde` the evaluator uses, and the eval runs in the evaluator's own. The
/// work per iteration is the same; only the context that holds `globals` differs.
fn bench_bulk_copy(c: &mut Criterion) {
    let streams = HashMap::new();
    let ctx =
        json!({ "output": "DP-1", "dpi": 140.0, "screen_width": 2560, "screen_height": 1440 });
    let evaluator =
        JsxEvaluator::new("export default () => <div />;", ctx, None).expect("layout compiles");
    let runtime = rquickjs::Runtime::new().expect("runtime");
    let context = rquickjs::Context::full(&runtime).expect("context");
    for (pattern, walk) in BULK_COPY_PATTERNS {
        let walk: rquickjs::Persistent<rquickjs::Function<'static>> = context.with(|qjs| {
            let f: rquickjs::Function = qjs.eval(*walk).expect("walk compiles");
            rquickjs::Persistent::save(&qjs, f)
        });
        let mut group = c.benchmark_group(format!("eval_with_bulk_copy/{pattern}"));
        for (name, data) in cases() {
            group.bench_with_input(BenchmarkId::from_parameter(name), &(), |b, ()| {
                b.iter(|| {
                    let start = data.clone();
                    context.with(|qjs| {
                        let value = rquickjs_serde::to_value(qjs.clone(), &start).expect("in");
                        qjs.globals().set("globals", value).expect("set");
                        let walk = walk.clone().restore(&qjs).expect("restore");
                        walk.call::<(), ()>(()).expect("walk");
                    });
                    let out = evaluator.eval(&streams).expect("eval");
                    let after: Map<String, Value> = context.with(|qjs| {
                        let value: rquickjs::Value = qjs.globals().get("globals").expect("get");
                        rquickjs_serde::from_value(value).expect("out")
                    });
                    std::hint::black_box((start, out, after))
                });
            });
        }
        group.finish();
    }
}

/// Layouts for what a local Store costs (`tauler-core/src/local_store.rs`): name, and
/// the layout file after its `import { store } from "tauler"`.
///
/// The first of each pair has no local Store, and is what every layout pays for the
/// stack of component instances and for named calls; it also loads on a build from
/// before local Stores, which is how the "before" figures in `local_store.rs` were
/// measured. `{STORE}` and `{DECL}` are filled in by [`local_cases`].
const LOCAL_ROWS: &str = r#"
function Row({ label }) {
    {STORE}
    return <div class="flex gap-2"><span>{label}</span><span>{VALUE}</span></div>;
}
const names = Array.from({ length: 60 }, (_, i) => "row" + i);
export default () => <div>{names.map((name) => <Row key={name} label={name} />)}</div>;
"#;

/// Six chains of ten components, one inside the other: identities up to eleven links.
const LOCAL_DEEP: &str = r#"
function Level({ depth }) {
    {STORE}
    return depth > 0 ? <div><Level depth={depth - 1} /></div> : <span>{VALUE}</span>;
}
function Chain() { return <Level depth={9} />; }
const chains = ["a", "b", "c", "d", "e", "f"];
export default () => <div>{chains.map((name) => <Chain key={name} />)}</div>;
"#;

/// A thousand calls that are the whole value of a `{DECL}`: `const` gets each its name
/// from the compile pass, `let` is left as written.
const LOCAL_CALLS: &str = r#"
function next(i) { return i + 1; }
function Work() {
    let sum = 0;
    for (let i = 0; i < 1000; i++) { {DECL} v = next(i); sum += v; }
    return <span>{String(sum)}</span>;
}
export default () => <Work />;
"#;

/// What [`LOCAL_ROWS`] renders, written with no component: the same elements, so the
/// difference between the two is what sixty component calls cost.
const LOCAL_ELEMENTS: &str = r#"
const names = Array.from({ length: 60 }, (_, i) => "row" + i);
export default () => <div>{names.map((name) =>
    <div class="flex gap-2"><span>{name}</span><span>x</span></div>)}</div>;
"#;

fn local_cases() -> Vec<(&'static str, String)> {
    let fill = |layout: &str, store: &str, value: &str| {
        layout.replace("{STORE}", store).replace("{VALUE}", value)
    };
    let declare = "const s = store({ open: false });";
    vec![
        ("rows_60/no_component", LOCAL_ELEMENTS.to_string()),
        ("rows_60/no_store", fill(LOCAL_ROWS, "", "\"x\"")),
        (
            "rows_60/store_each_unread",
            fill(LOCAL_ROWS, declare, "\"x\""),
        ),
        (
            "rows_60/store_each_read",
            fill(LOCAL_ROWS, declare, "String(s.open)"),
        ),
        ("deep_6x10/no_store", fill(LOCAL_DEEP, "", "\"x\"")),
        (
            "deep_6x10/store_each_unread",
            fill(LOCAL_DEEP, declare, "\"x\""),
        ),
        ("calls_1000/plain", LOCAL_CALLS.replace("{DECL}", "let")),
        ("calls_1000/named", LOCAL_CALLS.replace("{DECL}", "const")),
    ]
}

/// One eval per iteration of each [`local_cases`] layout.
fn bench_local(c: &mut Criterion) {
    let streams = HashMap::new();
    let ctx =
        json!({ "output": "DP-1", "dpi": 140.0, "screen_width": 2560, "screen_height": 1440 });
    let mut group = c.benchmark_group("eval_with_local");
    for (name, layout) in local_cases() {
        group.bench_function(name, |b| {
            // Built here, not above: a filter then decides which layouts are loaded.
            let source = format!("import {{ store }} from \"tauler\";\n{layout}");
            let evaluator = JsxEvaluator::new(&source, ctx.clone(), None).expect("layout compiles");
            b.iter(|| std::hint::black_box(evaluator.eval(&streams).expect("eval")));
        });
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .warm_up_time(std::time::Duration::from_secs(1))
        .measurement_time(std::time::Duration::from_secs(3));
    targets = bench_store, bench_bulk_copy, bench_local
}
criterion_main!(benches);
