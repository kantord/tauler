//! `<D3Figure>`: a node a layout file draws into with ordinary d3, drawn off the
//! loop and shown on the desktop as a Picture. Why it works this way — a third
//! runtime, a required key, markup that never travels through the tree — is ADR
//! 0044; why d3 and linkedom are downloaded rather than shipped is ADR 0045.
//!
//! ```jsx
//! <D3Figure key={`city/${coords}/${themeName}`} width={248} height={96}
//!           draw={(svg, d3) => { svg.append("path").attr("d", line(data)); }} />
//! ```
//!
//! # From `<D3Figure>` to pixels
//!
//! 1. **Render runtime.** The `D3Figure` global (`tauler-core/src/globals.rs`)
//!    throws without a key, hands `draw` to `registerFigure` — which the render
//!    runtime ignores, so `draw` never runs on the loop — and returns
//!    `<img src="tauler:picture/<key>@<width>x<height>">` at the given size: the
//!    Picture's name is the key plus the size, so a resized figure names a new
//!    Picture. From here on "key" means that name. The eval reads one
//!    [`crate::jsx::FigureCall`] per such `<img>` back from its tree, in document
//!    order — from the tree, not from the calls, because an element built at
//!    module scope calls `D3Figure` once, before any eval.
//! 2. **Loop.** [`requests::Requests`] turns those calls into a plan: the keys not
//!    asked for yet go to the Draw runtime together with every key on screen
//!    (which bounds what it keeps pending), and a figure whose key changed shows
//!    its previous key's Picture meanwhile (`picture::set_fallback`).
//! 3. **Draw runtime** ([`draw`]). Its own thread evaluates the same layout file
//!    against the inputs of the eval that requested it, finds the figure by key in
//!    *its own* tree (keeping each key's latest `registerFigure`, and those made
//!    while the module loaded, for as long as the key is in that tree), runs `draw(svg, d3)` against an empty `<svg>` in a linkedom
//!    document, and sends the markup back, pinging the loop's notifier.
//! 4. **Loop.** Each arrival from the current Draw runtime is stored as its
//!    key's Picture ([`crate::render::picture`]), and Pictures no figure names
//!    any more are dropped.
//! 5. **Render worker.** Every frame binds each Picture its tree names under the
//!    `<img>`'s src, rasterized once per Picture and size; the frame cache misses
//!    exactly when one of a frame's own Pictures appears, changes or goes.
//!
//! d3 and linkedom come from [`assets`] (the pinned files and their cache) through
//! [`fetch`] (the download, off the loop). [`import`] turns d3's UMD build into
//! the `"d3"` ES module, so a layout can also `import * as d3 from "d3"` for the
//! maths.
//!
//! In a page (ADR 0026/0027) none of this runs: the web renderer marks each such
//! `<img>` with its key (`tauler-core/src/dom.rs`), and the page's glue
//! (`tauler-web/js/runtime.js`) swaps it for a real `<svg>` and runs `draw` there,
//! in the page's realm. Same author code, no Picture.
//!
//! # Behaviour authors can rely on
//!
//! - A D3Figure can be built anywhere an element can, module scope included.
//! - `draw` runs once per key and size while both stay on screen; a figure that
//!   should look different at the same size needs a new key. A resized figure is
//!   drawn again at its new size, showing the old size's Picture meanwhile. A key
//!   that leaves and comes back is drawn again, and so is every figure after a
//!   layout reload, since its `draw` may have changed.
//! - Until its drawing arrives, a figure shows its previous key's Picture, or
//!   nothing the first time. The rest of the bar never waits for it. A layout
//!   reload is no exception: each figure keeps showing what it showed until the
//!   new Draw runtime's drawing arrives, and a drawing the old runtime finishes
//!   after the reload is ignored.
//! - A `draw` that throws, or runs past [`draw::TIME_LIMIT`] and is interrupted,
//!   leaves the figure's previous Picture in place and is logged.
//! - The Draw runtime has no `fetch`, filesystem or shell builtins, so `d3.json`
//!   and friends throw. Its `setTimeout`, `setInterval`, `requestAnimationFrame`
//!   and their cancels exist but never fire: d3-timer schedules a frame as soon as
//!   anything starts one (`d3.forceSimulation()` does on construction) and would
//!   otherwise throw. A Picture is drawn once — run a simulation to rest with
//!   `.tick()`; a transition never plays.
//! - linkedom has no layout engine, so `getBBox`, `getComputedTextLength` and
//!   `getBoundingClientRect` measure nothing: d3 code that sizes labels by
//!   measuring text works in a page, not on the desktop.
//! - What a `draw` costs is the author's business, because it is off the loop. A
//!   60-node force layout takes ~390 ms in QuickJS (d3-force runs ~12× slower than
//!   in Node); a line chart with axes, a few ms. d3 and linkedom load on the first
//!   draw, not before: +65–70 ms and +17–19 MB once, on the Draw runtime's thread,
//!   and never for a layout with no D3Figure.
//!
//! # Known limitations
//!
//! - **A third answer to module-scope code.** The layout file is evaluated in three
//!   runtimes (ADR 0034 accepted two), so nondeterministic module-scope code can
//!   give the Draw runtime a different tree, even from the same inputs. A key the
//!   Draw runtime's tree does not have is not drawn, and is warned about once per
//!   layout load (later misses log at debug).
//! - **First rasterization on the Render worker** — see [`crate::render::picture`].

pub mod assets;
pub mod draw;
pub mod fetch;
pub mod import;
pub mod requests;
#[cfg(test)]
pub(crate) mod test_support;
