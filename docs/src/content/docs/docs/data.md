---
title: Data and events
description: Reading from subprocesses, sending them messages, and the state a layout can keep.
---

Every piece of external data a bar shows comes from a subprocess. A layout declares what it
wants to read from — on every tick, unconditionally — and tauler decides what to spawn,
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

tauler keeps exactly one value per stream — the latest line. A widget that needs to show
what happened *before* now gets it by piping the stream through `tauler-accumulate`, inside
the script you are already writing:

```jsx
const recent = useJSONStream("/bin/sh", `
  journalctl -f -o json | tauler-accumulate -n 5
`);
```

`recent` is an array of the last five lines, **oldest first**. One array is written per
input line, starting from the first one — the window grows to `-n` rather than staying
blank until it fills, so the widget appears immediately.

It is a ring buffer and nothing more. Each line is parsed as JSON if it parses, and kept as
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
strings if the exact text matters. And a stream is never restarted once its subprocess
exits, which a pipe makes twice as likely, so a dead source keeps rendering its last window
indefinitely.

### A complete widget

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

On a click, tauler finds the **topmost element painted over that point** that carries a
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
| `ctx.screen_width` | monitor width in logical pixels |
| `ctx.screen_height` | monitor height in logical pixels |
| `ctx.outputs` | array of `{ name, screen_width, screen_height }` for every connected output |
| `ctx.dpi` | display DPI |

## `globals`

A plain JS object that persists in the JavaScript context between ticks. It is the only way
to accumulate state across renders — tracking which workspaces have unread notifications
across a stream of events, say.

Use it sparingly. Every tick is otherwise a pure function of the current stream values, and
`globals` is the one thing that breaks that. Prefer deriving what you need from the current
values, and reach for `globals` only when you genuinely need to remember something over
time.
