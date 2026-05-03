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
registered from Rust. tauler exposes only `useStringStream`, `useJSONStream`, `Module`,
`globals`, and `ctx`.

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
| `root` | mandatory top-level node, contains one or more `panel` nodes |
| `panel` | declares one desktop surface (X11 window / Wayland layer surface) |

### `<panel>` props

| prop | type | description |
|---|---|---|
| `anchor` | `"left" \| "right" \| "top" \| "bottom"` | stick to this screen edge and reserve strut space. omit for a free-floating panel |
| `width` | number | panel width in logical pixels |
| `height` | number | panel height in logical pixels |
| `x` | number | x position (ignored when `anchor` is set) |
| `y` | number | y position (ignored when `anchor` is set) |
| `above` | boolean | stack above other windows (for overlays like notifications) |
| `output` | string | RandR output name, e.g. `"DP-2"`. omit for primary output |
| `outer_gap` | number | gap reserved around screen edges |


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

### `<Module bin="...">{(data, events) => ...}</Module>`

A bidirectional subprocess. Sends an init event on startup, receives JSON data on stdout,
and accepts events on stdin. The child render function receives:

- `data` — latest parsed JSON output from the subprocess
- `events` — a Proxy that generates JSON event descriptors

`on_click` values produced via `events` are plain JSON objects, not JS callbacks. When a
click is detected, Rust performs a hit-test on the rendered tree, finds the `on_click`
value, and writes it to the subprocess's stdin. No JS executes on click.

```jsx
<Module bin="/home/kantord/.cargo/bin/tauler-i3">
  {(data, events) => (
    <WorkspaceList workspaces={data?.workspaces} events={events} />
  )}
</Module>
```


## Display backend

### Abstraction boundary

All display-server-specific code lives behind a clear boundary. The main loop never calls
x11rb directly — it only calls through the panel abstraction. Adding Wayland support later
requires only a new implementation of that abstraction, with zero changes to the core loop,
JSX evaluation, render pipeline, or data layer.

### X11 panel

Current implementation. One `XPanel` struct per `<panel>` node. Responsibilities:
- Create and configure the X11 window (override-redirect, strut properties)
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
