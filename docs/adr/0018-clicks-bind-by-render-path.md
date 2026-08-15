# Clicks bind by render path, not by walking two trees in step

Hit-testing resolves a click to a node through takumi's scene walk — the paint list gives
each painted node a path back into the render tree, and that path is what identifies the
node that was hit. It does not walk the measured tree and the layout tree side by side,
pairing children by index.

## Why the obvious approach is wrong

Index-pairing is the natural thing to write, and it holds right up until a node contains
inline content. When takumi builds an inline layout for a node, it *replaces* that node's
measured children with a flat list of inline boxes — it does not add them alongside the
real subtree. The children that come back are boxes, in box order, with no children of
their own.

So the moment a node contains inline content, the two trees stop describing the same
shape: source nodes get paired against inline boxes, and anything nested below becomes
unreachable. Under [0016](0016-layout-nodes-are-html-elements.md) that is most of a bar,
because bare strings are inline content and nearly every element holds one.

## Why it appeared to work before

With the old vocabulary, text was a leaf node that could not carry `on_click`. A desync
therefore produced a miss rather than a wrong answer, the walk fell back to the nearest
ancestor that did carry a handler, and that was usually the right node anyway. The bug was
real and load-bearing code depended on it not mattering.

## Consequences

**`on_click` works on block-level elements only.** Non-atomic inline elements — `<span>`,
`<em>` — never get a layout node of their own; their geometry lives in glyph runs, and
binding a click to one means walking those runs and mirroring takumi's private inline
item collection. That mirror desynchronizes silently if takumi adds a node kind, which is
too much fragility to carry for a capability nothing has asked for.

**A handler that can never fire says so.** An `on_click` whose node has no clickable box
is warned about once — named as its author wrote it, `<span id="dismiss" class="…">`,
because a path of child indices is not something anyone can map back to a line of JSX.

"No clickable box" is the test, not "absent from the paint list". An inline element *does*
get a paint entry, with a zero-area box that no point can fall inside; treating presence
as reachability would silence the warning in precisely the case it exists for.

The warning fires on the first click that reaches the surface, because that is when
layout runs. A bar nobody clicks reports nothing — accepted, since a handler nobody
clicks is also a handler nobody misses.

**Lifting the limit later is one field, not a redesign.** takumi already threads a link
target from an ancestor down to each glyph run for its PDF output; a source identifier
threaded the same way would make inline runs bindable. That is an upstream change to ask
for when something needs it.
