# The web renderer emits DOM, and the browser is the rasterizer

On the web a layout tree becomes an HTML string, written into a Mount node. The browser
resolves the Tailwind, supplies the presets from its own user-agent stylesheet, lays the
boxes out and paints them. takumi is not in the bundle.

This is the decision most likely to be "fixed" by someone who notices that takumi
compiles to wasm perfectly well.

## Why not takumi in wasm

It does compile — `cargo check --target wasm32-unknown-unknown` on `takumi 2.7` is clean,
parley and skrifa and tiny-skia and all. So the reason is not that it cannot be done.

The reason is that the thing this renderer exists to prove would stop being provable. The
claim we want to check is *"the tree we build lays out the same way in a browser as it does
in takumi."* Run takumi in wasm and paint its buffer to a canvas, and the two images agree
because they came out of the same rasterizer — a test that cannot fail is not a test. See
[0028](0028-web-fidelity-gates-geometry-not-pixels.md) for what is checked instead.

It also leaves [0016](0016-layout-nodes-are-html-elements.md) unredeemed. That decision
paid for naming every layout node after the HTML element it is; the payoff is being able
to hand the tree to something that already knows what a `<p>` is.

## Why the browser resolves the styles

The alternative is resolving Tailwind through takumi in wasm and emitting computed
`style="…"` on every node. That needs a takumi `Style` → CSS writer: roughly a hundred
properties, mechanical, and a third representation of styling to keep honest alongside
takumi's and the browser's. Nothing in the ecosystem provides it — takumi resolves
Tailwind into its own `Style`, not into CSS text.

Letting the browser do it costs no tauler code at all, because `theme/resolver.rs` already
rewrites theme tokens into ordinary Tailwind (`bg-background` → `bg-[#hex]`) before anything
downstream sees them. What reaches the DOM is plain Tailwind with arbitrary values, which
real Tailwind compiles.

The presets come free from the same place. ADR [0017](0017-takumi-html-is-vendored-not-called.md)
records that tauler's preset table is a *copy* of Chromium's user-agent stylesheet, and
that it "drifts silently when takumi corrects theirs, and only a manual diff against the
permalink will say so." Under this decision, CI diffs it against actual Chromium on every
run. A documented TODO becomes a gate at no cost.

## Why `innerHTML` and not a diff

Rebuilding the whole subtree each tick is not a shortcut taken here; it is
[0007](0007-every-tick-re-renders-everything.md) applied unchanged. Handlers survive it
because [0018](0018-clicks-bind-by-render-path.md) already binds them by render path: the
walk emits `data-tauler-path`, one delegated listener on the Mount node reproduces desktop
hit-testing, and nothing is attached per node to be destroyed.

## Why the walk is Rust and not JavaScript

The DOM walk is small enough to be tempting to write in the page. It is not written there
because `DROPPED_TAGS` — the tags whose subtree never reaches the tree at all — contains
`script`. A second copy of that list, in another language, is a security-relevant
duplicate. `MAX_DEPTH`, the falsy-child rules and the text-node rule travel with it for the
same reason.

## What is given up

**Tailwind's subset problem now runs both ways.** takumi implements part of Tailwind; the
browser implements all of it. Where they disagree the web output is *more* correct than the
desktop's, silently. This is a real hazard and the reason [0028](0028-web-fidelity-gates-geometry-not-pixels.md)
exists at all.

**Classes computed from live data have no CSS.** The stylesheet is generated at build time
by harvesting the resolved class strings out of every example, so `bg-[${color}]` computed
at runtime resolves to nothing. Fine for a documentation site, fatal for a live deployment
— that is the day Tailwind's in-browser JIT gets added.

**`innerHTML` destroys focus, selection and CSS transitions every tick.** Also fine for a
documentation site, also not fine for a real one. Replacing it with a keyed diff is a change
to one function; the tree crossing the boundary does not change.

## Consequences

The wasm bundle contains the Rust UI components, the theme resolver, the presets and the
walk — and neither takumi nor QuickJS. Keeping it that way is enforced by a crate boundary
rather than by discipline: the wasm-clean subset lives in `tauler-core`, which cannot
depend on x11rb or rquickjs because it does not depend on `tauler`. See
[0010](0010-display-server-code-lives-behind-a-seam.md).

A Dom surface is the third Surface kind and the only one tauler draws nothing for.
