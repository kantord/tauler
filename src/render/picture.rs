//! Pictures: the finished pixels of one D3View drawing (src/d3_view.rs), kept
//! under the drawing's key and placed with `<img src="tauler:picture/<key>">`.
//! Stub — design only; not in the module tree (`render/mod.rs` has no
//! `mod picture;`), so nothing here compiles.
//!
//! TERM: Picture — an SVG document a D3View's `draw` produced, rasterized by
//! tauler once per (key, dpr) and bound by key for every later render. A key
//! names one immutable picture: drawing never repeats for a key, so a new look
//! needs a new key. Layout files never see a Picture; they write `<D3View>`.
//! Avoid: image (any `<img>` source), drawing (the act), bitmap/buffer (CONTEXT.md
//! keeps those away from **Frame**), canvas (suggests drawing commands).
//! DECISION (Q4, answered): "Picture" (internal) and "City scene" (the example).
//!
//! Why it exists — measured by `examples/weather_map_tracer.rs` on a City scene
//! (examples/weather-map/CityScene.jsx), 248×96 card, 220 KB–2.5 MB of SVG:
//! the same drawing as an SVG data URI costs 1.5–19 ms of eval on every tick
//! (the string leaves QuickJS through `json_stringify` and serde) and 12–101 ms on
//! every repaint (takumi re-parses and re-rasterizes it; `preload_layout_images`
//! skips `data:`). As a bitmap bound under a short key: 0.05–0.17 ms and
//! 0.4–0.7 ms, indistinguishable from no drawing at all.
//! Rejected: caching `data:` URIs by hash inside `preload_layout_images` — fixes
//! the repaint cost, keeps the per-tick eval cost and adds hashing on top.
//!
//! DECISION: a Picture is rasterized by tauler's own renderer — an `<img>` of
//! the SVG at the D3View's size × dpr, through `render_frame_rgba` — and bound
//! per render exactly like `tauler:root-bg` (`frame_options`, mod.rs). The tracer
//! did exactly this for its bitmap variant; the result matched the data-URI
//! render (compared by eye on the suburb and gothic samples).
//! Rejected: a resvg dependency of our own — takumi already carries one.
//!
//! DECISION: the SVG comes from the draw runtime (src/d3_view.rs), never from the
//! render runtime, so nothing about a drawing blocks a tick.
//!
//! SPEC: an `<img>` whose `src` names a Picture draws it at the `<img>`'s box;
//! the bitmap is drawn once per (key, dpr) and reused for every later repaint.
//! SPEC: an SVG that does not rasterize leaves no Picture for its key and logs
//! once per key; the rest of the panel draws.
//! SPEC: a layout reload drops every Picture whose key the new layout's tree
//! does not name.
//! SPEC: the frame cache (render/cache.rs) needs no change — a tree that names a
//! Picture by key is identical as long as the key is, and a key never changes
//! meaning (TERM above). The one exception is the moment a Picture arrives: the
//! tree is unchanged but the pixels are not, so arrival invalidates the frames
//! that name the key.
//!
//! RISK (accepted, owner: the user, 2026-10-24; check when Pictures are built):
//! the first raster of a Picture (12–101 ms for a City scene, the data-URI
//! repaint number) lands on the render worker and delays every surface's next
//! repaint once. Kill: a visible stall in a drag that happens to coincide with a
//! new key (theme switch mid-drag) → rasterize Pictures on a side thread and bind
//! them when ready. Cost: small (trace a theme switch under a slider drag, ADR
//! 0042's rule for admitting delays on interactive paths).
//!
//! Pseudocode:
//!
//! ```text
//! draw runtime → render worker:   (key, svg, logical size)
//! render worker: for each dpr in use: bitmaps[(key, dpr)] = render_frame_rgba(img(svg), size × dpr)
//!                drop cached frames naming key; ping the loop's notifier
//! frame_options: bind bitmaps[(key, this dpr)] as "tauler:picture/<key>"
//! ```
