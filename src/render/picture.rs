//! Pictures: vector drawings a layout file hands tauler once, under a key, and
//! places with `<img src="tauler:picture/<key>">`. Stub — design only; not in the
//! module tree (`render/mod.rs` has no `mod picture;`), so nothing here compiles.
//!
//! TERM: Picture — an SVG document a layout file built itself (with d3, or by
//! hand), registered under a key it chose, and drawn by tauler once per physical
//! size. A key names one immutable picture: registering an existing key never
//! rebuilds it, so a new look needs a new key.
//! Avoid: image (any `<img>` source), drawing (the act), bitmap/buffer (CONTEXT.md
//! keeps those away from **Frame**), canvas (suggests drawing commands).
//! ASK (Q4): the names "Picture" and "City scene". Recommend: both as written.
//! PROVISIONAL (Q4): the name.
//!
//! Why it exists — measured by `examples/weather_map_tracer.rs` on a City scene
//! (examples/weather-map/CityScene.jsx), 248×96 card, 220 KB–2.5 MB of SVG:
//! the same picture as an SVG data URI costs 1.5–19 ms of eval on every tick
//! (the string leaves QuickJS through `json_stringify` and serde) and 12–101 ms on
//! every repaint (takumi re-parses and re-rasterizes it; `preload_layout_images`
//! skips `data:`). As a bitmap bound under a short key: 0.05–0.17 ms and
//! 0.4–0.7 ms, indistinguishable from no picture at all.
//!
//! ASK (Q3): grow tauler a Picture primitive (this file), or keep tauler as is
//! and have the fetcher subprocess draw the scene to a PNG file itself (d3 in
//! Node, outside tauler; `<img src=path>` is already cached by `src`).
//! Recommend: the Picture — it is the general fix for any large d3 drawing in a
//! layout (dashboards, maps), and the subprocess route takes d3 out of tauler.
//! PROVISIONAL (Q3): the layout-file API, recommended shape:
//!
//! ```js
//! <img src={picture("city/52.520,13.405/248x96/" + themeKey,
//!                   { width: 248, height: 96 },
//!                   () => citySceneSvg(...))} />
//! ```
//!
//! `build` is called only when the key is new, so the per-tick cost is one
//! lookup and the key string — never the SVG. Rejected: `picture(svg)` returning
//! a content hash — the SVG would cross into Rust and be hashed on every tick,
//! which is the data URI's eval cost again. Rejected: caching `data:` URIs by
//! hash inside `preload_layout_images` — fixes the repaint cost, keeps the
//! per-tick eval cost (1.5–19 ms) and adds hashing on top.
//!
//! DECISION: a Picture is rasterized by tauler's own renderer — an `<img>` of
//! the SVG at the picture's size × dpr, through `render_frame_rgba` — and
//! bound per render exactly like `tauler:root-bg` (`frame_options`, mod.rs).
//! The tracer did exactly this for its bitmap variant; the result is pixel-for-
//! pixel the data-URI render (compared by eye on the suburb and gothic samples).
//! Rejected: a resvg dependency of our own — takumi already carries one.
//!
//! SPEC: `picture(key, size, build)` returns `"tauler:picture/" + key`; `build`
//! runs at most once per key per loaded layout.
//! SPEC: an `<img>` whose `src` names a Picture draws it at the `<img>`'s box;
//! the bitmap is drawn once per (key, dpr) and reused for every later repaint.
//! SPEC: a Picture whose `build` throws, or returns something that is not an SVG
//! document, draws nothing and logs once per key; the rest of the panel draws.
//! SPEC: a layout reload drops every Picture the new layout does not register.
//! SPEC: the frame cache (render/cache.rs) needs no change — a tree that names a
//! Picture by key is identical as long as the key is, and a key never changes
//! meaning (TERM above).
//!
//! DECISION: `build` runs inside the render call, on the loop thread. It is the
//! one-time cost Q2 asks about (58–605 ms for a City scene). Rejected: building
//! in a worker runtime — QuickJS values cannot leave their runtime, so the
//! geometry and d3 would have to be loaded twice.
//!
//! RISK: the first raster of a Picture (12–101 ms for a City scene, the data-URI
//! repaint number) lands on the render worker and delays every surface's next
//! repaint once. Kill: a visible stall in a drag that happens to coincide with a
//! new key (theme switch mid-drag) → rasterize Pictures on a side thread and bind
//! them when ready. Cost: small (trace a theme switch under a slider drag, ADR
//! 0042's rule for admitting delays on interactive paths).
//!
//! Pseudocode:
//!
//! ```text
//! jsx.rs, beside useStringStream:   picture(key, size, build)
//!     if !registry.contains(key): registry[key] = (size, build())   // QuickJS → String once
//!     return "tauler:picture/" + key
//! render/mod.rs, preload_layout_images: for each img src "tauler:picture/<key>":
//!     for each dpr in use: if !bitmaps.contains((key, dpr)):
//!         bitmaps[(key, dpr)] = render_frame_rgba(img(svg_data_uri), size × dpr)
//! render/mod.rs, frame_options: bind bitmaps[(key, this dpr)] as "tauler:picture/<key>"
//! ```
