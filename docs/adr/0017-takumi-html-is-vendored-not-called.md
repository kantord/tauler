# takumi-html is vendored, not called

tauler converts its own tree to takumi nodes with its own walk. The `takumi-html` crate —
which exists to do exactly this conversion — is not a dependency. Its Chromium preset
table is copied into tauler behind an attribution fence, and nothing else of it is used.

This is the decision most likely to be "fixed" by someone who finds the crate on crates.io
and assumes it was overlooked.

## Why it cannot be called

`takumi-html`'s entire public surface is `from_html(&str, options)`. The input is text. It
parses that text with html5ever and walks the resulting DOM. There is no entry point that
takes a tree.

tauler's input is a tree that was never text, and it carries things no HTML string can
hold: intent arrays for [0012](0012-controls-are-never-self-holding.md) controls, style
objects with values computed per tick, and `<Module>` render-prop results that were
already resolved during JSX evaluation. Using the crate means serializing that live tree
to markup every time content changes, parsing it back, and re-attaching everything markup
could not carry through a side table keyed by `id`.

## Why that is not simpler

The tempting reading is "use their walk instead of writing one." That is not the trade on
offer, because our input is a tree in both designs. Calling `from_html` does not remove
our walk — it puts a serializer in front of it, and adds a re-parse and a re-attachment
pass behind it. What it saves is one table lookup per node.

## What is given up

**html5ever's correctness on real-world markup.** We have no real-world markup. Nothing
hostile, malformed or hand-written ever reaches this code — the tree comes from a JSX
evaluator that already rejected anything ill-formed.

**`<style>` blocks and CSS selectors.** takumi supports stylesheets; reaching them from a
tree rather than a document is a separate piece of work. `class` is recorded on every node
against the day it happens — see [0016](0016-layout-nodes-are-html-elements.md).

**Inline `<svg>`.** `takumi-html` handles it by re-serializing the DOM subtree back to
markup for resvg. We have JSX, not a DOM, so supporting it means writing a JSX-to-XML
serializer for one tag. An SVG data URI in `<img src>` covers the same ground.

## What is gained

html5ever's *normalization* stops applying. A JSX tree can hold shapes HTML forbids — a
`<p>` containing a block element, table content outside a table — and the browser parser
would silently restructure them on the way through. Written as authored, rendered as
written.

## The vendored table

Both projects are `MIT OR Apache-2.0`, so copying is clean provided the notice travels
with it. The table is fenced in comments carrying the upstream copyright, the license, and
a **commit-pinned** permalink — a branch link rots and stops proving what was copied.

The values are Chromium's user-agent stylesheet restated in Rust, so what is actually
borrowed is closer to a constant than to a program. Attribution costs nothing and settles
the question.

The fence carries a TODO: upstreaming a public `preset_for_tag` accessor would remove the
copy entirely, and with it the one real cost of this decision — the table drifts silently
when takumi corrects theirs, and only a manual diff against the permalink will say so.

Two smaller constants came across with it. `DROPPED_TAGS` is `takumi-html`'s tag list and
is covered by the same fence. The nesting cap is *not*: 512 was inherited, found to be
twelve times higher than this walk's real stack budget, and replaced with a measured
value of our own — see `layout::html::MAX_DEPTH`.
