//! A local Store: a Store declared with `store(...)` inside a function, kept once per
//! component instance. This file holds its design and the JavaScript that builds its
//! State identity while a layout renders (`local_store_js!`, joined into
//! `JSX_GLOBALS_JS`, one source for the desktop and the page).
//!
//! `CONTEXT.md` defines the terms; ADR 0051's amendment is the decision record, with an
//! example. `src/state.rs` in the `tauler` crate holds the Store machinery, which is
//! unchanged. `store_pass.rs` writes the static half of an identity; a host
//! (`src/state.rs`, `tauler-web/js/state.js`) sees an identity as an opaque string.
//!
//! ## Paths
//!
//! Each is held by the tests named after it: `local_store_*` in `src/jsx.rs` unless
//! another file is named.
//!
//! P1  A component declares a local Store and is written twice with `key`; each keeps
//!     its own value on the next Tick. (`..two_keyed_instances_keep_their_own_values`)
//! P2  A function that declares a local Store is called from two components, and twice
//!     in one; every call has its own value, and swapping the two calls changes nothing.
//!     (`..a_function_called_twice_in_one_component_is_told_apart_by_its_consts`)
//! P3  Rows made in a `.map()` keep their values by `key` when the list is reordered.
//!     (`..rows_of_a_list_keep_their_values_by_key`)
//! P4  A handler writes a local Store; one Tick shows it. (P1's test)
//! P5  A layout reload keeps every local Store whose identity still matches, and fills
//!     in a key its default gained. (`..a_key_added_to_the_default_is_filled_in_by_a_reload`;
//!     `a_local_store_survives_a_layout_reload_..` in `src/app.rs`, with a RISK on Streams)
//! P6  A Unit hook and `draw` code read a local Store their own render declared.
//!     (`..identities_are_the_same_in_every_runtime_and_only_the_render_runtime_drops`)
//! P7  The same layout file runs on a page (ADR 0027). (`tests/page_state_test.rs`)
//!
//! ## The identity
//!
//! DECISION: one function, `store`; a declaration at the top level of a module is
//! shared, one inside a function body is local. Reason: it is the rule a JavaScript
//! variable follows. Rejected: a second name (`useLocal`), a `use…` naming convention.
//!
//! DECISION: a local Store's State identity has three parts, outermost first:
//!
//! ```text
//! local://<instance chain>  >  <call names>  >  <declaration>
//!
//! local://components/Sidebar.jsx#Sidebar > components/Menu.jsx#Menu[left] > menu > lib/toggle.jsx#toggle.s
//! ```
//!
//! - instance chain: the components running when this one was written, each as
//!   `<module>#<function>`, with `[key]` when its `key` prop is a string or a number
//!   (`encodeURIComponent` of it, so no key can spell another link;
//!   `..only_a_string_or_a_number_key_is_part_of_a_link`). It is the chain of *authors*
//!   (the function whose body wrote the JSX), not the output tree: `h` calls a component
//!   at once and children are evaluated before their parent, so the author is the only
//!   ancestor known (`..the_chain_is_the_authors_not_the_parents`,
//!   `..a_sibling_in_front_and_a_prop_change_move_no_value`). The layout's default
//!   export is called by the host and is no link.
//! - call names: for each call on the way down that is the whole value of a `const`,
//!   the name of that `const` ("Which calls get a name" below).
//! - declaration: `<module>#<function>.<const>`, as a shared Store is `<module>#<const>`.
//!
//! The compile pass writes every static part; `[key]` and the order come from a stack
//! kept while rendering. Reason: nothing depends on call order, so reordering, a
//! condition or a loop around a declaration moves no value to another owner. No shared
//! identity starts with `local://`: a module name has no `//`. Rejected: a counter of
//! calls per instance (two calls swap values when their lines are swapped); a
//! function's `.name` alone (`..components_of_one_name_from_two_modules_do_not_share`).
//!
//! DECISION (rule 1, at load): a local `store(...)` must be the whole value of a `const`.
//! A condition or a loop around it is allowed
//! (`..declared_under_a_condition_is_one_store_while_it_is_reached`). `store_pass.rs`
//! lists what else one file is enough to refuse.
//!
//! DECISION (rule 2, while rendering): one identity is declared at most once per render.
//! A second declaration throws; the message names the identity and says to make the
//! repeated part a component with a `key`. One check covers a loop or `.map()` body, a
//! function called twice with no `const` of its own, two siblings with no `key`, and a
//! component called twice as a plain function
//! (`..declared_twice_in_one_render_throws_and_says_what_to_do`,
//! `..beneath_a_repeated_component_names_the_whole_chain`,
//! `..repeated_components_with_no_store_beneath_do_not_throw`). Rejected: refusing loops
//! and conditions at load — they are harmless when the declaration is reached once.
//!
//! DECISION: accepted limits. (a) A render prop is called by the component that
//! receives it, so that component is the author of what it writes
//! (`..a_render_prop_gets_the_chain_of_the_component_that_calls_it`). (b) Writing a
//! component under another author resets its local Stores. (c) Renaming a component, a
//! `const` or a file resets what is named after it, and nothing else. (d) A plain `.js`
//! import gets the pass too (`store_pass.rs`), so it pays the named-call wrapper:
//! 0.30 µs per `const x = f()`.
//!
//! ## The stack
//!
//! ```text
//! frames: [ link ]                    // components and named calls now running:
//! keys:   [ key | nothing ]           //   the first `depth` places; `keys` beside them
//! declared: Map<identity, Proxy>      // this render's
//! live: Map<identity, Proxy>          // the last render's that returned, per render function
//!
//! __tauler_rendering(default_export, root?)   // host: once per layout (per Mount on a page)
//!   returns a function that: frames = [root?], declared = {}, calls default_export
//! __tauler_state_flush()              // host: after a render or a handler returned
//!   after a render: for identity in live not in declared: tell the host to drop it;
//!   live = declared
//!
//! h(type, props, ...children)         // `__tauler_h(factory)`, the only `h`
//!   type is not a function: unchanged
//!   frames[depth] = name_of(type), keys[depth] = key_of(props), depth += 1
//!   try factory(type, props, ...children) finally depth = what it was
//!
//! const menu = toggle(a, b)           // as the pass rewrites it: __tauler_call("menu", toggle, a, b)
//!   the same with "menu" and no key
//!
//! const { open } = toggle(a, b)       // __tauler_destructured_call("<module>:<line>", toggle, a, b)
//!   destructured = "<module>:<line>"; try toggle(a, b) finally destructured = what it was
//!
//! __tauler_local_store(default, "<module>#<function>.<const>", token)   // `store` inside a function
//!   destructured is set: throw, and say to assign the call to a plain `const` first
//!   no render running, and not beneath a top-level `const`: throw
//!   identity = "local://" + frames (each with its key) joined by " > " + declaration
//!   identity in declared: throw rule 2's message
//!   add to declared; return live[identity], or declare to the host and make a Proxy
//! ```
//!
//! A frame is removed by putting `depth` back in a `finally`, never by one pop, so an
//! exception caught inside a render leaves the stack right
//! (`..a_named_call_behaves_as_the_call_the_author_wrote`).
//!
//! ## Dropping
//!
//! DECISION: a local Store that a render does not declare (its component is hidden,
//! its row left the list) is dropped at once
//! (`..a_row_that_leaves_the_list_is_dropped_and_starts_over`). Reason: a hidden
//! component starts from its default, as a React component does; state that must
//! outlive hiding belongs in a shared Store. Rejected: keeping the value until the next
//! reload — a list whose keys come and go leaves an entry behind for every key it had
//! (`..keys_that_come_and_go_leave_nothing_behind`).
//! - Only a render of the render runtime that returned drops anything, and only what
//!   the last such render declared. `__tauler_local_settle` is the last statement of
//!   `__tauler_state_flush`, so no rollback follows a drop.
//! - A render that threw drops nothing
//!   (`..a_render_that_throws_rolls_back_and_drops_nothing`).
//! - The reconciler and draw runtimes never drop; there the host function only forgets
//!   the default that runtime kept (below).
//! - A failed reload drops nothing (`a_failed_reload_keeps_every_local_store` in
//!   `src/app.rs`). After a reload that succeeded, `drop_undeclared_states` there keeps
//!   what the new evaluator's modules declared and what its last render declared.
//!
//! DECISION: a write to a local Store the host does not hold is ignored:
//! `LOCAL_IDENTITY_PREFIX` in `src/state.rs` holds the rule, its reason and its tests.
//!
//! DECISION: a runtime that only reads (reconciler, draw) keeps the default of each
//! Store it declares, and reads from it while the store holds nothing under that
//! identity: its evaluation can reach a declaration before the render runtime does.
//! `Defaults` in `src/jsx.rs` holds the rule. Test:
//! `..a_read_only_runtime_reads_the_default_until_the_store_holds_a_value`.
//! On a page every Mount shares one realm, so each Mount's render function has its own
//! first link (`mount:<n>`) and its own `live` map (`tests/page_state_test.rs`).
//!
//! ## At the top level of a module
//!
//! DECISION: a function that declares a Store, called through a `const` at the top
//! level of a module (`const menu = toggle();`), declares a shared Store. No component
//! is running, so the identity is only names,
//! `local://<module>#menu > lib/toggle.jsx#toggle.s`. Reason: it falls out of the
//! identity rule, and it is the one way to reuse the logic around a shared Store. The
//! pass writes such a call as `__tauler_shared_call`, which is what says "a module is
//! loading". One identity twice there fails the load. Tests:
//! `..declared_through_a_top_level_const_is_a_shared_store`,
//! `..declared_twice_at_the_top_level_of_a_module_fails_the_load`, and
//! `two_mounts_of_one_layout_share_no_local_store_..` in `tests/page_state_test.rs`.
//!
//! DECISION: an element built at the top level of a module, written there or by a
//! function called there, has no author and no render, so a Store beneath it fails the
//! load instead of being shared by every component it is placed in
//! (`..beneath_an_element_built_at_the_top_level_of_a_module_fails_the_load`). A Store
//! declared from a handler throws (`..declared_from_a_handler_throws_and_changes_nothing`).
//!
//! ## Which calls get a name
//!
//! DECISION: the pass names every `const x = f(...)` whose callee is a plain identifier
//! bound in the file; a call with no `const` adds no name, and rule 2 is the only check
//! on calls. Reason: the pass reads one file, and QuickJS asks for an import only after
//! the importer's source is handed over (`SchemaAwareLoader::load` in `src/jsx.rs`), so
//! the pass cannot know which functions declare a Store.
//! Rejected (mechanism A): the pass follows imports to name or refuse only calls of
//! functions that declare a Store. It needs the import graph before load and about five
//! times the code, and still cannot see a function passed as a value: rule 2 would stay.
//!
//! DECISION: a Store declared beneath a call whose value is destructured,
//! `const { open, flip } = toggle();`, throws, and the message says to assign the call
//! to a plain `const` first. Reason: a call that declares a Store is found again by the
//! name of its `const`, and a pattern has no one name. The pass cannot refuse it at
//! load, so it rewrites the call to `__tauler_destructured_call`, and `store` throws
//! while one is running. A destructured call of a function that declares no Store
//! works as written. Rejected: naming the call after the names it binds — renaming one
//! would reset the Store. Tests: `..beneath_a_destructured_call_throws_and_says_to_assign_it_first`,
//! `..a_destructured_call_of_a_function_with_no_store_works_as_written`,
//! `..beneath_a_destructured_call_at_the_top_level_fails_the_load`.
//!
//! DECISION: a component's name reaches run time through a `WeakMap` from the function
//! to its `<module>#<function>`, filled by `__tauler_name`. The pass tags every function
//! a file binds at its top level
//! (`..every_function_bound_at_the_top_level_of_a_file_is_a_named_component`). A
//! component with no tag (defined inside another function, made by a call) costs
//! nothing until a Store is declared beneath it; then the declaration throws
//! (`..beneath_a_component_with_no_name_throws_and_says_where_to_move_it`). Reason:
//! loud, never a silent share. Rejected: the function's `.name` (two of one name
//! share); tagging a function inside a function (set again on every render).
//!
//! ## Cost
//!
//! The budget: `eval_only` in `benches/pipeline.rs` slows by no more than 5% with no
//! local Store in the layout, and one unread local Store costs no more than twice the
//! 3.5 µs an unread shared Store costs. Medians; "before" is the commit before local
//! Stores (`benches/store.rs`, group `eval_with_local`):
//!
//! ```text
//!                                              before      after
//! frame/eval_only (benches/pipeline.rs)        1.665 ms    1.722 ms   +3.4%
//! eval_with_store/read_none/realistic          3.46 µs     3.87 µs    +0.4 µs
//! eval_with_local/rows_60/no_component         461 µs      457 µs
//! eval_with_local/rows_60/no_store             531 µs      557 µs     +4.9%
//! eval_with_local/deep_6x10/no_store           203 µs      219 µs
//! eval_with_local/calls_1000/plain             80.0 µs     80.7 µs
//! eval_with_local/calls_1000/named             79.8 µs     385 µs     0.30 µs a call
//! eval_with_local/rows_60/store_each_unread                685 µs     2.1 µs a Store
//! eval_with_local/rows_60/store_each_read                  668 µs
//! eval_with_local/deep_6x10/store_each_unread              399 µs     3.0 µs a Store
//! ```
//!
//! A local Store costs 1.5 to 3 µs per Tick, by how deep it is (about 0.2 µs per link;
//! identities are not interned). A layout with no local Store pays 3 to 5% per
//! evaluation; the same binary moves by 5% between runs on one machine.
//!
//! RISK (accepted): what a layout with no local Store pays is not explained. Counted in
//! QuickJS alone, the frame costs 0.1 to 0.2 µs per component call: 10 µs for the 59
//! component calls of the `eval_only` fixture, where the bench shows 50 to 90 µs.
//! Kill: `eval_only` is over 5% in a run of interleaved before and after binaries —
//! then profile the bench built with symbols before changing anything.
//! Cost: small to find, unknown to fix.
//!
//! RISK: `key` is an ordinary prop of a built-in component that is an author, and
//! `<Module>` passes every prop it does not know on as a Module prop.
//! Kill: a layout keys a `<Module>` — then the built-in components drop `key` from
//! what they pass on.
//! Cost: small.

/// The JavaScript half, as a literal so `globals.rs` can join it into
/// [`crate::globals::JSX_GLOBALS_JS`] with `concat!`.
macro_rules! local_store_js {
    () => {
        r#"
    // ---- Local Stores (`tauler-core/src/local_store.rs` holds the design). ----
    //
    // `frames`: one link per component and per named call now running, outermost
    // first, in the first `depth` places; what is beyond `depth` is left over and
    // means nothing. A link is a string; a component the compile pass could not tag
    // is the function itself, so nothing is built for it unless a Store is declared
    // beneath.
    // `keys`: the `key` prop of the component at the same index of `frames`, when it
    // has one that is a string or a number. Kept apart and not joined into the link,
    // so a keyed component costs no string until a Store is declared beneath it.
    // `phase`: 0 nothing is running that may declare; 1 a call that is the whole
    // value of a `const` at the top level of a module is running; 2 a render is.
    // `declared`: identity -> Proxy, for what this render declared.
    // `live`: the same for the last render that returned, of the render function now
    // running; `kept` is the render function that returned last.
    // `shared`: every identity declared in phase 1. Never emptied: a module runs once.
    // `destructured`: `<module>:<line>` of the nearest call now running whose value is
    // destructured, or null. Put back in a `finally`, as `depth` is.
    globalThis.__tauler_local = {
        frames: [],
        keys: [],
        depth: 0,
        phase: 0,
        declared: new Map(),
        live: new Map(),
        kept: null,
        pending: null,
        shared: new Set(),
        destructured: null,
    };
    // A tagged function's `<module>#<function>`. Kept beside the function, not on it.
    globalThis.__tauler_names = new WeakMap();
    globalThis.__tauler_name = (fn, name) => {
        if (typeof fn === "function" && !__tauler_names.has(fn)) __tauler_names.set(fn, name);
        return fn;
    };
    // `h`, around the factory each engine has (`__esto_h` in QuickJS, `estoH` in a
    // page). A function tag is a component: its link is on `frames` while it runs.
    // The factory calls it at once, so the push and the truncate are around the
    // factory call; `depth` is put back to what it was, so a link left by anything
    // that threw beneath is removed with it.
    globalThis.__tauler_h = (factory) => {
        const component = (type, props, children) => {
            const local = __tauler_local;
            const depth = local.depth;
            const name = __tauler_names.get(type);
            local.frames[depth] = name === undefined ? type : name;
            // `key` is already a prop of `<D3Figure>`, where it may be an object or an
            // array: only a string or a number is part of a link.
            const key = props == null ? undefined : props.key;
            local.keys[depth] = typeof key === "string" || typeof key === "number" ? key : undefined;
            local.depth = depth + 1;
            try {
                return __tauler_flatten_node(factory(type, props, ...children));
            } finally {
                local.depth = depth;
            }
        };
        // An element's path is kept as short as it was before components had frames.
        return (type, props, ...children) =>
            typeof type === "function"
                ? component(type, props, children)
                : __tauler_flatten_node(factory(type, __tauler_register_handlers(type, props), ...children));
    };
    // What the compile pass turns `const menu = toggle(a, b)` into:
    // `__tauler_call("menu", toggle, a, b)`. The arguments were evaluated by the
    // caller, in the author's order, before `name` is pushed.
    globalThis.__tauler_call = (name, fn, ...args) => {
        const local = __tauler_local;
        const depth = local.depth;
        local.frames[depth] = name;
        local.keys[depth] = undefined;
        local.depth = depth + 1;
        try {
            return fn(...args);
        } finally {
            local.depth = depth;
        }
    };
    // What the compile pass turns `const { open } = toggle(a, b)` into:
    // `__tauler_destructured_call("<module>:<line>", toggle, a, b)`. The call has no one
    // name, so no link is pushed; `destructured` holds the place of the nearest such
    // call now running, and `store` refuses a declaration while it is set.
    globalThis.__tauler_destructured_call = (place, fn, ...args) => {
        const local = __tauler_local;
        const outer = local.destructured;
        local.destructured = place;
        try {
            return fn(...args);
        } finally {
            local.destructured = outer;
        }
    };
    // The same for a `const` at the top level of a module, where `name` is
    // `<module>#<const>`: no component is running, so what the call declares is shared
    // and is declared once, as a Store at the top level is.
    globalThis.__tauler_shared_call = (name, fn, ...args) => {
        const local = __tauler_local;
        const phase = local.phase;
        local.phase = 1;
        try {
            return __tauler_call(name, fn, ...args);
        } finally {
            local.phase = phase;
        }
    };
    // What the compile pass makes a `store` call inside a function call
    // (`<module>#<function>.<const>`): build the State identity from `frames`, refuse
    // a second declaration of it in one render, and answer the Proxy the last render
    // had for it, or declare it to the host.
    globalThis.__tauler_local_store = (initial, declaration, token) => {
        const local = __tauler_local;
        if (local.destructured !== null) {
            throw new Error(
                "store(): `" + declaration + "` is declared beneath a call whose value is " +
                "destructured, at `" + local.destructured + "`. A call that declares a Store " +
                "is found again by the name of the `const` it is assigned to, and a pattern " +
                "has no one name. Assign the call to a plain `const` first and destructure " +
                "that, as in `const menu = toggle(); const { open, flip } = menu;`."
            );
        }
        const frames = local.frames;
        const shared = local.phase === 1;
        // A module is loading: `frames[0]` is the top-level `const`, and a named call
        // beneath it is a plain name. Any other link is a component, written with no
        // render running, which is refused as an element built there with no call is.
        let rendering = local.phase !== 0;
        for (let i = 1; shared && i < local.depth; i++) {
            if (typeof frames[i] !== "string" || frames[i].includes('#')) rendering = false;
        }
        if (!rendering) {
            throw new Error(
                "store(): `" + declaration + "` is declared inside a function, so it is local " +
                "to a component instance and can only be declared while a component renders. " +
                "A handler, a Unit hook and `draw` code run after the render: declare the Store " +
                "in the component and use it from there. To share it, declare it at the top " +
                "level of a file, or assign the call of the function that declares it to a " +
                "`const` there."
            );
        }
        const keys = local.keys;
        let identity = "local://";
        for (let i = 0; i < local.depth; i++) {
            const link = frames[i];
            if (typeof link !== "string") {
                throw new Error(
                    "store(): `" + declaration + "` is declared beneath the component `" +
                    (link.name || "(a function with no name)") + "`, which tauler cannot name. " +
                    "A component above a local Store must be a function or a `const` at the " +
                    "top level of its file: move it there."
                );
            }
            const key = keys[i];
            if (key === undefined) {
                identity += link + " > ";
                continue;
            }
            // Encoded, so that no key can spell another link. Half of a surrogate pair
            // cannot be encoded.
            try {
                identity += link + "[" + encodeURIComponent(key) + "] > ";
            } catch (_) {
                throw new Error(
                    "store(): `" + declaration + "` is declared beneath a component whose " +
                    "`key` is not valid text (it holds half of a surrogate pair). Use " +
                    "another `key`."
                );
            }
        }
        identity += declaration;
        const declared = local.declared;
        if (shared) {
            if (local.shared.has(identity)) {
                throw new Error(
                    "store(): the shared Store `" + identity.slice(8) + "` is declared twice " +
                    "while its file loads, and each declaration needs a name of its own. " +
                    "Assign each call of the function that declares it to its own `const`."
                );
            }
            // Kept as a Store at the top level is: until a reload that does not declare it.
            local.shared.add(identity);
            __tauler_state_declare(identity, initial ?? {}, token);
            __tauler_state_identities.push(identity);
            return __tauler_state_proxy(identity);
        }
        if (declared.has(identity)) {
            throw new Error(
                "store(): the local Store `" + identity.slice(8) + "` is declared twice in " +
                "one render, and each declaration needs a place of its own. Make the " +
                "repeated part a component with its own `key` (a row of a list, the body of " +
                "a loop, a component written twice), or assign each call of the function " +
                "that declares it to its own `const`."
            );
        }
        let proxy = local.live.get(identity);
        if (proxy === undefined) {
            __tauler_state_declare(identity, initial ?? {}, token);
            proxy = __tauler_state_proxy(identity);
        }
        declared.set(identity, proxy);
        return proxy;
    };
    // The layout's render function as a host calls it. `root`, when given, is the
    // first link of every identity declared beneath (a page's Mount).
    globalThis.__tauler_rendering = (render, root) => {
        const local = __tauler_local;
        const own = { live: new Map() };
        return () => {
            local.depth = 0;
            if (root !== undefined) {
                local.frames[0] = root;
                local.keys[0] = undefined;
                local.depth = 1;
            }
            if (local.declared.size !== 0) local.declared.clear();
            local.live = own.live;
            local.pending = null;
            local.phase = 2;
            try {
                const out = render();
                local.pending = own;
                return out;
            } finally {
                local.phase = 0;
                local.depth = 0;
            }
        };
    };
    // After a render that returned and whose writes are flushed
    // (`__tauler_state_flush`): a local Store the last render had and this one did not
    // declare is dropped, in every host that stores anything. A render that threw
    // never gets here (`__tauler_state_abort`), so it drops nothing.
    globalThis.__tauler_local_settle = () => {
        const local = __tauler_local;
        const own = local.pending;
        local.pending = null;
        local.kept = own;
        const live = own.live;
        const declared = local.declared;
        if (live.size === 0 && declared.size === 0) return;
        for (const identity of live.keys()) {
            if (!declared.has(identity)) __tauler_state_drop(identity);
        }
        own.live = declared;
        live.clear();
        local.declared = live;
    };
    // Every identity this runtime holds a declaration of: what its modules declared,
    // then what the last render that returned declared.
    globalThis.__tauler_state_declared = () => {
        const kept = __tauler_local.kept;
        return kept === null
            ? __tauler_state_identities.slice()
            : [...__tauler_state_identities, ...kept.live.keys()];
    };
    // The built-in components that call a function they are handed (`<Module>`'s
    // render prop) are authors like any other, so they need a name too.
    for (const name of ["Panel", "I3Layout", "Contents", "Workspaces", "ConfigFile", "D3Figure", "Module"])
        __tauler_name(globalThis[name], "tauler#" + name);
"#
    };
}
pub(crate) use local_store_js;
