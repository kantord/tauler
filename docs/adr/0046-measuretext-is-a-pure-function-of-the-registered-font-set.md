# measureText is a pure function of the registered font set

A layout file can call `measureText(text, { family, size, weight, style })` and get back the
`width` tauler will draw that text at, and the `ascent` and `descent` of the family's first
face, in logical pixels rounded to 1/64. It exists in every runtime that evaluates a layout
file — render, reconciler, Draw — and in the page, outside the `Effects::Allowed` gate.

## Why a layout may measure at all

ADR [0007](0007-every-tick-re-renders-everything.md) makes a Tick a pure function of the
current data. Measuring does not break that, because the answer depends on nothing but its
arguments and the fonts tauler registered: not on time, call order, or anything a previous
Tick left behind. The font set is the same kind of input as `ctx`: fixed for the life of a
layout load unless the fonts are reloaded, and the same for every runtime in the process.

Without it, a chart that sizes its margins or thins its ticks by the length of its labels has
to guess widths from per-font tables, which are a few percent off for the face tauler
actually draws, and far more for some labels. linkedom in the Draw runtime has no
layout engine, so `getBBox` and its relatives measure nothing there.

## Why it is not behind the effects gate

The gate (ADR [0034](0034-hooks-run-in-a-second-runtime.md)) keeps out what touches the
world. Measuring touches nothing; it lays a line of text out with the fonts already loaded.
And it has to be everywhere: a chart lays itself out in the render runtime when its key
depends on that layout, and in the Draw runtime otherwise. If only one of them could measure,
the same chart would lay out differently depending on where it was built. So it is registered
the way `__tauler_sha256` is (ADR [0044](0044-d3figures-draw-in-a-third-runtime-and-arrive-as-pictures.md)).

## How the answer is made to match the drawing

There is no second text engine. A measurement is a one-line layout run through the same
parser and the same takumi `Fonts` a frame is drawn with, so the family list resolves exactly
as a Text node's does. A D3Figure's Picture is rasterized by the same renderer, whose SVG
text layout shapes with the same fonts. A family no font answers is not an error: the measurement is the
fallback face drawing would use, because an unattended bar must not go blank over a host's
font configuration. Programmer mistakes — an empty family list, a size that is not a finite
positive number, a text that is not a string — throw a `TypeError`.

An uncached measurement costs about 100 µs, and a chart asks for dozens every Tick, so answers
are memoised. The memo lives beside the fonts and is replaced with them on a font reload, so
it never answers for fonts that are gone. It is process-wide, not per runtime, so the
runtimes cannot disagree, and it is not state a Tick can observe: a hit and a miss return the
same value. A cached call costs about 5 µs from JavaScript.

## Consequences

**A layout that measures depends on the host's fonts.** Two machines with different fonts get
different widths, and a figure key derived from measured widths differs between them. That is
correct — they draw different pictures — and `fonts.files_only` is the switch for renders
that must be identical everywhere.

**The page measures with the browser's font.** It uses a canvas, which is ADR
[0028](0028-web-fidelity-gates-geometry-not-pixels.md)'s position restated: the page and the
desktop agree when they resolve the same face, and nothing makes them resolve the same face.
The checks and the 1/64 rounding are shared source, so both refuse and round alike.

**A font reload between the render and the Draw runtime's evaluation of the same Tick** can
give the two different widths for one label. A key derived from them then differs, and the
Draw runtime reports the key as unknown until the next round asks again. It heals itself.

Rejected: a per-font advance table handed to JavaScript, which has no kerning, no fallback per
character and must be marshalled into three runtimes; and a canvas-like object with a `font`
setter, which is state across calls for no gain over one function.
