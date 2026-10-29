# D3Figures draw in a third runtime and arrive as Pictures

A **D3Figure**'s draw function never runs on the loop. The render runtime only puts an
`<img src="tauler:picture/<key>">` at the figure's size in the tree, and the loop reads from
the tree which figures exist. A **Draw runtime** — a third QuickJS runtime on its own thread, built the way
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
layout, by key. *(Amended below: that evaluation now uses the inputs of the eval that asked
for the figure, so it no longer trails the bar's by one round.)*

**A figure's identity across Ticks is its position among the figures, in document order.**
That is what lets a figure whose key changed keep showing its previous Picture while the
new one is drawn. Superseded Pictures are dropped as soon as their successor lands; the live
check that found them piling up measured ~100 KB/s for a figure redrawn every second.

**The page does not use any of this.** In a browser (ADR
[0026](0026-the-web-renderer-emits-dom.md), [0027](0027-the-browsers-own-engine-runs-the-layout-file.md))
the same `draw` runs in the page's realm against a real `<svg>`, placed inline; there is no
Draw runtime and no Picture.

## Amendment: the size is part of a Picture's name

The key alone let an author get the identity wrong without noticing: a figure resized with
an unchanged key was never drawn again, and its old drawing was scaled into the new box. So
tauler adds what it already knows. The `D3Figure` global names the Picture
`<key>@<width>x<height>` and registers `draw` under that name, so a resized figure names a
new Picture and is drawn again, showing the old size's Picture meanwhile, and one key at two
sizes is two Pictures. Everything downstream already keyed on the name and did not change.
"A key names one Picture for good" now reads "a key names one Picture per size for good";
the key is still required and still the author's to change when the drawing should.

## Amendment: a draw evaluates the inputs of the eval that asked for it

The Draw runtime used to evaluate the layout with the bar's Stream values and `globals` as
they were when its round started. When those had moved on since the eval that asked, its
tree no longer named the key it was asked to draw, and the figure was not drawn. Issue #713
measured it: a stream ticking every 50 ms against a ~200 ms draw missed between 4% and over
90% of rounds, and a render that writes to `globals` never drew at all, because by the time
the Draw runtime read them the render had already changed them.

So each eval records what it ran with: an `Arc` snapshot of `globals` as they were before
the render, and of every Stream value: not only the ones the render read, because a
figure's `draw` runs against the same snapshot and may read others. A tree with no
`<D3Figure>` keeps no Stream values, since nothing draws from it. Draw requests carry it, and the Draw
runtime evaluates with the newest batch's snapshot, once per round. One snapshot is enough:
a pending key is always one the newest batch names, so that eval names every pending key.
The draw evaluator's `globals` are its own, filled from the snapshot and never written back.
Taking the snapshot costs ~4 µs per eval.

`ctx` is not in the snapshot. The render, reconciler and Draw runtimes are all built from
the same `ctx` together, on every layout load, so it cannot differ between them; a future
`ctx` refresh has to reach all of them at once.

The reconciler (ADR 0034) still reads fresh values. A Sweep run against slightly stale
values converges on the next round; a draw cannot, because it has to find the exact key it
was asked for.

With the inputs fixed, a deterministic render names the same keys in both runtimes, so an
`UnknownKey` that remains means a nondeterministic render (module-scope state, `Date.now`,
`Math.random`). The first one in a layout load is logged as a warning, instead of the
debug line that used to treat it as expected; later ones stay at debug. The limit is per
load, not per key: a key built from `Date.now` or a counter is new on every eval, and a
per-key limit would warn on every draw round.

## Amendment: an object key names its Picture by hash

`String(key)` gave every object key the same name, `[object Object]`, so figures with
different object keys silently shared one Picture, and an author whose natural key was the
figure's inputs had to turn them into a string by hand. So a key may now be any JSON value.
A string, number or boolean keeps its name, `String(key)`, unchanged. An object or array is
named `json-<first 16 hex digits of sha256(JSON.stringify(key))>`, and the size follows as
before: `json-…@<width>x<height>`. `JSON.stringify` follows insertion order, so key order
is part of the name.

A key that JSON would represent lossily throws, naming the path of the bad value: a
function, `undefined`, a symbol, `NaN`, `±Infinity`, a `BigInt`, a cycle, a `Date`, an
array hole, or an object whose prototype is not `Object.prototype`, `Array.prototype` or
null. A silent drop would be the stale-Picture bug in a new form: two keys that differ only
in what JSON drops would name one Picture.

Unit keys are not checked this way. The desktop compares a Unit key by the text
`JSON.stringify` gives, so it changes the same values silently, and it leaves out an Item
whose key it cannot write at all. Units are older and native-only, and their keys are
strings in every example and test in this repository, so they keep that behaviour.
`units.md` says where the two kinds of key differ.

`draw` now receives the resolved name as a third argument, `draw(svg, d3, { name })`, on the
desktop and in a page. Ids derived from the key — a clip-path's, say — must differ per
figure in the page's single DOM, and the name is the one string that already does.

The hash is an internal helper, `__tauler_sha256`, registered in every QuickJS runtime
outside the `Effects::Allowed` gate, since the render runtime is the one that names
Pictures. The page gets it as a synchronous wasm export installed before the shared globals
run, because `crypto.subtle` is asynchronous and the name is needed mid-render.

The cost is one `JSON.stringify` per Tick in both the render runtime and the Draw runtime —
measured at about 0.8 ms per figure for 500 rows × 3 fields; the hash itself adds close to
nothing. A hand-made string key stays the cheaper choice for large data.

Rejected: a public `hash()` global, which would be a new public global the page must also
provide, and a `memo()` helper, which would keep state across Ticks against ADR
[0007](0007-every-tick-re-renders-everything.md).
