# tauler architecture specification

tauler is a **status bar and widget system** for Linux desktops. It is not a general-purpose
GUI framework. The scope is desktop shell surfaces: bars, docks, notification areas, and
similar widgets. Layouts are declared in JSX, data comes from subprocess streams, and
rendering is done in software via takumi + tiny-skia.


## Mental model

The execution model borrows JSX syntax from React but is fundamentally different. Do not
map React concepts onto it — the differences are more significant than the similarities.

**Every tick is a full re-render from scratch.** When any stream value changes, the entire
layout function is re-evaluated and produces a new JSON tree. There is no virtual DOM, no
reconciliation, no diffing, and no component identity across ticks. Panels are cached by
a hash of their output JSON; only panels whose content changed are rasterized.

**The layout function is pure.** It takes the current stream values as inputs and returns a
tree. The only exception is `globals` (see below), which is an explicit escape hatch for
cross-tick state.

A useful mental model: each tick is like a server-side render from the current data snapshot.
The output is a static tree, not a live component hierarchy.


## Scripting language: JavaScript (JSX via rquickjs + OXC)

Layout files are `.jsx` files evaluated by **QuickJS** (via `rquickjs`) with JSX syntax
transformed by **OXC** (`oxc_transformer`). No custom parser or preprocessor — two maintained
Rust crates, both available on crates.io. Requires a C compiler at build time (QuickJS is
vendored via `rquickjs-sys`, no system libraries needed).

### How it works

1. **On layout file load or change**: OXC parses the JSX source and locates the last
   top-level `ExpressionStatement` via the AST. Everything before it becomes the body of
   `globalThis._render`; the final expression becomes its return value. OXC then transforms
   JSX syntax to `_jsx(...)` calls and emits plain JS.
2. **On each data tick**: The QuickJS `Runtime` and `Context` are kept alive between ticks
   (created once, reused forever). Stream values are updated in a shared map, then
   `_render()` is called. No reparse, no recompile. Returns a JS object tree. Rust walks
   the tree to extract panels and build takumi node trees.

OXC transform + wrap: **<1ms** (layout-file-change only). QuickJS `_render()` call:
**~100–200μs**. Dominant cost is always takumi + skia rasterization.

### Sandbox

rquickjs is deny-by-default. No filesystem, network, or process access unless explicitly
registered from Rust. tauler exposes only `useStringStream`, `useJSONStream`, `useEvents`,
`Module`, `globals`, and `ctx`.

### Example layout file

```jsx
function TimeCard() {
  const time = useStringStream("/usr/bin/bash", `
    while true; do date +"%H:%M"; sleep 1; done
  `);
  return (
    <container tw="flex flex-col gap-1 rounded-lg border px-3 py-2">
      <text tw="text-[10px] opacity-60">TIME</text>
      <text tw="text-[14px] text-white">{time}</text>
    </container>
  );
}

<root>
  <panel anchor="left" width={250} height={ctx.screen_height}>
    <container tw="flex flex-col h-full w-full px-4 py-4">
      <Module bin="/home/kantord/.cargo/bin/tauler-i3">
        {(data, events) => <WorkspaceList workspaces={data?.workspaces} events={events} />}
      </Module>
      <TimeCard />
    </container>
  </panel>
</root>
```

Components are plain JS functions. No framework, no hooks protocol.


## Layout file

The layout file is the single source of configuration. There is no `config.yaml`. If
future top-level settings are needed they go as props on `<root>`.

The file is watched for changes and hot-reloaded. On reload all subprocesses are restarted
and stream values are cleared.

The file is re-evaluated on every data tick (stream value change). Re-evaluation is cheap
(~100–200μs) because `_render()` is pre-compiled and the QuickJS context is reused.


## Nodes

Low-level nodes map directly to takumi nodes. The script produces a JS object tree; Rust
walks it to construct the takumi node tree. No intermediate representation.

`_jsx` is registered from Rust as a global. It receives `(tag, props, ...children)` and
returns a plain JS object `{ type, ...props, children }`.

### Layout nodes (inside panels)

| node | description |
|---|---|
| `container` | flex container, maps to takumi container |
| `text` | text node |
| `image` | image node |

### Shell nodes (top-level structure)

| node | description |
|---|---|
| `root` | mandatory top-level node, contains `panel` and `wallpaper` nodes |
| `panel` | declares one desktop surface (X11 window / Wayland layer surface) |
| `wallpaper` | paints its subtree into the desktop background of one output |

Both surface kinds parse into the same `SurfaceSpec`; only the destination of the
rasterized buffer differs. `<surface type="panel">` and `<surface type="wallpaper">`
are accepted long-hand spellings — a `type` prop overrides the tag name during JSX
flattening, so they are indistinguishable from the short forms by the time Rust sees
them. A bare `<surface>` names no kind and is a parse error.

### `<panel>` props

| prop | type | description |
|---|---|---|
| `anchor` | `"left" \| "right" \| "top" \| "bottom"` | stick to this screen edge. omit for a free-floating panel. does **not** reserve space — see "Reserving space" below |
| `width` | number | panel width in logical pixels |
| `height` | number | panel height in logical pixels |
| `x` | number | x position (ignored when `anchor` is set) |
| `y` | number | y position (ignored when `anchor` is set) |
| `above` | boolean | stack above other windows (for overlays like notifications) |
| `output` | string | RandR output name, e.g. `"DP-2"`. omit for primary output |
| `outer_gap` | number | gap reserved around screen edges |

### `<wallpaper>` props

| prop | type | description |
|---|---|---|
| `output` | string | RandR output name, e.g. `"DP-2"`. omit for primary output |

A `<wallpaper>` has no geometry props: it always covers its output exactly, and its
subtree is laid out against those dimensions. It reserves no strut space and receives
no clicks — there is no window, only pixels handed to the desktop background.

```jsx
<root>
  <wallpaper output="DP-2">
    <container tw="flex w-full h-full items-end justify-end p-12"
               style={{ backgroundImage: "linear-gradient(160deg, #0b1020, #1c2b4a)" }}>
      <text tw="text-[28px] text-white opacity-30">{time}</text>
    </container>
  </wallpaper>
</root>
```

Everything a wallpaper does is ordinary layout. Scaling or cropping a photo is
`<image>` plus `object-fit`; a solid colour or gradient is a `<container>` with a
background — there is no wallpaper-specific fitting, tiling or colour handling, and
none is planned. The buffer is a straight pixel passthrough.

### Fake transparency (`root-bg`)

A panel is rasterized into its own isolated buffer, so there is nothing behind it
to show through. To give it something, tauler binds the slice of wallpaper each
panel covers as an image named `root-bg`, for the duration of that one render:

```jsx
<panel id="sidebar" anchor="left" width={272} height={ctx.screen_height}>
  <container style={{ position: "relative", width: "100%", height: "100%" }}>
    <image src="root-bg" style={{ position: "absolute", top: 0, left: 0,
                                  width: "100%", height: "100%" }} />
    <container tw="h-full w-full p-2" style={{ position: "relative" }}>
      <container tw="h-full w-full rounded-2xl"
                 style={{ backgroundColor: "rgba(20,20,24,0.55)" }}>
        ...
      </container>
    </container>
  </container>
</panel>
```

**Use an `<image>` node, not `backgroundImage: url(root-bg)`.** Both work, but
takumi's background-image path rebuilds per-pixel state that does not depend on
the pixel — including a validating `PixmapRef::from_bytes` for every pixel — so a
full-height panel costs ~19ms/render against a ~6ms floor. The `<image>` node
path hoists that setup correctly and costs ~5ms. Keep the overlaying content
`position: relative` rather than `absolute`: one out-of-flow sibling avoids the
multiple-absolute-siblings bug family documented in
`docs/takumi-absolute-sibling-bug-research.md`, and still paints above the image.

The pixels come from tauler's own `<wallpaper>` render, matched by output — a
wallpaper set by another program (feh, xwallpaper) is not visible to this. A
panel on an output with no `<wallpaper>` gets no `root-bg` at all: the key is
removed rather than left holding the last panel's crop, so a bare output renders
without a backdrop instead of borrowing its neighbour's pixels.

Wallpapers and panels are reconciled as two separate sets, wallpapers first, so
the backdrop is never a tick stale. Ordering cannot come from sorting the specs:
`OptativeSet::reconcile` dedups them into a `HashMap` and iterates its keys, and
runs `update_existing` before `enter_new` — so a sorted input buys nothing and a
newly added wallpaper would otherwise land after every existing panel.

Freshness needs two things, and either alone is insufficient. The reconciler
compares each panel's last-rendered wallpaper generation against the registry's
current one, so a panel whose own spec is byte-identical is still re-rendered
when the wallpaper moves. And the render cache is keyed by `(generation, rect)`:
the generation stops a stale hit, while the rect tells two same-size,
same-content panels apart — without it they collide on one entry and the second
is served the first one's slice of wallpaper.

On X11 the buffer is blitted into the root window's background pixmap, and
`_XROOTPMAP_ID` / `ESETROOT_PMAP_ID` are published so pseudo-transparent clients can
find it. Wayland and macOS do not support `<wallpaper>` yet; the node is ignored with
a warning there.


## Components

Components are plain JS functions that take props and return a node tree. JSX handles
`<Card />` as a function call naturally — no registration needed.

### Global context (`ctx`)

`ctx` is injected by Rust before each evaluation. Read-only.

| field | description |
|---|---|
| `ctx.screen_width` | monitor width in logical pixels |
| `ctx.screen_height` | monitor height in logical pixels |
| `ctx.outputs` | array of `{ name, screen_width, screen_height }` for all connected outputs |
| `ctx.dpi` | display DPI |

### Cross-tick state (`globals`)

`globals` is a plain JS object that persists in the QuickJS context between ticks. It is
the only mechanism for accumulating state across renders — for example, tracking which
workspaces have unread notifications across a stream of notification events.

Use it sparingly. Because `globals` is mutated as a side effect during `_render()`, the
layout function is no longer pure when it uses `globals`. Prefer deriving everything from
the current stream values when possible; reach for `globals` only when you genuinely need
to accumulate state over time.


## Data layer

All external data flows through the data layer. The same subprocess registry underlies all
three calling conventions.

### Subprocess identity

The identity key is `(bin, script)`. On each re-evaluation, Rust diffs the old set against
the new one: unchanged identities reuse the running subprocess, removed ones are killed,
new ones are spawned.

Registering a bin as a module changes its spec, and a changed spec restarts the
subprocess. So **call the hooks for a given bin unconditionally**, at the same level of the
same component — never inside a branch that sometimes does not render. A hook that comes
and goes restarts its subprocess on every transition, which for a singleton like
`tauler-notify` means dropped notifications and a momentarily released D-Bus name.

### `useStringStream(bin, script?)`

Returns the latest stdout line from the subprocess as a string. This is not a React hook —
it is a Rust-registered global that reads the current value from a map. There are no
ordering rules, no dependency arrays, and no cleanup functions. Calling it registers the
subprocess identity for the current tick; Rust handles spawning and reconciliation.

```jsx
const time = useStringStream("/usr/bin/bash", `
  while true; do date +"%H:%M"; sleep 1; done
`);
```

### `useJSONStream(bin, script?)`

Same as `useStringStream` but parses each stdout line as JSON and returns a JS object.

```jsx
const data = useJSONStream("/usr/bin/myscript");
```

### `useEvents(bin)`

Registers the subprocess and returns a Proxy for addressing it. Each property is a
function; calling it produces an **intent** — a plain JSON object naming a destination and
the message to deliver there.

```jsx
const notify = useEvents("/home/kantord/.cargo/bin/tauler-notify");

notify.dismiss({ id: 42 })
// { "channel": "/home/kantord/.cargo/bin/tauler-notify",
//   "event": { "type": "dismiss", "id": 42 } }
```

The property name becomes `event.type`; the argument's keys are merged alongside it.
Calling with no argument yields just the type.

### Event handlers

`on_click` is **always an array of intents**, never a bare object and never a JS callback.
Functions do not survive the JSON boundary at the end of evaluation, so a handler that is a
function is silently dropped.

When a click is detected, Rust hit-tests the rendered tree for the deepest node carrying an
`on_click`, then delivers each intent's `event` object **verbatim** to that intent's
channel over stdin — one JSON object per line, with no wrapping envelope. A module
therefore only ever sees its own vocabulary; it never learns that a click caused the
message. No JS executes on click.

Because a handler is a list, one gesture can address several subprocesses at once. Each
intent is delivered independently, in no guaranteed order across channels; an intent naming
an unknown channel is logged and skipped without affecting the others.

```jsx
on_click={[
  i3.switchWorkspace({ workspace: ws.name }),
  notify.dismiss({ id: n.id }),
]}
```

Scroll wheel motion is not a click. X11 reports it as button presses 4–7; those are
discarded before hit-testing.

### `<Module bin="...">{(data, events) => ...}</Module>`

Sugar over `useJSONStream` + `useEvents` for the common case of a subprocess you both read
from and talk to. Sends an init event on startup, receives JSON data on stdout, and accepts
intents on stdin. The child render function receives:

- `data` — latest parsed JSON output from the subprocess
- `events` — the same proxy `useEvents` returns

```jsx
<Module bin="/home/kantord/.cargo/bin/tauler-i3">
  {(data, events) => (
    <WorkspaceList workspaces={data?.workspaces} events={events} />
  )}
</Module>
```

### Module props

Any prop other than `bin` and `children` is merged into the init payload and written to
the subprocess's stdin — once at spawn, and again whenever the value changes. Identical
props are not re-sent, so a module only sees real changes. Keys of the derived init
payload (`type`, `config`, `output`, `dpi`, …) win over declared props; that payload is
the module protocol, not user-editable state.

`tauler-i3` reads `gaps` this way. Every side is declared, never derived; an omitted side
reserves nothing, and outputs with no panel are revoked to zero regardless.

`gaps` values are **logical pixels** and tauler passes them through untouched — see
"Reserving space" below for why there is no conversion.

```jsx
<Module bin="/home/kantord/.cargo/bin/tauler-i3" gaps={{ left: 300, top: 8 }}>
  {(data, events) => <WorkspaceList workspaces={data?.workspaces} events={events} />}
</Module>
```


### Reserving space

`anchor` places a panel; it does not reserve space for it. On X11 tauler creates panels
as **override-redirect** windows, which the window manager does not manage at all — so a
`_NET_WM_STRUT_PARTIAL` on them would never be read, and tauler does not set one. i3 would
not honour it anyway: it classifies dock clients as `W_DOCK_TOP` or `W_DOCK_BOTTOM` only
(`include/data.h`), and consults just the top/bottom struts (`src/manage.c`). There is no
left/right dock, so a full-height sidebar can never reserve space via struts.

Space is therefore reserved by declaring it, through `tauler-i3`'s `gaps` prop. Rather
than writing both the panel sizes and the matching gaps by hand, use `<I3Layout>`, which
derives one from the other:

```jsx
<I3Layout module="~/.cargo/bin/tauler-i3">
  <Panel id="sidebar" anchor="left" size={272}>…</Panel>
  <Panel id="topbar"  anchor="top"  size={26}>…</Panel>
</I3Layout>
```

Each `<Panel>` eats from an edge of the remaining rectangle in document order, and the
four amounts eaten become the gaps — so a size can never drift from its reservation. See
"Edge layout" below.

The `gaps` prop remains available directly for cases `<I3Layout>` does not cover; tauler
never derives it, because "how much space should i3 reserve" is a decision, not a
consequence of geometry.

The values reach i3 unconverted. i3's own `cmd_gaps` opens with `logical_px(atoi(value))`
(`src/commands.c`), and `logical_px` is `ceil(dpi/96 * value)` above a 1.25 DPI threshold,
identity below (`libi3/dpi.c`) — so **i3's gap unit is already logical pixels**, the same
unit a layout is written in. Scaling here would only have to be undone there, with
different rounding. i3's grammar accepts a trailing `px` keyword, but it is discarded and
`cmd_gaps` has no unit parameter; there is no physical-pixel path.

### Edge layout (`<I3Layout>` / `<Panel>`)

`<I3Layout>` positions panels around a screen and reports what they consumed, so the
panel sizes and the i3 gaps cannot disagree.

```jsx
<I3Layout module="~/.cargo/bin/tauler-i3">
  <Panel id="sidebar"   anchor="left"   size={272}>…</Panel>
  <Panel id="topbar"    anchor="top"    size={26}>…</Panel>
  <Panel id="dock"      anchor="left"   size={120}>…</Panel>
  <Panel id="bottombar" anchor="bottom" size={26}>…</Panel>
</I3Layout>
```

**Document order is the API.** Each `<Panel>` takes `size` logical pixels off one edge of
what is left, then the next sees the smaller rectangle. Above, `topbar` starts to the
right of `sidebar` rather than under it, and `dock` sits below `topbar`. Swapping two
lines swaps which one owns the corner. The four running totals are the gaps.

| prop | description |
|---|---|
| `id` | surface id, as on `<panel>` |
| `anchor` | `"left" \| "right" \| "top" \| "bottom"`. An unrecognised value reserves nothing |
| `size` | thickness along the anchored axis, logical px. The other axis fills what is left |
| `output` | RandR output name; omit for primary |

`<I3Layout>` takes one prop, `module` — the bin to send the computed gaps to, normally
`tauler-i3`. Omit it and `<I3Layout>` is pure geometry, registering nothing.

The panels it emits are ordinary `<panel>` nodes with explicit `x`/`y`/`width`/`height`
and no `anchor` — `anchor` only pins a surface to a monitor edge, which cannot express
"below the bar declared before me".

Two implementation notes, because both look like bugs otherwise:

- **`<I3Layout>` is a JS shim** (`JSX_GLOBALS_JS`) over a Rust component
  (`ui::components::i3_layout`). The arithmetic is Rust and unit-tested; the shim only
  dispatches. It has to be JS because registering the gaps is a JS-side call
  (`useEvents`), and a Rust component receives serde data with no `Ctx` to call back with.
- **The gaps are registered after the children.** `<I3Layout>` cannot know them until every
  `<Panel>` has been evaluated, and children evaluate before their parent. That is why
  `registerModule` merges registrations for a bin instead of keeping the first — see
  `jsx::merge_missing`.

## Display backend

### Abstraction boundary

All display-server-specific code lives behind a clear boundary. The main loop never calls
x11rb directly — it only calls through the panel abstraction. Adding Wayland support later
requires only a new implementation of that abstraction, with zero changes to the core loop,
JSX evaluation, render pipeline, or data layer.

### X11 panel

Current implementation. One `XPanel` struct per `<panel>` node. Responsibilities:
- Create and configure the X11 window (override-redirect; no struts — see ADR 0001)
- Accept BGRX pixel buffers and put them to the window via `XPutImage`
- Report button-press events up to the main loop
- Expose monitor geometry so panels can size themselves via `ctx.screen_width` /
  `ctx.screen_height`

### Wayland panel (future)

Will implement the same interface using the wlr-layer-shell protocol
(`zwlr_layer_surface_v1`). `anchor` maps directly to layer-shell anchor edges. Strut
reservation is handled automatically by the compositor.


## Rendering

Each panel has its own `RenderCache` keyed by the canonical JSON of its subtree
(`json_canon`). Only panels whose content changed since the last tick are re-rasterized.
Rasterization is software-only (takumi + tiny-skia). The dominant cost is the full-panel
rasterization pass (~40–90ms at 365×2160); all upstream steps (JSX eval, layout parse,
cache key check) are <1ms.
