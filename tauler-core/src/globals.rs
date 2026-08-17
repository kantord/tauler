//! Everything a layout file can reach, as source.
//!
//! One string, evaluated verbatim in whichever JavaScript realm is running the layout
//! file — QuickJS on a desktop, the browser's own engine in a page. That is what ADR 0025
//! means by two engines sharing source rather than sharing an implementation: `useEvents`,
//! the handler registry, pointer capture and step rounding are written once here and are
//! the same characters in both.
//!
//! It lives in this crate rather than beside the QuickJS evaluator for exactly that
//! reason — the browser has no rquickjs to reach through.

pub const JSX_GLOBALS_JS: &str = r#"
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
    globalThis.I3Layout = ({ module, children }) => {
        const decls = (Array.isArray(children) ? children : [children]).filter(Boolean);
        const out = __ui_i3_layout({
            children: decls,
            width: ctx.screen_width,
            height: ctx.screen_height,
        });
        if (module) useEvents(module, { gaps: out.gaps });
        return out.panels;
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
    globalThis.Module = ({ bin, children, ...rest }) => {
        const child = Array.isArray(children) ? children[0] : children;
        if (typeof child === 'function') return child(useJSONStream(bin), useEvents(bin, rest));
        return { "bin@": bin, ...rest };
    };
"#;
