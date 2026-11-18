//! The page's half of `store(...)` (`tauler-web/js/state.js`), run against the shared
//! globals and the `"tauler"` module exactly as a page loads them, in QuickJS: the file
//! is plain JavaScript and needs nothing a browser has.
//!
//! What a Store does on a desktop is tested in `src/jsx.rs` (`store_*`); these are the
//! same behaviours through the page's host functions.

use rquickjs::loader::{BuiltinLoader, BuiltinResolver};
use rquickjs::{CatchResultExt, Context, Module, Runtime};
use tauler_core::globals::{JSX_GLOBALS_JS, TAULER_MODULE, TAULER_MODULE_JS};

const STATE_JS: &str = include_str!("../tauler-web/js/state.js");

/// Evaluate `scenario` as a module that can import `./state.js` and `"tauler"`, after
/// the shared globals, and return what it left in `globalThis.out`.
fn run(scenario: &str) -> serde_json::Value {
    let runtime = Runtime::new().unwrap();
    runtime.set_loader(
        BuiltinResolver::default()
            .with_module("state.js")
            .with_module(TAULER_MODULE),
        BuiltinLoader::default()
            .with_module("state.js", STATE_JS)
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
