# tauler

A status bar and widget system for Linux desktops. Layouts are declared in JSX, data
arrives from subprocess streams, and everything is rasterized in software. The scope is
desktop shell surfaces — bars, docks, notification areas — not general-purpose GUI.

## Language

### Surfaces

**Surface**:
A rectangle tauler produces output for. Every surface has exactly one kind — Panel,
Wallpaper or Dom — which decides where the finished output goes, and nothing else.
_Avoid_: widget, window (a Wallpaper is neither)

**Panel**:
A surface that owns a desktop window of its own.
_Avoid_: bar, dock, widget, bar window

**Wallpaper**:
A surface painted into the desktop background of one output. It has no window, takes no
clicks, and always covers its output exactly.
_Avoid_: background, backdrop, root image, desktop

**Dom**:
A surface whose output is markup rather than pixels. The browser is what rasterizes it,
which is why it is the one surface kind tauler draws nothing for.
_Avoid_: mount, web surface, canvas, root

**Mount node**:
The element in a web page a Dom surface's markup is written into. Everything inside it is
tauler's; everything outside it is the page's.
_Avoid_: host, container, target, portal

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
Something with pixels of its own. Today every target is a Panel or a Wallpaper; a
`<BufferBoundary>` will make one out of a subtree. What makes it a target is having its
own slot in the scheduler and its own entry in the cache — which is why a Dom surface is
not one.
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

### The loop

**Pass**:
One turn of the main loop: reconcile the subprocess set, drain every channel, re-evaluate if
a value changed, then wait. Runs whether or not anything happened, which is what finds a
crashed subprocess. Not to be confused with a **Tick**, which is the re-evaluation a pass
may or may not do.
_Avoid_: tick (that is the re-evaluation), frame, iteration, cycle

**Notifier**:
The one thing the loop waits on. Anything with work for the loop pings it, which is what
ends the wait; a ping carries no information, because the pass it wakes drains everything.
See ADR 0024.
_Avoid_: waker, signal, event bus

**Coalescing floor**:
The shortest a Pass may take. It is what gives a burst a Pass to be batched in — without it
the loop would present each event alone, with nothing to collapse against.
_Avoid_: frame rate, tick rate, poll interval (it bounds how often the loop *may* run, not
how often it does)

### The layout file

**Layout file**:
The single `layout.op.mdx` file that declares everything a bar *is*, plus — in a YAML
frontmatter block at its top — theme mode and font choice: what to render with, never what
to render. Anything describing a surface, its contents or its data belongs in the layout
file's body, not its frontmatter. On the legacy split-file path, the frontmatter's place is
taken by a sibling `config.yaml` instead, and the file itself is `layout.jsx`.
_Avoid_: config, config file, theme file (all three name the frontmatter or `config.yaml`,
not this)

**Extra font**:
A font registered under `fonts.extra` in the layout file's frontmatter (or, on the legacy
path, `config.yaml`), usable directly by name in a layout file (`font-[Name]`) with no
assigned role. Distinct from `primary`, the default, and `emoji`, the fallback used for
emoji glyphs.
_Avoid_: custom font, additional font

**Font role**:
A name in the active theme's `fonts` map, resolved by `font-<role>` in a layout file to
whichever font that theme assigns it — the same mechanism `bg-primary` and `rounded-lg`
use for `colors` and `radius`. Distinct from an **Extra font**, which a layout file
addresses directly by its own family name with no theme involved.
_Avoid_: theme font, font token

**Tick**:
One full re-evaluation of the layout file, triggered by any stream value changing. Every
tick rebuilds the whole tree from scratch; nothing survives between ticks except
`globals`. A **Pass** may happen without one, and usually does.
_Avoid_: frame, render pass, update, re-render; pass (that is the loop turn, not the
re-evaluation)

**Shell node**:
A node that describes structure rather than content: `root`, `panel`, `wallpaper`, `dom`.
Shell nodes never reach the rasterizer, and they are the only lowercase names in a layout
file that are not HTML elements.
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

**Outbox**:
What holds a Module to one intent at a time: the intent in flight, and the newest one
waiting behind it. A Module that has not answered is handed nothing, and what it gets next
is the newest thing produced since — never a backlog. See ADR 0025.
_Avoid_: queue, buffer, backpressure (it is the mechanism, not the effect)

**Channel**:
The destination of an intent — the bin it is delivered to. A Module only ever sees
messages addressed to its own channel, and never learns what caused them.
_Avoid_: topic, target, recipient

**Transport**:
Whatever carries a Stream's values in and a Module's intents out. Subprocesses are the
one tauler ships; a JavaScript module inside a page is another. A layout file names the
bin, never the transport that answers to it.
_Avoid_: glue, backend, adapter, driver, bridge

### Reconcilers

**Unit**:
A kind of reconcilable thing: what identifies one, what value decides whether it has
changed, how to observe the world for it, and what to do when one appears, changes or goes
away. Declared by a `unit()` call, which returns a component — so a Unit is never a node,
only its Items are. See ADR 0033.
A Surface is reconciled by the same machinery. What makes a Unit its own term is that its
hooks are the layout file's own JavaScript, and so may take as long as they like.
_Avoid_: resource (that is a subprocess argument), target (that is a Render target), kind
(that is a Component kind), type
_Elsewhere_: custom resource definition (Kubernetes), provider resource (Terraform)

**Item**:
One instance of a Unit, identified by its key. What the layout tree yields, what an
Observation reports, and what a hook receives. Two Items with the same key are the same
thing, whether one came from the tree and the other from the world.
_Avoid_: instance, entry, record

**Observation**:
The set of Items the world currently holds, as a Unit reports it. The only thing that
establishes what is really there: a hook's return value is not one, and neither is the fact
that a hook was run. See ADR 0035.
_Avoid_: state, snapshot, current state, actual state

**Sweep**:
One turn of a Unit's reconciliation: observe, diff against what the layout declared, run
the hooks the diff calls for. A Sweep belongs to one Unit and runs on the reconciler thread,
so it is never a **Pass** and never a **Tick** — neither of those may wait for it.
_Avoid_: pass, tick, cycle, loop, run

**Refresh interval**:
How often a Unit sweeps. Independent of what the last Sweep did: it is the gap between
Sweeps, not a response to what one found. It is also the Unit's blast radius, since a Unit
that can never converge retries exactly this often. See ADR 0035.
_Avoid_: poll interval, retry delay, backoff (it is not a response to failure)

### Measurement

**Logical pixel**:
The unit every length in a layout file is written in, and the unit i3 states gaps in.
Unqualified "pixel" in this project means this one.
_Avoid_: dip, point, scaled pixel

**Physical pixel**:
A pixel on the actual panel. Appears only inside the display backend; a value in this unit
should never cross back out into layout or gaps.
_Avoid_: device pixel, real pixel, hardware pixel

### Latency

**Hop**:
One delay between a cause and the frame that shows it — a sleep, a queue, a subprocess
round-trip, a rasterization, a compositor's wait for vblank. The unit a latency class is
stated in.
_Avoid_: step, stage, leg

**Stacked**:
Every Hop on one path, added up. A path is classified by its stacked total, which has its
own budget per class — always larger than one Hop's, and always stated rather than derived
from it.
_Avoid_: total, cumulative, end-to-end (say which path)

**Negligible**:
8ms per Hop, 20ms Stacked. Nothing a person can perceive.
_Avoid_: instant, immediate, real-time, zero-cost

**Minimal**:
10ms per Hop, 24ms Stacked. Perceptible only next to something faster.
_Avoid_: fast, quick, snappy

**Slow**:
100ms per Hop, 200ms Stacked. Felt, and acceptable only where the work earns it —
rasterizing a full-height panel is the example.
_Avoid_: sluggish, heavy, expensive

**Non-interactive**:
400ms per Hop, 1200ms Stacked. For what nobody is waiting on: restarting a dead
subprocess, noticing a monitor was unplugged.
_Avoid_: background, async, deferred, best-effort

**Lagged**:
Over 1200ms Stacked. A defect, never a budget. Nothing may be specified as Lagged; it is
only ever a measurement.
_Avoid_: janky, laggy, hang, freeze (a freeze is the watchdog's word for a stalled loop)

**Latency claim**:
Calling a Hop or a path by a class. A claim is only as good as what measured it: the
measurement's uncertainty may be no worse than the per-Hop budget of the class one step
faster. So a Minimal claim reading 10ms admits a true value anywhere in 2–18ms and stands;
a Negligible claim needs measurement to within 1ms, which is what separates the two classes
at all. Being faster than a budget never fails a claim — the numbers are ceilings.
_Avoid_: benchmark, SLA, target

### Accessibility

**Accessibility tree**:
The a11y nodes tauler pushes for a Panel, per Repaint, to an **AT**. Rebuilt whole
each time — nothing survives between Ticks, the same way the layout tree doesn't —
and reconciled by the platform by NodeId: the `data-tauler-path` child-index path.

**AT**:
A screen-reader/automation client that attaches to tauler through at-spi and then
reads and drives the Accessibility tree. The tree exists for it; nothing is built
or updated when none is attached.

**Activate**:
An AT's "do the default thing" on a node. Behaves as a press at the element's
box origin: the pointer it produces has `x`/`y`/`press_x`/`press_y` of `0` with
real width and height. See ADR 0038.
_Avoid_: click, trigger
