# a11y rides the same interaction pipeline as clicks

A screen reader (an **AT**) activating a node must reach the same
`App::resolve` / `App::send` flow a pointer press uses — not a second
dispatch path. An AT "activate" carries no pointer position: it names a
node (by its `data-tauler-path`), so the tick thread re-derives that node's
`on_click` the same walk `hit_test` does on a click, resolves it (so a
`$handler` function reaches the QuickJS runtime — the one place a function
handler can be called), and sends the resulting intents through the existing outbox.

## Why not dispatch AT intents directly

`App::resolve` (`src/app.rs:923`) is the only place a `$handler` function
reference becomes intents: it calls into the live QuickJS evaluator on the tick
thread. An AT action arrives on accesskit's own thread, so the intents
cannot be resolved there. Routing the raw `(panel_id, path)` to the tick
thread keeps every interaction — clicks, drags, and a11y — inside one pipeline,
and gives the long-term "first-class control tools" direction a single seam to
extend.

## The fabricated press point

A click's `pointer` is only meaningful to function handlers that read it. AT
activation has no position, so the pointer is fabricated as a press at the
element's box origin: `x`/`y`/`press_x`/`press_y` are `0`, width and height
are real. Documented so a function handler can rely on it being well-defined.

## Consequences

**There is no second intent path for AT to diverge from later.** A future
first-class a11y control tool lands on the same resolve/send seam, not a new
one.

**A plain `<div on_click>` stays generic** — an AT can only activate it if
the author wrote `role="button"` explicitly; no implicit interactive roles.

**AT actions are only as good as what the layout declared.** An activate on a
node with no `on_click` does nothing, exactly as a click on one would.