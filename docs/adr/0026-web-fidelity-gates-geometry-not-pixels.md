# Web fidelity gates geometry, not pixels

The web renderer is checked against the committed component screenshots by two different
instruments. Every element's box, measured with `getBoundingClientRect()`, must agree with
takumi's to within a logical pixel — that gate is strict and it fails the build. The second
check asks only whether the component rendered at all; the measured difference is written
out for a person to look at.

## Why not simply gate the image

[0005](0005-desktop-screenshots-are-reviewed-not-gated.md) already ruled on this, and its
argument applies here with more force, not less:

> Gating on a hash would therefore buy either a suite that goes red for reasons unrelated
> to the change under review, or a tolerance threshold — a number nobody can justify and
> everybody eventually raises.

An exact hash is impossible by construction. takumi rasterizes text with parley and skrifa
into tiny-skia; Chrome uses Skia's own hinting and antialiasing. Individual glyph pixels
will differ no matter how carefully the fonts are pinned, so the only available image gate
*is* the tolerance threshold 0005 rejects.

## Why the geometry gate is a different instrument

Box positions have no such excuse. Two engines given the same computed styles either agree
about where a box goes or one of them is wrong, and the answer is a number rather than a
judgement. So the strict claim moves there, and the image comparison is relieved of
carrying it.

This is 0005's own conclusion — "what is gated is the structure, not the pixels" — reached
by the same reasoning at a different layer. There the structure was i3's reported gaps and
the X server's window geometry; here it is the box tree.

The machinery is nearly free. `hit_test::painted_boxes` walks takumi's scene and hands back
a box per render path; `layout::dom` already emits that same path as `data-tauler-path` for
click delegation ([0018](0018-clicks-bind-by-render-path.md)); pairing them is a lookup.
When it fails it names the node and the axis, which no image diff can do.

The *measured* tree is deliberately not used, for 0018's reason: takumi replaces a node's
measured children with flat inline boxes wherever it holds inline content, so the measured
tree stops describing the layout tree's shape as soon as an element contains text. The
consequence is that inline elements have no box to compare — a `<span>` inside a paragraph
is checked only through the block that contains it.

## Why there is a second, coarse check at all

0005 could rely on structural assertions alone because a blank desktop screenshot still
fails them. This renderer has a failure mode 0005 did not: emit an empty tree, and every
geometry comparison passes trivially — nothing to compare — while the page renders
nothing at all.

So the second check answers *"did it render"*, not *"is it close enough"*. That distinction
is what makes it a number that can be justified and, more importantly, one nobody has an
incentive to raise.

## What the second check measures, and what it does not

The obvious instrument is the share of pixels that differ between the two images. **It does
not work, and the measurement says so plainly.** These components are mostly dark background
with sparse text, so against the committed screenshots:

| | correct render | blank render |
|---|---|---|
| `datatable` | 36.9% of pixels differ | **10.0%** |
| `card` | 31.8% | **6.1%** |
| `table` | 26.8% | **7.2%** |

A blank rectangle scores *better* than a correct render, because a correct render disagrees
at the edge of every glyph and a blank one agrees everywhere there is no glyph. The metric
ranks the failure above the success. A threshold on it is not merely hard to tune — there is
no value that separates the two.

What separates them is **ink**: the share of pixels that are not the render's own background.
Across the seven components the browser draws between **1.008×** and **5.612×** takumi's ink
— the upper end is Chrome's antialiasing spreading a glyph over more pixels than tiny-skia's,
not a difference in what was drawn — and a render that produced nothing draws **0.000×**. The
bar is a quarter, with four times the margin to the nearest real render on one side and to
zero on the other.

There is no upper bound, deliberately. Judging *how* something rendered is the geometry
gate's job; judging how it looks is a person's.

Both numbers come from measurement rather than in advance. A threshold chosen before seeing
real output is a threshold chosen to make today's output pass.

## Consequences

**A visible regression that moves no box and clears the liveness bar is caught only if
someone looks at the published diff.** That is the same trade 0005 accepted, made
deliberately again.

**The geometry gate was expected to fail on first run, on text advance width. It did not.**
parley and Blink agree about every box takumi paints, in all seven components, to within a
pixel — including the blocks that contain text, though not inline elements, which takumi
gives no box of their own. A stronger result than this decision expected, and the reason to
keep the tolerance where it is now that it is known to be achievable.

What the pixel comparison does show, once the capture is honest, is that the residual is
entirely glyph rasterization. On the pinned browser, mean per-channel difference against the
committed screenshots:

| `progress` | `badge` | `slider` | `knob` | `table` | `card` | `datatable` |
|---|---|---|---|---|---|---|
| 1.26 | 6.15 | 11.78 | 20.52 | 57.09 | 71.42 | 79.95 |

It orders by text density, and `progress` — which draws two bars and no glyphs — is within
one part in 255. The suite writes this table to `docs/.tauler/web-shots/difference.csv` on
every run, so the numbers are reproducible rather than quoted.

The **pinned** browser is the qualifier that matters. The same measurement against a
development machine's own Chrome gives `card` 6.8 rather than 71.4 — an order of magnitude
apart, with the ink ratios identical to three decimals, so the two browsers draw the same
shapes and fill the glyphs differently. That is the whole argument for the pinned image
restated as a measurement: a number from an unpinned browser is not a number about tauler.

**Making the capture honest took three fixes, all found by measurement**, and they are
recorded in `tauler-web-e2e/src/browser.rs` because each one produced a difference that
reads exactly like a renderer disagreement and is nothing of the kind: a mount at a
fractional page offset (every row sliced mid-pixel, a uniform ~12/255 everywhere), a pinned
mount painted underneath the site's header (`z-index` cannot escape an ancestor's stacking
context), and `captureBeyondViewport` re-laying-out the page so the clip no longer matched
what was measured.

**The browser runs in a container with a pinned Chrome**, for the reason
[0004](0004-e2e-builds-tauler-inside-the-image.md) gives about glibc: a threshold measured
against an unpinned Chrome means nothing the next time Chrome updates. The comparisons are
`#[ignore]` by default, per [0006](0006-e2e-scenarios-are-ignored-by-default.md).

**There is one baseline, not two.** The committed `docs/src/assets/*.png` gain a second
producer rather than a counterpart, so there is no second set of golden images to keep in
step. takumi is host-independent enough for that to hold; `<Icon>` is not, because
`append_symbol_fallback` resolves its font through fontconfig, and it is excluded from the
web preview for that reason.
