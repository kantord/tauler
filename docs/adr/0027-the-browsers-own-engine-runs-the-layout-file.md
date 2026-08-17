# On the web, the layout file runs in the browser's JS engine

QuickJS does not go to the web. A layout file rendered in a page is transformed to plain
JavaScript ahead of time and evaluated by whatever engine the browser has. The wasm bundle
supplies the Rust half — the UI components, the theme resolver, the walk — and the page
supplies the realm.

So tauler has two JavaScript engines, and a layout file can behave differently in each.
That is the trade, and it was not chosen for elegance.

## Why not QuickJS in wasm

It does not build. `rquickjs-sys` compiles QuickJS from C, and against
`wasm32-unknown-unknown` clang stops at `libregexp.c:24: fatal error: 'stdlib.h' file not
found` — there is no sysroot. Reaching it means a wasi-sdk toolchain and a WASI shim in the
page, for a second engine inside wasm inside an engine.

`optative-script` inherits the same problem: it depends on `rquickjs` unconditionally, with
no feature to ask for the oxc half alone. This is why the JSX transform runs at build time
rather than in the browser — every documentation example is known when the site is built, so
a native binary transforms them and the page receives `.js`. When a layout file genuinely
needs to be edited live in a browser, the fix is a feature flag in `optative-script`
splitting transform from runtime; both callers then reach the same function and nothing
built here is discarded.

## Why the divergence is acceptable

Layout files are small, declarative, and ES2023 at most. The globals they are evaluated
against — `_jsx`, `useStringStream`, the handler registry, pointer capture, step rounding
— are a single string in `src/jsx.rs`, and that string is evaluated verbatim in whichever
realm is running. The shared surface is shared source, not a reimplementation.

What is *not* shared is registration: on the desktop `rquickjs` sets the globals, and in a
page the wasm glue does. Both are generated from the same `UI_COMPONENTS` table, which
already carries everything either needs — `{module_path, export_name, global_name}` — so no
JavaScript is written by hand on either side.

## Why this is the right shape for everything after documentation

Every future transport named when this was decided — a pure-JS collector, a web worker, an
Electron host, an SSH bridge — is something that lives in the page's realm and pushes values
in. With QuickJS in wasm, each of those has to marshal across the wasm boundary into a
second engine. Here they are objects in the same realm as the layout file, and the seam
they plug into is the one that already exists: a Stream is `(bin, script) → latest line`,
pushed in, and nothing in `jsx.rs` knows those come from subprocesses.

That seam gets a name in `CONTEXT.md` — a **Transport** — precisely so the subprocess
implementation stops being mistaken for the concept.

## Consequences

**A layout file can pass on the desktop and fail in a page, or the reverse.** Nothing
catches this except running both. The documentation examples are the only layout files
currently run in both, which makes them a de facto conformance suite and worth treating as
one.

**The browser needs no JSX transformer.** It also cannot accept a `.jsx` file, which means
the web renderer is not yet a place a user edits a layout — only a place one is displayed.

**Latest-value state lives in Rust.** `useStringStream`, `registerModule` and the writer
that feeds them are `#[wasm_bindgen]` exports over Rust state, not a JavaScript map, so the
key normalisation and the missing-value behaviour have one implementation.
