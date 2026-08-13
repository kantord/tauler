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

**root-bg**:
The slice of Wallpaper sitting behind one Panel, bound as an image for the duration of a
single render. It is what makes a Panel look transparent; nothing is actually translucent.
_Avoid_: transparency, blur, backdrop

### The layout file

**Layout file**:
The single `.jsx` file that declares everything. There is no other configuration.
_Avoid_: config, config file, theme file

**Tick**:
One full re-evaluation of the layout file, triggered by any stream value changing. Every
tick rebuilds the whole tree from scratch; nothing survives between ticks except
`globals`.
_Avoid_: frame, render pass, update, re-render

**Shell node**:
A node that describes structure rather than content: `root`, `panel`, `wallpaper`. Shell
nodes never reach the rasterizer.
_Avoid_: top-level node, container node

**Layout node**:
A node that describes content and is rasterized: `container`, `text`, `image`.
_Avoid_: leaf node, visual node, element

**Edge layout**:
Deriving both a set of Panels and the matching reservation from one ordered list of edge
declarations, so the two cannot disagree. This is what `<I3Layout>` does.
_Avoid_: docking, packing, auto-gaps

**Panel declaration**:
A `<Panel>` inside an `<I3Layout>` — an instruction to eat `size` off one edge. It is not
a Panel; it *produces* one. Note the casing: lowercase `<panel>` is the surface, capital
`<Panel>` is the declaration.
_Avoid_: panel (unqualified — it is the collision this term exists to prevent)

### Data

**Module**:
A subprocess tauler both reads JSON from and sends intents to. A Module owns a vocabulary;
tauler does not interpret it.
_Avoid_: plugin, provider, service, backend

**Stream**:
A subprocess whose stdout tauler reads, one value per line. Read-only — a stream that is
also written to is a Module.
_Avoid_: source, feed, watcher

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
