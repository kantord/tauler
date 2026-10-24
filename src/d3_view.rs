//! `<D3View>`: a node a layout file draws into with ordinary d3, drawn off the
//! render loop. Stub — design only; not in the module tree (`lib.rs` has no
//! `mod d3_view;`), so nothing here compiles.
//!
//! TERM: D3View — a layout node whose content is drawn by a `draw` function the
//! author writes against a d3 selection of an empty `<svg>`, exactly as on a web
//! page. tauler runs `draw` somewhere else and later places the finished pixels
//! where the node is; the node itself never waits for them.
//! Avoid: chart (a City scene is a D3View and not a chart), canvas, picture (the
//! cached pixels a D3View produces — see render/picture.rs).
//! _Elsewhere_: view (Vega), figure (Observable Plot), an `<svg>` ref handed to
//! d3 in a `useEffect` (React).
//! DECISION (Q8, answered): `<D3View>`. Rejected: `<D3Chart>` (a map is not a
//! chart), `<D3>` (reads as the library, not a place on the screen).
//!
//! ```jsx
//! <D3View key={`city/${coords}/${themeName}`} width={248} height={96}
//!         draw={(svg, d3) => { svg.append("path").attr("d", line(data)); }} />
//! ```
//!
//! DECISION (Q3, answered): d3 support is a node that names d3, not a generic
//! picture API. The author writes idiomatic d3 — `select`, `append`, data joins,
//! axes — not string-built SVG. Rejected: `picture(key, size, () => svgString)`
//! (the earlier recommendation): it works, but it leaves the author building SVG
//! by hand and calling d3 only for the maths.
//!
//! DECISION: `draw` gets a real DOM on both renderers. In a page that is the
//! browser's own; on the desktop it is linkedom (0.18, ISC), bundled with d3.
//! Tracer, 2026-10-24: d3 7 + linkedom evaluated in tauler's QuickJS drew a 60-
//! point chart — a data join of bars, a monotone line, `axisBottom` and
//! `axisLeft` — in 2.15 ms per draw, and its `outerHTML` rasterized correctly
//! through takumi. The bundle is 609 KB minified for both. Rejected: a hand-
//! written DOM shim — d3-selection, d3-axis and data joins together need
//! `querySelectorAll`, `insertBefore`, `ownerDocument` and more; linkedom has them.
//!
//! DECISION (Q2, the model): `draw` runs in a draw runtime — a third QuickJS
//! runtime on its own thread, built the way the reconciler runtime is (ADR 0034).
//! It loads the same layout file, receives the same Stream values and
//! `globals`, one way, and finds each D3View's `draw` in *its own* tree by render
//! path; no closure crosses runtimes. What it hands back is an SVG string, which
//! the render worker rasterizes into a Picture (render/picture.rs) the way it
//! draws everything else: one slot per key, a newer request replaces a queued one
//! (ADR 0023). When the Picture is ready the loop's notifier is pinged (ADR 0024)
//! and the next tick's tree, which still names the same key, draws it.
//! Rejected: running `draw` during the render call — the one-time freeze of
//! 35–670 ms measured for a City scene (examples/weather-map/CityScene.jsx) at
//! load, theme switch and location change. Rejected: a subprocess per drawing
//! (ADR 0014's shape) — d3 would run in Node, outside tauler.
//!
//! SPEC: a D3View whose Picture is not ready yet draws the Picture it last had
//! under any key, else nothing; the rest of the panel never waits for it.
//! SPEC: `draw` runs at most once per `key`. A new key requests a new draw; the
//! old Picture stays on screen until the new one is ready (P3/P4 of the City
//! scene).
//! DECISION (answered): `key` is required. Without it there is no cheap way to
//! tell that nothing changed — hashing `draw`'s inputs means sending them across,
//! the per-tick cost Pictures exist to avoid. A live chart pays with a key such
//! as `cpu/${lastSampleTime}`. Rejected: an automatic key from serialized props.
//! SPEC: `draw` that throws leaves the last Picture in place and logs once per key.
//! SPEC: in a page (ADR 0026/0027) `draw` runs in the page's realm against a real
//! `<svg>` element inside the node, synchronously. Same author code, no Picture.
//!
//! DECISION (Q5, answered): the browser renderer is left out of the first step
//! and included in the first merge.
//!
//! DECISION: the draw runtime may load d3 and linkedom eagerly. Measured 2026-
//! 10-24, tauler-screenshot on a one-span layout, 5 runs: adding the bundle and
//! one `d3.select(...).append("svg")` grew wall time from 161–171 ms to 226–241
//! ms (+65–70 ms) and max RSS from 35 MB to 52–55 MB (+17–19 MB), under the
//! 100 ms / 30 MB that would have forced lazy loading — and it is paid on the
//! draw runtime's own thread, not the loop's. Rejected: lazy loading on the
//! first D3View, for now; it is the fix if a layout ever has none and minds.
//!
//! RISK (accepted, owner: the user, 2026-10-24; check when the draw runtime is
//! built): a layout file that does something nondeterministic at module scope
//! gets a third answer to it (ADR 0034 already has two). Kill: a real layout
//! where the draw runtime's tree disagrees with the render runtime's about which
//! D3Views exist → find D3Views by `key`, not render path. Cost: small.
//!
//! Pseudocode:
//!
//! ```text
//! render runtime (jsx.rs):  D3View({key, width, height, draw}) →
//!     {type: "img", src: "tauler:picture/" + key, style: {width, height}}
//!     + note (key, render path) in this tick's draw requests
//! loop (app.rs):            new key in draw requests → send to draw runtime
//! draw runtime thread:      eval layout against the same streams → find D3View
//!     at render path → doc = linkedom; svg = d3.select(doc.body).append("svg")
//!     → draw(svg, d3) → svg.node().outerHTML → render worker
//! render worker:            rasterize → Picture(key) → ping notifier
//! ```
