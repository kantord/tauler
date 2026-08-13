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

`on_click` is **always an array of intents** — never a bare object, and never a callback.
Functions do not survive the JSON boundary at the end of evaluation, so a handler that is a
function is silently dropped.

```jsx
on_click={[
  i3.switchWorkspace({ workspace: ws.name }),
  notify.dismiss({ id: n.id }),
]}
```

On a click, tauler hit-tests the rendered tree for the deepest node carrying an `on_click`,
then delivers each intent's `event` object verbatim to that intent's channel over stdin —
one JSON object per line, no wrapping envelope. No JavaScript runs on click.

A module therefore only ever sees its own vocabulary, and never learns that a click caused
the message. Because a handler is a list, one gesture can address several subprocesses at
once; each intent is delivered independently and in no guaranteed order, and an intent
naming an unknown channel is logged and skipped without affecting the others.

Scroll wheel motion is not a click. X11 reports it as button presses 4–7, and those are
discarded before hit-testing.

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
[Screen layout](/tauler/layout/).

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
