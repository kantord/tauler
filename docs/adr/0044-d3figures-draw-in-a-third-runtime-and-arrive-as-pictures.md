# D3Figures draw in a third runtime and arrive as Pictures

A **D3Figure**'s draw function never runs on the loop. The render runtime only records
that a figure with a given key and size exists, and puts an `<img src="tauler:picture/<key>">`
in the tree. A **Draw runtime** — a third QuickJS runtime on its own thread, built the way
the reconciler runtime is (ADR [0034](0034-hooks-run-in-a-second-runtime.md)) — loads the
same layout file, finds the figure by key in *its own* tree, runs `draw` against an empty
`<svg>`, and hands the markup back. The Render worker rasterizes it once into a **Picture**,
and every later Frame binds the Picture under that src the way `tauler:root-bg` is bound.
Until it arrives the figure shows its previous Picture, or nothing.

## Why not run `draw` in the render call

The first tracer did exactly that, and measured why it cannot stay: a city map drawn with
d3 took 35–670 ms to build, and a 60-node force layout ~390 ms (d3-force runs about 12×
slower in QuickJS than in Node). On the loop, each of those is a Lagged Tick at load, at
every theme switch and whenever the data moves. In the Draw runtime the same cost is a
Picture that shows up a fraction of a second later.

## Why not ship the SVG through the tree

The tracer's other measurement. As an SVG data URI in `<img src>`, a 0.2–2.5 MB drawing
cost 1.5–19 ms of eval on every Tick (the string leaves QuickJS through `json_stringify`
and serde) and 12–101 ms on every repaint (takumi re-parses and re-rasterizes it). Named by
a short key and bound as a bitmap: 0.05–0.17 ms and 0.4–0.7 ms — the same as no drawing.

## Why a key, and why it is required

A key is the only cheap way to know that nothing changed. Without one, tauler would have to
send `draw`'s inputs across to compare them, which is the per-Tick cost above again. So the
author says it: a figure whose key is unchanged is never drawn again, and a figure that
should look different gets a new key. A key names one Picture for good, which is also what
lets the frame cache stay keyed by content: a tree that names the same keys shows the same
Pictures, and a frame's cache key changes only when one of *its* Pictures appears, changes
or disappears.

## Consequences

**The layout file is evaluated in three runtimes.** Module top-level side effects run once
per runtime, as ADR 0034 already accepted for two.

**No closure crosses runtimes.** The Draw runtime finds `draw` in its own evaluation of the
layout, by key, so a figure is drawn from that runtime's view of the data, which can trail
the bar's by one round.

**A figure's identity across Ticks is its position among the figures, in document order.**
That is what lets a figure whose key changed keep showing its previous Picture while the
new one is drawn. Superseded Pictures are dropped as soon as their successor lands; the live
check that found them piling up measured ~100 KB/s for a figure redrawn every second.

**The page does not use any of this.** In a browser (ADR
[0026](0026-the-web-renderer-emits-dom.md), [0027](0027-the-browsers-own-engine-runs-the-layout-file.md))
the same `draw` runs in the page's realm against a real `<svg>`, placed inline; there is no
Draw runtime and no Picture.
