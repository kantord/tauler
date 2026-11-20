//! The page's half of `store(...)` (`tauler-web/js/state.js`), run against the shared
//! globals and the `"tauler"` module exactly as a page loads them, in QuickJS: the file
//! is plain JavaScript and needs nothing a browser has.
//!
//! What a Store does on a desktop is tested in `src/jsx.rs` (`store_*`, and
//! `local_store_*` for a Store declared inside a function); these are the same
//! behaviours through the page's host functions.

use rquickjs::loader::{BuiltinLoader, BuiltinResolver};
use rquickjs::{CatchResultExt, Context, Module, Runtime};
use tauler_core::globals::{JSX_GLOBALS_JS, TAULER_MODULE, TAULER_MODULE_JS};

const STATE_JS: &str = include_str!("../tauler-web/js/state.js");

/// Evaluate `scenario` as a module that can import `./state.js` and `"tauler"`, after
/// the shared globals, and return what it left in `globalThis.out`.
fn run(scenario: &str) -> serde_json::Value {
    run_with_example("export default () => null;", scenario)
}

/// [`run`], with `example` importable as `'example'`: a layout file as a page gets it,
/// through the compile pass and the JSX transform (`example_js` in
/// `tauler-docgen/src/web.rs`).
fn run_with_example(example: &str, scenario: &str) -> serde_json::Value {
    let named =
        tauler_core::store_pass::rewrite(example, "example.jsx", "examples/example.jsx", "page")
            .expect("the example passes the Store pass");
    let example = optative_script::jsx::transform_source(&named, "example.jsx");
    let runtime = Runtime::new().unwrap();
    runtime.set_loader(
        BuiltinResolver::default()
            .with_module("state.js")
            .with_module("example")
            .with_module(TAULER_MODULE),
        BuiltinLoader::default()
            .with_module("state.js", STATE_JS)
            .with_module("example", example)
            .with_module(TAULER_MODULE, TAULER_MODULE_JS),
    );
    let context = Context::full(&runtime).unwrap();
    context.with(|ctx| {
        let run = || -> rquickjs::Result<String> {
            ctx.eval::<(), _>(JSX_GLOBALS_JS)?;
            let source = format!(
                "import {{ installStateHost, withState }} from 'state.js';\n\
                 import {{ store }} from 'tauler';\n\
                 installStateHost();\n\
                 globalThis.out = {{}};\n\
                 const read = (s) => withState(() => JSON.parse(JSON.stringify(s)));\n\
                 {scenario}"
            );
            let (_, promise) = Module::declare(ctx.clone(), "scenario", source)?.eval()?;
            promise.finish::<()>()?;
            ctx.eval::<String, _>("JSON.stringify(globalThis.out)")
        };
        let out = run()
            .catch(&ctx)
            .unwrap_or_else(|e| panic!("the scenario threw: {e}"));
        serde_json::from_str(&out).unwrap()
    })
}

/// What the compile pass writes for `const desk = store(default)` in an example.
const DESK: &str =
    r#"const desk = store({ count: 0, items: [], note: null }, "examples/a.jsx#desk", "page");"#;

#[test]
fn a_store_starts_from_its_default_and_keeps_what_a_handler_writes() {
    let out = run(&format!(
        "{DESK}
         out.initial = read(desk);
         withState(() => {{ desk.count += 1; desk.items.push('x'); desk.note = {{ text: 'a' }}; }});
         out.written = read(desk);
         withState(() => {{ delete desk.note; desk.items = ['y']; }});
         out.replaced = read(desk);
         out.has = withState(() => ['count' in desk, 'note' in desk]);"
    ));
    assert_eq!(
        out,
        serde_json::json!({
            "initial": { "count": 0, "items": [], "note": null },
            "written": { "count": 1, "items": ["x"], "note": { "text": "a" } },
            "replaced": { "count": 1, "items": ["y"] },
            "has": [true, false],
        })
    );
}

#[test]
fn a_handler_that_throws_leaves_every_store_as_it_was() {
    let out = run(&format!(
        "{DESK}
         withState(() => {{ desk.count = 1; desk.items.push('kept'); }});
         try {{
           withState(() => {{
             desk.count = 99; desk.items.push('lost'); desk.added = true; delete desk.note;
             throw new Error('boom');
           }});
         }} catch (e) {{ out.threw = e.message; }}
         out.after = read(desk);"
    ));
    assert_eq!(
        out,
        serde_json::json!({
            "threw": "boom",
            "after": { "count": 1, "items": ["kept"], "note": null },
        })
    );
}

#[test]
fn a_write_of_an_equal_value_moves_no_version() {
    let out = run(&format!(
        "{DESK}
         const version = () => __tauler_state_version('examples/a.jsx#desk', 'items');
         withState(() => {{ desk.items = ['x']; }});
         const before = version();
         withState(() => {{ desk.items = ['x']; desk.items.length; }});
         out.same = version() === before;
         withState(() => {{ desk.items.push('y'); }});
         out.moved = version() > before;"
    ));
    assert_eq!(out, serde_json::json!({ "same": true, "moved": true }));
}

/// Two Mounts on one page may load the module that declares a Store only once, but a
/// second declaration of a known identity must still behave as a desktop reload does.
#[test]
fn a_known_identity_keeps_its_value_and_takes_only_new_keys_from_a_default() {
    let out = run(&format!(
        "{DESK}
         withState(() => {{ desk.count = 5; }});
         const again = store({{ count: 0, fresh: true }}, 'examples/a.jsx#desk', 'page');
         out.again = read(again);"
    ));
    assert_eq!(
        out,
        serde_json::json!({
            "again": { "count": 5, "fresh": true, "items": [], "note": null },
        })
    );
}

#[test]
fn a_default_that_is_not_an_object_and_a_value_that_is_not_json_are_refused() {
    let out = run(
        "const refusal = (run) => { try { run(); return 'no error'; } catch (e) { return e.message; } };
         out.array = refusal(() => store([], 'examples/a.jsx#bad', 'page'));
         const s = store({}, 'examples/a.jsx#s', 'page');
         out.fn = refusal(() => withState(() => { s.f = () => 1; }));",
    );
    assert_eq!(
        out,
        serde_json::json!({
            "array": "store(): the default must be a JSON object",
            "fn": "state holds only JSON values",
        })
    );
}

// ---- Local Stores on a page (`tauler-core/src/local_store.rs`, P7) ----

/// A list of rows, each a component with a local Store, and a function that declares
/// one called through a `const` at the top level.
const ROWS_EXAMPLE: &str = r#"import { store } from "tauler";
function counter() {
    const s = store({ n: 0 });
    return s;
}
const total = counter();
function Row({ name }) {
    const s = store({ n: 0 });
    const twin = counter();
    return <span on_click={() => { s.n += 1; twin.n += 10; total.n += 100; }}>{`${name}=${s.n}/${twin.n}/${total.n}`}</span>;
}
export default function render() {
    if (globalThis.fail) return <div><Row key="a" name="a" /><Row key="a" name="a" /></div>;
    return <div>{globalThis.rows.map((name) => <Row key={name} name={name} />)}</div>;
}
"#;

/// What `runtime.js` does around the example: `h` over a factory that is `estoH` in
/// short, and one `__tauler_rendering` per Mount with the Mount as the first link.
const TWO_MOUNTS: &str = r#"
    // Loaded after the host is installed, as a page loads an example after `boot`.
    const { default: render } = await import('example');
    globalThis.h = __tauler_h((type, props, ...children) =>
        typeof type === 'function'
            ? type({ ...(props ?? {}), children })
            : { type, props: { ...(props ?? {}) }, children });
    const mounts = [1, 2].map((n) => __tauler_rendering(render, 'mount:' + n));
    const tick = (n) => {
        __tauler_handlers.length = 0;
        const tree = withState(mounts[n - 1]);
        return tree.children.flat().map((row) => row.children[0]);
    };
    const click = (n, row) => {
        __tauler_handlers.length = 0;
        const tree = withState(mounts[n - 1]);
        const id = tree.children.flat()[row].on_click.$handler;
        withState(() => __tauler_handlers[id]());
    };
    const held = (identity) => __tauler_state_version(identity, 'n') !== 0;
    const row = (mount, name) => `local://mount:${mount} > examples/example.jsx#Row[${name}] > examples/example.jsx#Row.s`;
    globalThis.rows = ['a', 'b'];
"#;

#[test]
fn two_mounts_of_one_layout_share_no_local_store_and_neither_drops_the_others() {
    let out = run_with_example(
        ROWS_EXAMPLE,
        &format!(
            "{TWO_MOUNTS}
             out.first = [tick(1), tick(2)];
             click(1, 1);
             out.clicked = [tick(1), tick(2)];
             out.declared = __tauler_state_declared();
             globalThis.rows = ['b'];
             out.one_row = tick(1);
             out.dropped = [held(row(1, 'a')), held(row(1, 'b')), held(row(2, 'a'))];
             globalThis.rows = ['a', 'b'];
             out.back = [tick(1), tick(2)];"
        ),
    );
    assert_eq!(
        out,
        serde_json::json!({
            "first": [["a=0/0/0", "b=0/0/0"], ["a=0/0/0", "b=0/0/0"]],
            // The Row's own and the one behind `twin` are the Mount's; `total` is shared.
            "clicked": [["a=0/0/100", "b=1/10/100"], ["a=0/0/100", "b=0/0/100"]],
            "declared": [
                "local://examples/example.jsx#total > examples/example.jsx#counter.s",
                "local://mount:2 > examples/example.jsx#Row[a] > examples/example.jsx#Row.s",
                "local://mount:2 > examples/example.jsx#Row[a] > twin > examples/example.jsx#counter.s",
                "local://mount:2 > examples/example.jsx#Row[b] > examples/example.jsx#Row.s",
                "local://mount:2 > examples/example.jsx#Row[b] > twin > examples/example.jsx#counter.s",
            ],
            "one_row": ["b=1/10/100"],
            "dropped": [false, true, true],
            "back": [["a=0/0/100", "b=1/10/100"], ["a=0/0/100", "b=0/0/100"]],
        })
    );
}

/// A handler kept from an earlier render writes a local Store that a later render
/// dropped: the page ignores the write, as the desktop does, and keeps the handler's
/// write to a shared Store.
#[test]
fn a_handler_held_past_its_render_writes_nothing_to_a_dropped_local_store() {
    let out = run_with_example(
        ROWS_EXAMPLE,
        &format!(
            "{TWO_MOUNTS}
             __tauler_handlers.length = 0;
             const tree = withState(mounts[0]);
             const press = __tauler_handlers[tree.children.flat()[0].on_click.$handler];
             const twin = 'local://mount:1 > examples/example.jsx#Row[a] > twin > examples/example.jsx#counter.s';
             globalThis.rows = ['b'];
             tick(1);
             withState(() => press());
             out.keys = [__tauler_state_keys(row(1, 'a')), __tauler_state_keys(twin)];
             out.held = [held(row(1, 'a')), held(twin), held(row(1, 'b'))];
             out.one_row = tick(1);
             globalThis.rows = ['a', 'b'];
             out.back = tick(1);
             click(1, 0);
             out.clicked = tick(1);"
        ),
    );
    assert_eq!(
        out,
        serde_json::json!({
            "keys": [[], []],
            "held": [false, false, true],
            // `total` is shared: that write of the handler is kept.
            "one_row": ["b=0/0/100"],
            "back": ["a=0/0/100", "b=0/0/100"],
            "clicked": ["a=1/10/200", "b=0/0/200"],
        })
    );
}

#[test]
fn a_render_that_declares_one_local_store_twice_throws_and_drops_nothing() {
    let out = run_with_example(
        ROWS_EXAMPLE,
        &format!(
            "{TWO_MOUNTS}
             tick(1);
             click(1, 0);
             globalThis.fail = true;
             try {{ tick(1); }} catch (e) {{ out.threw = e.message.slice(0, e.message.indexOf(',')); }}
             globalThis.fail = false;
             out.after = tick(1);"
        ),
    );
    assert_eq!(
        out,
        serde_json::json!({
            "threw": "store(): the local Store `mount:1 > examples/example.jsx#Row[a] > examples/example.jsx#Row.s` is declared twice in one render",
            "after": ["a=1/10/100", "b=0/0/100"],
        })
    );
}

/// A render that throws takes back the local Stores it declared, entry and all: a
/// handler's later write to one stores nothing, as on the desktop (`Undo::Declared` in
/// `src/state.rs`). A Store that was held before that render stays held.
#[test]
fn a_render_that_throws_leaves_no_local_store_it_declared() {
    let out = run("const kept = 'local://mount:1 > kept';
         const fresh = 'local://mount:1 > fresh';
         withState(() => __tauler_state_declare(kept, { n: 1 }, 'page'));
         try {
           withState(() => {
             __tauler_state_declare(kept, { n: 1, more: 2 }, 'page');
             __tauler_state_declare(fresh, { n: 1 }, 'page');
             throw new Error('boom');
           });
         } catch (e) { out.threw = e.message; }
         out.written = withState(() => [
           __tauler_state_set(fresh, 'n', 5) > 0,
           __tauler_state_set(kept, 'n', 5) > 0,
         ]);
         out.keys = [__tauler_state_keys(fresh), __tauler_state_keys(kept)];");
    assert_eq!(
        out,
        serde_json::json!({
            "threw": "boom",
            "written": [false, true],
            "keys": [[], ["n"]],
        })
    );
}
