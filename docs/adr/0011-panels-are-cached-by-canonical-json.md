# Panels are cached by canonical JSON, and rasterized in software

Each panel owns a `RenderCache` keyed by the canonical JSON of its own subtree
(`json_canon`). A tick re-evaluates the whole layout, but only panels whose serialized
subtree changed are rasterized again. Rasterization itself is software — takumi plus
tiny-skia. There is no GPU path.

## Why cache at the panel, not the node

Rasterization is the entire cost: 40–90ms for a full-height panel at 365×2160, against
under a millisecond for everything upstream of it — JSX evaluation, layout parsing, the
cache-key check. So the only caching worth having is the kind that skips a rasterization,
and the panel is the smallest unit that maps to one.

Keying on canonical JSON rather than on identity is what makes
[0007](0007-every-tick-re-renders-everything.md) affordable: nothing needs to survive
between ticks for the cache to hit. Two structurally identical trees produce the same key
whether or not they came from the same component, so a full re-render costs nothing when
nothing changed.

## Why software rendering

A status bar redraws when a subprocess prints a line. Against that, a GPU context is a
dependency, a failure mode on headless and virtualized systems, and a driver matrix — in
exchange for making a 40ms operation faster than it needs to be. Software rasterization
also means the render pipeline is a pure function from tree to pixels, which is why it is
the one part of the codebase a second display backend did not have to touch
(see [0010](0010-display-server-code-lives-behind-a-seam.md)).

## Consequences

A panel with a live clock rasterizes every second, because its subtree genuinely changed —
the cache helps panels that are static, not panels that tick. Panel size is therefore the
dominant performance lever available to a layout author: a full-height sidebar is the
expensive surface, and splitting rarely-changing content into its own panel is what makes
it cheap.

Backdrops complicate the key. The cache is keyed by `(generation, rect)`, not by subtree
alone: the wallpaper generation stops a stale hit after the backdrop moves, and the rect
keeps two same-size, same-content panels from colliding on one entry and being served each
other's slice of wallpaper.
