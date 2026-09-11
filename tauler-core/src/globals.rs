//! Everything a layout file can reach, as source.
//!
//! One string, evaluated verbatim in whichever JavaScript realm is running the layout
//! file — QuickJS on a desktop, the browser's own engine in a page. That is what ADR 0027
//! means by two engines sharing source rather than sharing an implementation: `useEvents`,
//! the handler registry, pointer capture and step rounding are written once here and are
//! the same characters in both.
//!
//! It lives in this crate rather than beside the QuickJS evaluator for exactly that
//! reason — the browser has no rquickjs to reach through.

pub const JSX_GLOBALS_JS: &str = r#"
    // `h`'s per-node reshape: the factory's `{type, props, children}` into the layout
    // tree's flat shape. In JavaScript rather than a Rust callback because it is pure data
    // movement and the boundary is not free — as a callback it converted every node to
    // `serde_json` and back once per element, measured at 60% of an entire evaluation.
    //
    // One level deep is enough: every child came from its own `h` call and is already
    // flat. `flatten::flatten_passthrough` stays as the whole-tree safety net for anything
    // that reached the tree another way.
    globalThis.__tauler_flatten_node = (v) => {
        if (v === null || typeof v !== 'object' || Array.isArray(v)) return v;
        if (!('type' in v) || !('props' in v)) return v;
        const out = { type: v.type };
        const p = v.props;
        if (p !== null && typeof p === 'object') for (const k in p) out[k] = p[k];
        out.children = 'children' in v ? v.children : [];
        return out;
    };
    globalThis.useJSONStream = (bin, script) => {
        const str = useStringStream(bin, script);
        if (!str) return null;
        try { return JSON.parse(str); } catch { return null; }
    };
    // `props` are merged into the module's init event (see merge_module_props in app.rs),
    // so they are load-bearing. Registering the same bin more than once contributes
    // every declaration's props (see registerModule): one subprocess, union of props.
    globalThis.useEvents = (bin, props) => {
        registerModule(bin, props ?? {});
        return new Proxy({}, {
            get: (_, type) => (args) => ({
                channel: bin,
                event: { type: String(type), ...args }
            })
        });
    };
    // Declaration only: <I3Layout> reads these, positions them, and emits the
    // real <panel> nodes. Kept a marker rather than a panel because a panel's
    // position depends on every sibling declared before it.
    globalThis.Panel = (props) => ({ ...props, __i3panel: true });
    // Dispatch only — the layout arithmetic is `ui::components::i3_layout`.
    // The gaps must be registered here rather than in Rust: registration is a
    // JS-side call, and a Rust component has no context to make one.
    //
    // <Workspaces> is handled separately from plain <Panel>s: its thickness isn't
    // declared, it's measured (see `src/workspaces.rs`, which needs takumi and so
    // can't live beside i3_layout.rs in this takumi-free crate — ADR 0010). The
    // render-prop was already called eagerly when `h()` evaluated <Workspaces>, so
    // by the time it is a child here it is just `{__workspaces: true, wrapperTree}`.
    // A misplaced or repeated <Workspaces> degrades rather than fails the whole bar
    // (same rule as an unknown <Panel> anchor): only the last declared one is used,
    // and anything after it is silently dropped along with it — but "silently" only
    // as far as the layout goes. There is no `console` in this runtime to warn from
    // here, so `misplaced` crosses into `__workspaces_layout` and is reported from
    // Rust (`src/jsx.rs`), where `tracing::warn!` actually reaches a log.
    globalThis.I3Layout = ({ module, children }) => {
        const all = (Array.isArray(children) ? children : [children]).filter(Boolean);
        const workspacesDecls = all.filter((d) => d.__workspaces);
        const lastWorkspaces = workspacesDecls[workspacesDecls.length - 1];
        const misplaced = !!lastWorkspaces
            && (workspacesDecls.length > 1 || all[all.length - 1] !== lastWorkspaces);
        const decls = all.filter((d) => !d.__workspaces);
        const out = __ui_i3_layout({
            children: decls,
            width: ctx.screen_width,
            height: ctx.screen_height,
        });
        let panels = out.panels;
        let gaps = out.gaps;
        if (lastWorkspaces) {
            const freeX = gaps.left;
            const freeY = gaps.top;
            // Declared gaps can exceed the screen (a squished panel from a
            // transient bad DPR reading, or just a misconfigured layout) —
            // clamped here rather than left negative, since this crosses into
            // Rust as a `u32` and a negative value would throw and drop the
            // whole tick's panel/gaps update, not just this one (issue #525
            // bug #4).
            const freeW = Math.max(0, ctx.screen_width - gaps.left - gaps.right);
            const freeH = Math.max(0, ctx.screen_height - gaps.top - gaps.bottom);
            const frame = __workspaces_layout(lastWorkspaces.wrapperTree, freeW, freeH, misplaced);
            panels = panels.concat(
                frame.panels.map((p) => ({ ...p, x: p.x + freeX, y: p.y + freeY }))
            );
            gaps = {
                left: gaps.left + frame.gaps.left,
                right: gaps.right + frame.gaps.right,
                top: gaps.top + frame.gaps.top,
                bottom: gaps.bottom + frame.gaps.bottom,
            };
        }
        if (module) useEvents(module, { gaps });
        return panels;
    };
    // Declaration only, like <Panel>: <Workspaces> reads the render-prop's return
    // value and hands it to the native half. `Contents` is the placeholder the
    // wrapper renders in place of the real tiled workspace area — a plain div
    // carrying a marker `measure_content_rect` (`src/workspaces.rs`) looks for.
    globalThis.Contents = (props) => ({ ...props, type: "div", "data-tauler-workspaces-content": true });
    globalThis.Workspaces = ({ children }) => {
        const render = Array.isArray(children) ? children[0] : children;
        if (typeof render !== "function") {
            throw new Error("<Workspaces> needs one function child: {(Contents) => <Wrapper>...}");
        }
        return { __workspaces: true, wrapperTree: render(globalThis.Contents) };
    };
    // Handlers that are functions cannot cross the JSON boundary, so they stay here
    // and the tree carries `{$handler: n}` instead (ADR 0021). Rebuilt every tick;
    // a drag holds its own reference in __tauler_captured, so clearing is safe
    // mid-gesture.
    globalThis.__tauler_handlers = [];
    globalThis.__tauler_captured = null;
    // Only real elements. A component's props are its own — turning `on_change` into
    // a handler id would hand <Slider> an object where it expects a function.
    // Park a function in the registry and hand back the reference the tree carries.
    // Anything that is not a function passes through, so a plain intent array is left
    // exactly as written. A JS shim that calls a Rust component directly has to use
    // this itself — those props never pass through `h`.
    globalThis.__tauler_handler_ref = (fn) =>
        typeof fn === "function" ? { $handler: __tauler_handlers.push(fn) - 1 } : fn;
    globalThis.__tauler_register_handlers = (type, props) => {
        if (typeof type !== "string" || !props) return props;
        let out = null;
        for (const key in props) {
            if (key.startsWith("on_") && typeof props[key] === "function") {
                out = out ?? { ...props };
                out[key] = __tauler_handler_ref(props[key]);
            }
        }
        return out ?? props;
    };
    globalThis.__tauler_capture_handler = (id) => {
        __tauler_captured = __tauler_handlers[id] ?? null;
    };
    globalThis.__tauler_release_handler = () => { __tauler_captured = null; };
    // `id < 0` means the captured one, which outlives the tick it was registered in.
    // A handler may return one intent or several; downstream only ever sees an array.
    globalThis.__tauler_intents = (out) =>
        out == null ? null : (Array.isArray(out) ? out : [out]);
    // Rounding to a step is how a control keeps a drag to one message per distinct
    // value instead of one per pixel: a motion producing what was just sent is
    // skipped. `step` of 0 rounds nothing and only clears the float noise — binary
    // floating point turns 0.1 steps into 0.30000000000000004.
    globalThis.__tauler_snap = (v, step) =>
        Math.round((step > 0 ? Math.round(v / step) * step : v) * 1e6) / 1e6;
    globalThis.__tauler_invoke_handler = (id, pointer) => {
        const fn = id < 0 ? __tauler_captured : __tauler_handlers[id];
        if (typeof fn !== "function") return null;
        return __tauler_intents(fn(pointer));
    };
    // Normalizes JSX children into a flat, order-preserved array — the
    // shape every config-format wrapper (ADR 0041) needs before it can
    // decide what to do with them. `.flat(Infinity)` absorbs a `.map()`'s
    // nested array; the filter drops the holes JSX itself produces from a
    // conditional (`{cond && <X/>}` evaluates to `false` when `cond` is
    // falsy). Throwing on anything else is deliberate: a bare string or
    // number slipping through (stray text, whitespace between elements)
    // would otherwise reach a wrapper's `key`/`combine` as a marker with no
    // properties, producing corrupted output several calls away from the
    // actual mistake — this turns that into an error at the mistake itself.
    globalThis.flattenChildren = (children) => {
        const flat = (Array.isArray(children) ? children : [children]).flat(Infinity);
        return flat.filter((c) => {
            if (c === null || c === false || c === undefined) return false;
            if (typeof c !== 'object') {
                throw new Error(
                    'flattenChildren: expected a marker object, got ' + JSON.stringify(c) +
                    ' — stray text or whitespace between elements is not a valid child here'
                );
            }
            return true;
        });
    };
    // Groups `items` by `key(item)`, folding each group through `combine`
    // (ADR 0041). `combine` sees `undefined` on a key's first occurrence —
    // a `Map` already returns that from `.get()` on a missing key, and
    // already preserves a key's original position when `.set()` updates
    // it — so this is the whole mechanism, no separate "have we seen this
    // key" bookkeeping needed.
    //
    // There is deliberately no default `combine`: whether a second item
    // sharing a key replaces the first, extends it, or should never happen
    // is specific to the target format (rofi replaces per property,
    // systemd's `Environment=` accumulates), and a shared default would be
    // right for some formats and silently wrong for others.
    globalThis.collate = (items, key, combine) => {
        const buckets = new Map();
        for (const item of items) {
            const k = key(item);
            buckets.set(k, combine(buckets.get(k), item));
        }
        return buckets;
    };
    // A Unit factory for one external text file (ADR 0040). `path` is the
    // key, `render()` is called fresh every time the declared value is
    // needed, and `observe` reads the file back with builtins that already
    // exist (`read`/`exists`) — this is boilerplate factored out of a Unit
    // definition, not a new capability: nothing here `unit()`, `sh`, `read`
    // and `exists` couldn't already do by hand (see the docs' first draft
    // of this example, before `ConfigFile` existed).
    //
    // `apply`, if given, runs after every write with the path and the text
    // just written — the hook for a target that has to be told, unlike
    // rofi, which reads its file fresh on every launch. It is ordinary
    // JavaScript closed over by `write`, not an Item prop, because a prop
    // is serialised into the hook-dispatch batch (ADR 0034) and a function
    // does not survive that crossing.
    //
    // `value` tells the declared side from the observed side by shape:
    // `observe` always returns `{content}`, and a declared `<ConfigFile/>`
    // never has one (it takes no props), so the branch is exact rather
    // than a heuristic.
    globalThis.ConfigFile = ({ path, render, apply }) => {
        function write() {
            const rendered = render();
            sh`printf '%s' ${rendered} > ${path}`;
            if (apply) apply(path, rendered);
        }
        return unit({
            key: () => path,
            value: (f) => ('content' in f ? f.content : render()),
            reconciler: optativeSet({
                observe: () => (exists(path) ? [{ content: read(path) }] : []),
            }),
            enterOne: write,
            updateOne: write,
        });
    };
    globalThis.Module = ({ bin, children, ...rest }) => {
        const child = Array.isArray(children) ? children[0] : children;
        if (typeof child === 'function') return child(useJSONStream(bin), useEvents(bin, rest));
        return { "bin@": bin, ...rest };
    };
"#;
