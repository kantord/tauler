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
A surface painted into the desktop background of one Screen. It has no window, takes no
clicks, and always covers its Screen exactly.
_Avoid_: background, backdrop, root image, desktop

**Dom**:
A surface whose output is markup rather than pixels. The browser is what rasterizes it,
which is why it is the one surface kind tauler draws nothing for.
_Avoid_: mount, web surface, canvas, root

**Mount node**:
The element in a web page a Dom surface's markup is written into. Everything inside it is
tauler's; everything outside it is the page's.
_Avoid_: host, container, target, portal

**Screen**:
One connected display, named as the display server names it (`DP-2`).
_Elsewhere_: RandR output (the connector, not RandR 1.5 monitors, which are logical regions),
Wayland `wl_output`, i3/sway output
_Avoid_: output (generic), monitor (audio monitors), display

**Anchor**:
The edge of a Screen a surface is pinned to. An anchor *places* a surface. On X11 and macOS it
never reserves space for it. On Wayland the backend also asks the compositor to keep other
windows clear of an anchored surface (ADR 0001).
_Avoid_: dock edge, side, alignment

**Reservation**:
Space the window manager keeps clear of tiled windows. On X11 and macOS it is always
stated as a decision, never derived from a surface's geometry. On Wayland the backend
derives it from an anchored surface's size and hands it to the compositor.
_Avoid_: strut, exclusive zone (both are display-server mechanisms for expressing this
concept, not the concept itself)

**Gaps**:
The four per-edge distances that express a reservation to i3. The unit is the logical
pixel — see **Logical pixel**.
_Avoid_: margins, padding, insets

**Built-in name**:
A name starting `tauler:` that tauler answers itself instead of reading a file or running
a program: a Built-in stream such as `tauler:outputs`, or an image such as `tauler:root-bg`.
The prefix is what stops a file of the same name from shadowing it.
_Avoid_: reserved name, magic name, builtin
_Elsewhere_: the `node:` prefix for Node's own modules, `about:` URLs in browsers

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

**Picture**:
The pixels of one D3Figure's drawing on the desktop, named by that figure's key and size
and kept so that every later Frame can show them without drawing again. A key names one
Picture per size for good: a figure that should look different at the same size must say
so with a new key.
_Avoid_: image (any `<img>` source), bitmap, buffer (both name a Frame's pixels), drawing
(the act), canvas

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

**Draw runtime**:
The QuickJS runtime that runs D3Figures' draw functions, on a thread of its own. It loads
the same layout file as the render runtime, only to find the figures it is asked for, and
hands back markup — the Render worker is still the only thing that rasterizes. A slow draw
delays its Picture, never a Tick.
_Avoid_: draw thread, worker (the Render worker is the worker), sandbox

**Presenter**:
What turns Frames into pixels on a display server and pointer input into events for the
loop. It blocks on commands and display events together and never on a clock — see ADR
0042.
_Avoid_: display thread, backend (the backend is the whole seam; the Presenter is the
part of it that runs), driver, event loop (what a Presenter is built from, not what it is)

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
frontmatter block at its top — theme choice and font choice: what to render with, never what
to render. Anything describing a surface, its contents or its data belongs in the layout
file's body, not its frontmatter. On the legacy split-file path, the frontmatter's place is
taken by a sibling `config.yaml` instead, and the file itself is `layout.jsx`.
_Avoid_: config, config file (both name the frontmatter or `config.yaml`, not this)

**Package**:
An `@gh/<owner>/<repo>` import a layout file names directly — a git repository tauler
fetches, caches, and grants the same trust its own layout file has, shell exec included
(see ADR 0041). Pinned to a commit by a Lockfile: a YAML file sibling to the layout file,
mapping `owner/repo` to a commit and whether it is in **Development mode**. Importing one
tauler has never seen is what creates its Lockfile entry, pinned to whatever commit was
current at that moment — there is no separate "install" step.
_Avoid_: dependency, module, library (a **Module** already names a different
relationship — a subprocess, not a git repository)
_Elsewhere_: package (npm, pip), dependency (Cargo) — the closest analogue in every one
of those ecosystems, but this project's own Cargo packages (`tauler-core`, `tauler-i3`,
...) are a separate, unrelated use of the same English word within this very repository;
only context tells the two apart, the same way "config" is disambiguated elsewhere in
this glossary.

**Development mode**:
A Package fetched once and never re-pinned or auto-updated afterward — `tauler pkg
update` skips it. For someone actually developing the package, not merely using it: the
checkout is theirs to edit by hand. Marked in the Lockfile, never in the layout file
itself.
_Avoid_: manual, unlocked, untracked (git already owns "untracked" for a different,
unrelated state)

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
One full re-evaluation of the layout file, triggered by a stream value changing or by a
Handler writing to a **Store**. Every tick rebuilds the whole tree from scratch; nothing
survives between ticks except what the layout file's Stores hold. A **Pass** may happen
without one, and usually does.
_Avoid_: frame, render pass, update, re-render; pass (that is the loop turn, not the
re-evaluation)

**Store**:
A JSON object a layout file declares with `store(...)` to keep between Ticks. Where it is
declared decides who shares it: a **shared Store** or a **local Store**. What a Store holds
is the only thing this glossary calls state. See ADR 0051.
_Avoid_: globals, atom, slice, context
_Elsewhere_: store (zustand, Svelte, Redux), atom (Jotai)

**Shared Store**:
A **Store** declared at the top level of a module: one value, shared by every component
that imports it, kept until a layout reload that no longer declares it. A function that
declares a Store, called through a `const` at the top level of a module, declares a shared
Store too.
_Avoid_: global store, module store
_Elsewhere_: a module-level store (zustand), a top-level atom (Jotai)

**Local Store**:
A **Store** declared inside a function. Each **component instance** that reaches the
declaration has its own value, and the value is dropped by the first Tick that does not
reach the declaration.
_Avoid_: hook state, `useLocal`, component state (hook alone names a Unit's callback — see **Hook**)
_Elsewhere_: `useState` (React), `$state` in a component (Svelte), except that neither is
found again by name

**Component instance**:
One place a component is written: the components whose code wrote it, outermost first, and
its `key` prop among siblings written by the same component. A **local Store** is kept
once per component instance. See ADR 0051.
_Avoid_: instance alone (that is avoided for **Item**), fiber, node, mounted component
_Elsewhere_: a component instance or fiber (React), which is a position in the output tree

**Key**:
What says which thing this is. The same key is the same thing as last time, and what is
kept for it stays; a new key is a new thing, and what is kept for it starts over. A
**component instance** keeps its **local Stores** under its `key` prop, a D3Figure keeps
its **Picture**, and a Unit keeps its **Item**.
_Avoid_: id (a `<Panel>`'s `id` names its surface, and is visible outside tauler)
_Elsewhere_: `key` (React), where a new key also unmounts and mounts again; a cache key

**State identity**:
What finds the same Store again on the next Tick, after a layout reload, and from another
runtime. For a shared Store it comes from the module that declares it and the name it is
declared under; for a local Store, from its **component instance** and names in the code
as well. No person writes one. See ADR 0051.
_Avoid_: key (that is one field of a Store, a JSX prop, or what identifies an Item), id, name
_Elsewhere_: an atom's `key` (Recoil) and a store's id (Pinia), both of which a person
writes; a hook's call order (React), which this replaces with names

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

**Workspace area**:
The rectangle i3 tiles real application windows into — what is left of the screen once
every Panel's reservation is subtracted. Never a surface tauler draws; `<Workspaces>`
frames it without ever covering it.
_Avoid_: workspace (unqualified — i3 already owns that word for a virtual desktop; this
is the area one occupies on screen, not the desktop itself)

### Themes

**Theme**:
One palette for one appearance: a named set of colours, corner radii and font roles, labelled
dark or light. A colour scheme that comes in a dark and a light style is two themes.
_Avoid_: skin, colour scheme (a scheme is what a built-in theme is made from, not the theme)
_Elsewhere_: a VS Code colour theme (one per appearance, labelled `vs` or `vs-dark`), a
tinted-theming scheme (`variant: dark | light`)

**Theme mode**:
The dark or light label a theme carries. A config may set its own, which relabels the theme
without touching its colours and so changes only which `dark:` / `light:` class variants apply.
_Avoid_: appearance, theme type

**Theme file**:
A YAML file that defines one theme, named by the layout file's frontmatter to use that theme
instead of a built-in one. Not the layout file and not its frontmatter.
_Avoid_: palette file, colour file

**Built-in theme**:
A theme that ships with tauler and is chosen by its name instead of by a file. The default
theme is the built-in used when none is named, one per mode.
_Avoid_: preset, bundled theme, stock theme
_Elsewhere_: the themes VS Code bundles

**Theme source**:
Where a bar's theme comes from: a built-in theme by name, a theme file, or nothing named, which
means the default theme. Naming both a built-in and a file is a mistake, never a preference
order.
_Avoid_: theme selection

**Core token**:
A colour name every theme must define because something in tauler reads it.
_Avoid_: required colour, mandatory key
_Elsewhere_: shadcn/ui's colour variables (`background`, `primary`, `muted-foreground`, …),
whose vocabulary tauler borrows

**ANSI token**:
One of the 16 terminal colours a theme defines, normal and bright, so a theme can colour a
terminal as well as the bar.
_Avoid_: terminal colour, term colour
_Elsewhere_: a terminal's `color0`–`color15`, base16's ANSI mapping

**Pair**:
A text colour and the surface colour it is read on, such as a card's foreground on the card.
The unit a theme's contrast is judged in.
_Avoid_: combo, contrast pair

**Swatch scene**:
The one layout every theme is rendered through — each token, each pair, the terminal colours and
a few components — so a theme's defects are visible at a glance.
_Avoid_: demo, preview, mockup

**Query**:
A question put to a running Neovim that has loaded a colour scheme, answered with one colour: a
highlight group's foreground or background, or a global such as a terminal colour.
_Avoid_: lookup, selector, mapping

**Query table**:
For each theme token, the query whose answer is its colour, with a separate query for dark and for
light themes where they differ. The whole of how a built-in theme is made from a colour scheme:
nothing is blended, nudged or guessed.
_Avoid_: mapping, preset

**Override**:
A query that one workload gives for one token in place of the query table's, so a scheme whose own
answer is unusable can still ship.
_Avoid_: patch, fix-up

**Workload**:
One colour scheme at a pinned revision, named for the built-in theme it produces, with any
overrides it needs.
_Avoid_: preset, import

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

**D3Figure**:
A Display component whose pixels come from the layout file's own d3 code: its draw
function is handed an empty `<svg>` and d3, as on a web page. On the desktop what it draws
becomes the figure's Picture; in a page it stays the inline `<svg>` it was drawn into. Its
key is what tells tauler that nothing changed.
_Avoid_: D3View ("view" is avoided above), chart (a map is a D3Figure too), canvas, plot
_Elsewhere_: figure (Observable Plot), view (Vega), an `<svg>` ref handed to d3 in a
`useEffect` (React)

**Control component**:
A Display component that also emits intents. It never holds a value: it renders the value
it is given and remembers nothing. What owns the value is a Module, a Stream or a **Store**.
_Avoid_: input, interactive component, widget
_Elsewhere_: controlled component (React). Observable's `viewof` is the opposite — see
ADR 0012 and ADR 0051.

**Handler**:
What an element does when the pointer reaches it: either an array of intents, or a function
from the pointer to one. A function may also write to a **Store**. `on_click` and `on_drag`
both take either — see ADR 0021.
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
A Stream that tauler also sends to: a subprocess registered by `useEvents` or `<Module>`.
Two-way, but lopsided — what it is sent is delivered latest-wins (see **Outbox**), and
nothing correlates a reply with the Event that may have caused it. A Module owns a
vocabulary; tauler does not interpret it.
_Avoid_: plugin, provider, service, backend

**Stream**:
A source tauler reads one value from, keeping only the latest: a subprocess's stdout, or
a Built-in stream named `tauler:…`. Read-only — a subprocess that is also sent to is a
Module.
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
An Event with its Channel attached and not yet sent: `{channel, event}`. Built by calling
a property on the proxy `useEvents` returns; it is data, never a callback. A Handler holds
Intents, and tauler sends them when the pointer reaches it.
_Avoid_: action, event handler, callback, command

**Event**:
The payload of an Intent — `{type, ...}` — delivered verbatim to a subprocess's stdin as
one JSON line. A command sent out, not something that happened to the layout, and not a
notification: nothing tells the author it was received. The word follows `dispatchEvent`
and `CustomEvent` in the DOM: the author creates it, the subprocess receives it.
_Avoid_: message, notification, signal
_Elsewhere_: `CustomEvent` (DOM), except that nothing here bubbles or is observed back

**Outbox**:
What holds a Module to one intent at a time: the intent in flight, and the newest one
waiting behind it. A Module that has not answered is handed nothing, and what it gets next
is the newest thing produced since — never a backlog. See ADR 0025.
_Avoid_: queue, buffer, backpressure (it is the mechanism, not the effect)

**Channel**:
The address half of an Intent — the subprocess, by its bin, that the Event is for. A
Module only ever sees Events addressed to its own Channel, and never learns what caused
them.
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

**Hook**:
A function a Unit's declaration supplies for one transition of an Item — `enter`, `update`,
`exit` — run by the Sweep. Not React's hook, and not local state, which is a **Local
Store**. The `use*` data functions are hook-like only in being called unconditionally
(a subprocess's identity is its bin and script); they are not Hooks in this sense.
_Avoid_: callback, lifecycle method, effect
_Elsewhere_: lifecycle hook (Kubernetes operators, Terraform provisioners); not `useEffect` (React)

**Item**:
One instance of a Unit, identified by its key. What the layout tree yields, what an
Observation reports, and what a hook receives. Two Items with the same key are the same
thing, whether one came from the tree and the other from the world.
_Avoid_: instance, entry, record

**Observation**:
The set of Items the world currently holds, as a Unit reports it. The only thing that
establishes what is really there: a hook's return value is not one, and neither is the fact
that a hook was run. See ADR 0035.
_Avoid_: state (unqualified, that is what a **Store** holds), snapshot, current state,
actual state

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

**Trace**:
The recorded timings of one input's journey to pixels, one entry per Hop. What a
Latency claim is measured with; a Hop nobody traced has no class. See ADR 0042.
_Avoid_: log, timing, profile (a profile is where CPU goes; a trace is where time goes)

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
