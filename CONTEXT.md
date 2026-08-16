# tauler

A status bar and widget system for Linux desktops. Layouts are declared in JSX, data
arrives from subprocess streams, and everything is rasterized in software. The scope is
desktop shell surfaces — bars, docks, notification areas — not general-purpose GUI.

## Language

### Surfaces

**Surface**:
A rectangle tauler rasterizes into. Every surface has exactly one kind — Panel or
Wallpaper — which decides where the finished pixels go, and nothing else.
_Avoid_: widget, window (a Wallpaper is neither)

**Panel**:
A surface that owns a desktop window of its own.
_Avoid_: bar, dock, widget, bar window

**Wallpaper**:
A surface painted into the desktop background of one output. It has no window, takes no
clicks, and always covers its output exactly.
_Avoid_: background, backdrop, root image, desktop

**Output**:
One connected monitor, named as RandR names it (`DP-2`).
_Avoid_: screen, display, monitor

**Anchor**:
The screen edge a surface is pinned to. An anchor *places* a surface; it never reserves
space for it.
_Avoid_: dock edge, side, alignment

**Reservation**:
Space the window manager keeps clear of tiled windows. Always stated as a decision, never
derived from a surface's geometry.
_Avoid_: strut, exclusive zone (both are display-server mechanisms, not this concept)

**Gaps**:
The four per-edge distances that express a reservation to i3. The unit is the logical
pixel — see **Logical pixel**.
_Avoid_: margins, padding, insets

**tauler:root-bg**:
The slice of Wallpaper sitting behind one Panel, bound as an image for the duration of a
single render. It is what makes a Panel look transparent; nothing is actually translucent.
The scheme marks it as a resource tauler binds, never a file to read.
_Avoid_: transparency, blur, backdrop

### Rendering

**Render target**:
Something with pixels of its own. Today every target is a Surface; a `<BufferBoundary>`
will make one out of a subtree. What makes it a target is having its own slot in the
scheduler and its own entry in the cache.
_Avoid_: layer, texture, canvas

**Repaint**:
Drawing a Render target that already exists again, because its content, its scale or the
Wallpaper under it changed. Nobody waits for one.
_Avoid_: redraw, refresh; frame (those are the pixels, not the act — see **Frame**)

**Frame**:
The finished pixels of one Render target, at one physical size. What a Repaint produces
and what the cache keeps.
_Avoid_: buffer, bitmap, image

**Render request**:
What to draw, at what physical size, against which slice of Wallpaper. It carries
everything the drawing needs, so the Render worker consults nothing else.
_Avoid_: draw call, render task

**Render job**:
A Render request plus what to do with the pixels: paint them (a Repaint) or hand them back
to a caller that is waiting. Both are drawn by the same thread from the same cache.
_Avoid_: message, command (a Surface command is the other channel)

**Render worker**:
The one thread that draws. Nothing else rasterizes, which is what lets the frame cache be
its private state rather than a global behind a lock.
_Avoid_: render thread, rasterizer (that is takumi), render queue

**Supersede**:
What a newer Render request does to the unpainted one in a target's slot. A render already
under way is never superseded — it finishes, and the newer request is drawn after it. See
ADR 0023.
_Avoid_: cancel, abort, drop (all three claim work stops, and none of it does); debounce
and throttle (they name a rate, not a replacement — see **Repaint floor**)

**Repaint floor**:
The shortest gap allowed between two Repaints of one target. It delays a Render request,
never discards it: the request waits in its slot and is drawn when the floor lifts.
_Avoid_: throttle, debounce, rate limit, frame cap

### The layout file

**Layout file**:
The single `.jsx` file that declares everything a bar *is*. A sibling `config.yaml`
carries theme mode and font choice — what to render with, never what to render. Anything
describing a surface, its contents or its data belongs in the layout file.
_Avoid_: config, config file, theme file (all three name the `.yaml`, not this)

**Tick**:
One full re-evaluation of the layout file, triggered by any stream value changing. Every
tick rebuilds the whole tree from scratch; nothing survives between ticks except
`globals`.
_Avoid_: frame, render pass, update, re-render

**Shell node**:
A node that describes structure rather than content: `root`, `panel`, `wallpaper`. Shell
nodes never reach the rasterizer, and they are the only lowercase names in a layout file
that are not HTML elements.
_Avoid_: top-level node, container node

**Layout node**:
A node that describes content and is rasterized. Layout nodes are named after the HTML
element they are — `div`, `span`, `img` — and behave as that element does.
_Avoid_: leaf node, visual node, tag (a tag is the name; the node is the thing)

**Text node**:
A Layout node holding a run of text. It has no element of its own: writing a bare value
in the tree is what makes one, and the only thing that does.
_Avoid_: label, string node, text element

**Preset**:
The style an element gets from its tag name alone — why a paragraph has margins and a
heading is large. Presets sit under everything else, so anything written on the node
wins.
_Avoid_: default style, base style
_Elsewhere_: user-agent stylesheet (browsers)

**Edge layout**:
Deriving both a set of Panels and the matching reservation from one ordered list of edge
declarations, so the two cannot disagree. This is what `<I3Layout>` does.
_Avoid_: docking, packing, auto-gaps

**Panel declaration**:
A `<Panel>` inside an `<I3Layout>` — an instruction to eat `size` off one edge. It is not
a Panel; it *produces* one. Note the casing: lowercase `<panel>` is the surface, capital
`<Panel>` is the declaration.
_Avoid_: panel (unqualified — it is the collision this term exists to prevent)

### Scenarios

**Scenario**:
One fixture plus the reservation it is supposed to produce, run on a desktop of its own.
The expected numbers are part of the scenario, written by hand — a scenario that derives
them is not checking anything.
_Avoid_: test case, e2e test

**Fixture**:
Everything a scenario installs onto its desktop before tauler starts. How much that is
depends on the kind of scenario: a contract scenario's fixture is one layout file, a
rice's is a home directory.
_Avoid_: test data, config directory

**Contract scenario**:
A scenario that exists only to check the reservation contract. Its fixture is as small as
a working desktop can be, and that minimality is itself the claim.
_Avoid_: basic scenario, simple scenario, smoke test

**Rice**:
A scenario whose point is how it looks. Its fixture carries the configuration of every
program on the desktop rather than tauler's alone, so what it demonstrates is a desktop
and not a bar. Held to the same reservation contract as any other scenario.
_Avoid_: demo, mockup, theme, example

### Components

**Data component**:
A component that renders no pixels and instead hands data to its children through a render
prop. Defined by shape, not by where the data comes from — a Module wrapper and a pure
transform are both Data components.
_Avoid_: provider, source, container
_Elsewhere_: headless component, render-prop component (React)

**Display component**:
A component that renders data as pixels. A wrapper that only decorates what it is given,
like `<Card>`, is a Display component too — wrapping is not a kind of its own.
_Avoid_: view, presentational component, dumb component
_Elsewhere_: mark (Vega-Lite), geom (ggplot)

**Control component**:
A Display component that also emits intents. It never holds a value: it renders the value
it is given and remembers nothing.
_Avoid_: input, interactive component, widget
_Elsewhere_: controlled component (React). Observable's `viewof` is the opposite — see
ADR 0012.

**Handler**:
What an element does when the pointer reaches it: either an array of intents, or a function
from the pointer to one. `on_click` and `on_drag` both take either — see ADR 0021.
_Avoid_: callback, listener, action, binding
_Elsewhere_: event handler (DOM), except that ours may be plain data

**Pointer capture**:
An element with `on_drag` taking every motion event until the button is released, whatever
the pointer is over. What makes a drag address the control you grabbed rather than whatever
you slid onto — see ADR 0020.
_Avoid_: grab, drag mode, tracking, focus
_Elsewhere_: `setPointerCapture` (DOM), implicit passive grab (X11)

**Press point**:
Where a drag's button went down, reported beside every position that drag produces. What a
control measures against when it reads how far the pointer has come rather than where the
pointer is — see ADR 0022.
_Avoid_: grab point, anchor, origin, drag start
_Elsewhere_: nothing in the DOM — a web page keeps it in a `pointerdown` handler

**Component kind**:
Which of Data, Display or Control a component is. Exactly one applies, resolved by
precedence: Data, then Control, then Display. Components that produce Shell nodes, like
`<I3Layout>`, have no kind.
_Avoid_: type, class, category

**Accessor**:
A prop naming which part of the data a component should use. A field name is shorthand for
a function — `y="usage"` means `y={r => r.usage}` — and the same form points a Repeater at
what to split on.
_Avoid_: key, selector, encoding, mapping
_Elsewhere_: accessor (d3), aesthetic mapping (ggplot), encoding channel (Vega-Lite),
field (Grafana)

### Data

**Module**:
A subprocess tauler both reads JSON from and sends intents to. A Module owns a vocabulary;
tauler does not interpret it.
_Avoid_: plugin, provider, service, backend

**Stream**:
A subprocess whose stdout tauler reads, one value per line. Read-only — a stream that is
also written to is a Module.
_Avoid_: source, feed, watcher

**Accumulator**:
A subprocess that buffers the stream piped into it and re-emits a window of the last N
lines on each line. It is what gives a layout file history, since tauler itself keeps only
the latest line of a stream.
_Avoid_: buffer, history, retention, ring buffer
_Elsewhere_: rolling window (pandas), range vector (Prometheus)

**Subprocess identity**:
The `(bin, script)` pair that decides whether a running subprocess is reused or replaced.
Two declarations with the same identity are the same process.
_Avoid_: process key, instance id

**Intent**:
A plain JSON object naming a channel and the message to deliver there. Produced by
calling a property on an events proxy; it is data, never a callback.
_Avoid_: action, event handler, callback, command

**Channel**:
The destination of an intent — the bin it is delivered to. A Module only ever sees
messages addressed to its own channel, and never learns what caused them.
_Avoid_: topic, target, recipient

### Units

**Logical pixel**:
The unit every length in a layout file is written in, and the unit i3 states gaps in.
Unqualified "pixel" in this project means this one.
_Avoid_: dip, point, scaled pixel

**Physical pixel**:
A pixel on the actual panel. Appears only inside the display backend; a value in this unit
should never cross back out into layout or gaps.
_Avoid_: device pixel, real pixel, hardware pixel
