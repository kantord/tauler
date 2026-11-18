---
title: Data and events
description: Reading from subprocesses, sending them messages, and the state a layout can keep.
---

Every piece of external data your layout shows comes from a subprocess. A layout declares what it
wants to read from — on every tick, unconditionally — and Tauler decides what to spawn,
keep, or kill.

## Declare subprocesses unconditionally

A subprocess is identified by the `(bin, script)` pair it was declared with. Each tick,
that set is diffed against the running one: unchanged identities keep their process, new
ones are spawned, and ones that vanished are killed.

That has a consequence worth internalising before writing anything else:

:::caution
Call the hooks for a given bin **unconditionally**, at the same level of the same
component. Never inside a branch that sometimes does not render.
:::

A hook that comes and goes restarts its subprocess on every transition. For a singleton
like `tauler-notify` that means dropped notifications and a momentarily released D-Bus
name — which looks like a broken notification daemon, not like a conditional in a layout
file.

Two components asking for the same `(bin, script)` share one subprocess, without either
knowing about the other.

## `useStringStream(bin, script?)`

The latest stdout line from the subprocess, as a string.

```jsx
const time = useStringStream("/usr/bin/bash", `
  while true; do date +"%H:%M"; sleep 1; done
`);
```

Despite the name, this is not a React hook. It is a Rust-registered global that reads the
current value out of a map. There are no ordering rules, no dependency arrays and no
cleanup functions. Calling it registers the subprocess for this tick.

## `useJSONStream(bin, script?)`

The same, but each stdout line is parsed as JSON and returned as an object.

```jsx
const data = useJSONStream("/usr/bin/myscript");
```

## History: `tauler-accumulate`

Tauler keeps exactly one value per stream — the latest line. A layout that needs to show
what happened *before* now gets it by piping the stream through `tauler-accumulate`, inside
the script you are already writing:

```jsx
const recent = useJSONStream("/bin/sh", `
  journalctl -f -o json | tauler-accumulate -n 5
`);
```

`recent` is an array of the last five lines, **oldest first**. One array is written per
input line, starting from the first one — the window grows to `-n` rather than staying
blank until it fills, so the layout has something to show immediately.

It keeps the last `-n` lines and nothing more. Each line is parsed as JSON if it parses, and kept as
a string if it does not, so a bare number arrives as a number and an error message arrives
as a string:

```
$ printf '0.41\n0.52\noops\n' | tauler-accumulate -n 2
[0.41]
[0.41,0.52]
[0.52,"oops"]
```

There is no query or filter option, because both sides of it already have one. To trim fat
lines before they are buffered, put `jq` in the pipe:

```sh
journalctl -f -o json | jq -c .MESSAGE | tauler-accumulate -n 5
```

To reshape the window afterwards, do it in the layout file, which is JavaScript:

```jsx
const load = useJSONStream("/bin/sh", `
  while :; do cut -d' ' -f1 /proc/loadavg; sleep 1; done | tauler-accumulate -n 60
`);

const average = load.reduce((a, b) => a + b, 0) / load.length;
const newestFirst = [...load].reverse();
```

Two things to know. Numbers are re-serialized, so `0.60` comes back as `0.6` — accumulate
strings if the exact text matters. And when the subprocess exits, Tauler starts it again
within about half a second. A restarted pipe begins with an empty window, because the
`tauler-accumulate` process that held the history is new.

### A complete component

The window is an ordinary array, so an existing component consumes it with no glue:

```jsx
import { DataTable } from "@ui/datatable";

function RecentLogs() {
  const lines = useJSONStream("/bin/sh", `
    journalctl -f -o json --output-fields=MESSAGE,_COMM | tauler-accumulate -n 5
  `) ?? [];

  return (
    <div class="flex flex-col gap-2 rounded-lg border px-3 py-3">
      <span class="text-[10px] text-foreground opacity-60">RECENT</span>
      <DataTable
        columns={[{ key: "_COMM", label: "UNIT" }, { key: "MESSAGE", label: "MESSAGE" }]}
        rows={[...lines].reverse()}
      />
    </div>
  );
}
```

And the window is what makes "peak over the last minute" expressible at all — the latest
line on its own cannot say it:

```jsx
function Load() {
  const load = useJSONStream("/bin/sh", `
    while :; do cut -d' ' -f1 /proc/loadavg; sleep 1; done | tauler-accumulate -n 60
  `) ?? [];

  const now  = load.length ? load[load.length - 1] : 0;
  const peak = load.length ? Math.max(...load) : 0;
  const mean = load.length ? load.reduce((a, b) => a + b, 0) / load.length : 0;

  return (
    <div class="flex flex-col gap-1 rounded-lg border px-3 py-2">
      <span class="text-[10px] text-foreground opacity-60">LOAD</span>
      <span class="text-[18px] text-foreground">{now.toFixed(2)}</span>
      <span class="text-[11px] text-foreground opacity-70">
        {`peak ${peak.toFixed(2)} · avg ${mean.toFixed(2)} · ${load.length} samples`}
      </span>
    </div>
  );
}
```

Both guard against an empty window with `?? []`, because a stream has no value until its
first line arrives.

## `useEvents(bin)`

Registers the subprocess and returns a proxy for addressing it. Every property is a
function, and calling one produces an **intent** — a plain JSON object naming a destination
and the message to deliver there.

```jsx
const notify = useEvents("~/.cargo/bin/tauler-notify");

notify.dismiss({ id: 42 })
// { "channel": "~/.cargo/bin/tauler-notify",
//   "event": { "type": "dismiss", "id": 42 } }
```

The property name becomes `event.type`, and the argument's keys are merged alongside it.
Calling with no argument yields just the type.

## Event handlers

A handler is **an array of intents**, or **a function returning one**. Both forms work on
`on_click` and on `on_drag`.

```jsx
on_click={[
  i3.switchWorkspace({ workspace: ws.name }),
  notify.dismiss({ id: n.id }),
]}

// same thing, computed when the click happens
on_click={p => [i3.switchWorkspace({ workspace: ws.name })]}
```

Reach for the array unless you need the pointer. The function form exists because a drag
has to turn a position into a number, and it receives the same argument everywhere for
consistency. A handler that throws is logged once and dispatches nothing.

On a click, Tauler finds the **topmost element painted over that point** that carries a
handler, then delivers each intent's `event` object verbatim to that intent's channel over
stdin — one JSON object per line, no wrapping envelope. A handler written as an array is
sent as it stands; one written as a function is called first, and what it returns is sent.
Either way nothing rewrites the intent on the way out.

An element counts as carrying a handler if it has `on_click` **or** `on_drag`, so an
element that only drags still shadows a clickable one beneath it.

:::caution[Put handlers on block-level elements]
`on_click` only fires on an element that has a box of its own — a `<div>`, or any element
that is a flex or grid item. A plain inline element like a `<span>` sitting in a run of
text has no box, so a handler on it never fires.

```jsx
// Nothing happens: the span is inline, so it has no box to click.
<p>PRs: <span on_click={[gh.open({ repo })]}>{count}</span></p>

// Works: the div is a flex item, so it has one.
<div class="flex flex-row">
  <div on_click={[gh.open({ repo })]}>{count}</div>
</div>
```

You do not have to guess which case you are in. The first click on the surface logs a
warning naming the element as you wrote it:

```
WARN on_click on a node that is never painted on its own — inline elements
     cannot take clicks; move the handler to a block-level element
     node=<span id="pr-count" class="text-[11px]">
```
:::

A module therefore only ever sees its own vocabulary, and never learns that a click caused
the message. Because a handler is a list, one gesture can address several subprocesses at
once; each intent is delivered independently and in no guaranteed order, and an intent
naming an unknown channel is logged and skipped without affecting the others.

Scroll wheel motion is not a click. X11 reports it as button presses 4–7, and those are
discarded before hit-testing.

## Controls

A control — `<Slider>`, `<Knob>`, `<ScrollArea>` — is a component that emits intents when
you press or drag it, and holds no value of its own. What that means, and how `on_drag`
lets any element become one, is on the [components page](/docs/components/#control-components).

## `<Module>`

Sugar over `useJSONStream` + `useEvents`, for the common case of a subprocess you both read
from and talk to. It sends an init event on startup, reads JSON from stdout, and accepts
intents on stdin.

```jsx
<Module bin="~/.cargo/bin/tauler-i3">
  {(data, events) => (
    <WorkspaceList workspaces={data?.workspaces} events={events} />
  )}
</Module>
```

The child function receives `data` — the latest parsed JSON — and `events`, the same proxy
`useEvents` returns.

### Module props

Any prop other than `bin` and `children` is merged into the init payload and written to the
subprocess's stdin: once at spawn, and again whenever the value changes. Identical props
are not re-sent, so a module only ever sees real changes.

Keys of the derived init payload (`type`, `config`, `output`, `dpi`, …) win over declared
props — that payload is the module protocol, not user-editable state.

`tauler-i3` reads its `gaps` this way. Every side is declared rather than derived; an
omitted side reserves nothing, and outputs with no panel are revoked to zero regardless.
The values are logical pixels and reach i3 untouched — see
[Screen layout](/docs/layout/).

```jsx
<Module bin="~/.cargo/bin/tauler-i3" gaps={{ left: 300, top: 8 }}>
  {(data, events) => <WorkspaceList workspaces={data?.workspaces} events={events} />}
</Module>
```

Note that registering a bin as a module changes its spec, and a changed spec restarts the
subprocess — the same rule as above, for the same reason.

## `ctx`

Injected by Rust before each evaluation. Read-only.

| field | description |
|---|---|
| `ctx.screen_width` | primary output width in logical pixels |
| `ctx.screen_height` | primary output height in logical pixels |
| `ctx.outputs` | array of `{ name, screen_width, screen_height }` for every connected output |
| `ctx.dpi` | display DPI |
| `ctx.theme` | the active theme: `{ name, mode, colors }` — see [Colours as values](/docs/themes/#colours-as-values), and `themeColor` for a colour at an alpha |

## `store`

A Store is an object your layout keeps between ticks. Everything else is rebuilt from the
current stream values on every tick; a Store is where you put what has to be remembered:
whether a drawer is open, which workspaces have unread notifications, the value of a
slider that no subprocess owns.

```jsx
import { store } from "tauler";

const desk = store({ open: false, pinned: [] });

export default () => (
  <root>
    <panel id="bar" anchor="top" width={1920} height={32}>
      <div on_click={() => { desk.open = !desk.open; }}>
        {desk.open ? "close" : "open"}
      </div>
      {desk.open && <Drawer pinned={desk.pinned} />}
    </panel>
  </root>
);
```

### Declare it

Call `store` with the default value, as the whole value of a `const` at the top level of
a file:

```jsx
const desk = store({ open: false, pinned: [] });
export const filters = store({ range: "1h" });
```

- The default is an object. What it holds must be JSON: numbers, strings, booleans,
  `null`, arrays and objects.
- A file can declare several Stores, and export them. Every file that imports a Store
  gets the same one.
- `store(...)` anywhere else — inside a function, in a `let`, as an array element — fails
  the load, with the file and the line. So does a second argument.
- `store` is an import, not a global. A file that calls it without
  `import { store } from "tauler"` fails the load with a message that says so.

Tauler finds a Store again by the file that declares it and the name of its `const`. You
never write a key for it, and two files can both call theirs `state`.

### Read and write it

Read a Store like any object, anywhere: in a component, in a handler, at the top level
of a file.

Write it with a plain assignment. Changing a nested value in place works too:

```jsx
desk.open = true;
desk.pinned.push("mail");
delete desk.note;
```

### What causes a tick

- A write from an event handler causes one tick, so the screen shows the new value.
  Several handlers that write at once still cause one tick, and a handler that leaves
  every value as it was causes none. A handler that only writes a Store does not have to
  return intents.
- A write made while the layout renders is kept and causes no tick. It shows on the next
  tick that something else causes. This is the place for bookkeeping that follows the
  data, like remembering which workspace was focused last.
- A handler or a render that throws leaves every Store as it was before it ran.

### What it survives

A Store keeps its value when the layout file is saved and reloaded. A key you add to the
default appears with its default value; the keys you already had keep theirs. Renaming
the `const`, or moving it to another file, starts that Store from its default again.
A Store you delete is forgotten.

A Store does not survive a restart of Tauler.

### Hooks and draw code read it

A [Unit](/docs/units/#driving-a-unit-from-your-layout)'s hooks and a
[`<D3Figure>`](/docs/components/#change-the-key-when-the-drawing-should-change)'s `draw`
read the same Stores your layout does. They cannot write them: an assignment there
throws. A write does not run either of them sooner. A Unit sees the new value at its
next Sweep, and a figure is drawn again when its `key` changes.
