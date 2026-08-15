# Layout nodes are HTML elements, presets and all

A layout node is named after the HTML element it is: `div`, `span`, `p`, `img`. The
`container` / `text` / `image` vocabulary is removed rather than kept as aliases, the full
Chromium user-agent preset table applies, `class` carries Tailwind utilities, and a bare
string anywhere in the tree becomes a text node.

## Why HTML names

takumi's node model is already CSS's. It has flex, grid, block and inline layout,
stacking contexts, blend modes, filters and `calc()`. The old vocabulary was a thin
rename of that model into three words — which meant nobody had to learn it, and nobody
could transfer anything they already knew into it either. Every question about how a box
behaves had to be answered by us.

Naming the nodes what they actually are moves those questions somewhere they are already
answered. `<p>` inside `<div>` behaves the way a reader expects because it *is* that, not
because we reimplemented something similar.

## Why the whole preset table

Half-HTML is worse than either half. A `<p>` without its margins, or an `<h1>` at body
size, is a name that promises something the renderer does not deliver — and that promise
gets broken once per person, forever, in the form of the same question.

Borrowing the name obliges the behaviour. The table is Chromium's, so the obligation is
met by a table rather than by judgement.

## Why replacement, not aliases

Keeping `container` alongside `div` means every documentation page, every example and
every built-in component picks a side, and a reader has to learn both vocabularies to read
someone else's bar. The migration is mechanical and the audience is small; a deprecation
window buys nothing against that.

## Why `class` and not `tw`

In HTML, `class` does not style anything — it is a hook that a stylesheet interprets.
Tailwind-in-`class` is that convention, and it is the one people arrive with.

Nothing is given up by using it: takumi records `className` even when it reads Tailwind
from the same attribute, so `class` still names the node for any future stylesheet.
Utilities that neither tauler's theme layer nor takumi recognizes pass through both
layers untouched, which is what makes a single attribute safe to overload.

## Where the principle stops

The rule is **HTML names only where the behaviour matches HTML**. It is what forces the
exceptions as much as the borrowings:

- **Event handlers stay `on_click`, not `onClick`.** An intent is data, not a callback —
  see [0012](0012-controls-are-never-self-holding.md). A React-shaped name on something
  that is not a React handler is exactly the lie this decision exists to avoid.
- **Table components render `div`s.** takumi has no `display: table`, so `<table>` would
  name a layout algorithm that does not exist.
- **The backdrop is `src="tauler:root-bg"`.** It is a resource tauler binds for one
  render, not a file to read. A scheme says so, and makes it impossible for a file of that
  name to shadow it.
- **Shell nodes keep their own names.** HTML has no `panel` or `wallpaper` to borrow, so
  nothing is being misrepresented by `root`, `panel` and `wallpaper` sitting in the same
  namespace as `div`.

## Consequences

**Layout behaviour changes, not just spelling.** A `container` defaulted to `inline`; a
`div` is `block`. Every existing layout has to be re-read rather than find-and-replaced,
and the block and inline paths through takumi are considerably less exercised than the
flex path this project has leaned on so far.

**There is no text element.** `<text>` had a required `text` field; a text node now comes
from writing a string, and only from that. It cannot be styled directly — style the
element around it, as in HTML.

`on_click` works on block-level elements only — see
[0018](0018-clicks-bind-by-render-path.md).

Inline `<svg>` is not supported; an SVG data URI in an `<img src>` is. See
[0017](0017-takumi-html-is-vendored-not-called.md) for why.
