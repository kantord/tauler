---
title: tauler-i3
description: The workspace data source for i3 and sway — install, the JSON it emits, the intents it accepts, and how it reserves space.
---

`tauler-i3` is a separate program that reports the workspaces of an i3 or sway session and
switches between them. A layout reads it through
[`<Module>`](/docs/data/#module), the same way it reads any other subprocess.

## Install

```sh
cargo install tauler-i3
```

The binary lands in `~/.cargo/bin/tauler-i3`.

## Use it in a layout

```jsx
<Module bin="~/.cargo/bin/tauler-i3">
  {(data, events) =>
    data?.workspaces?.map(ws => (
      <div
        class={ws.focused ? "text-white" : "text-white/50"}
        on_click={[events.switchWorkspace({ workspace: ws.name })]}
      >
        {ws.name}
      </div>
    ))
  }
</Module>
```

`data` is `undefined` until the first line arrives, so guard it with `data?.workspaces`.
`events` is the proxy described in [Data and events](/docs/data/#useeventsbin).

To have i3 keep tiled windows clear of your panels, pass the gaps to this process.
[`<I3Layout module="~/.cargo/bin/tauler-i3">`](/docs/layout/#i3layout) computes them from
your panels and sends them for you. Writing the `gaps` prop on `<Module>` by hand is
covered in [Doing it by hand](/docs/layout/#doing-it-by-hand).

## What it emits

One JSON object per line:

```json
{
  "workspaces": [
    { "name": "1: web", "focused": true, "urgent": false, "focused_windows": [] }
  ]
}
```

| field | meaning |
|---|---|
| `name` | the workspace name |
| `focused` | `true` for the one workspace that has focus. At most one workspace in the whole session is focused, so a workspace that is visible on another output reports `false`. |
| `urgent` | the workspace's urgency flag |
| `focused_windows` | window titles on the workspace, most recently focused first |

- The list holds the workspaces of the output named in the init event (see below). It follows the
  order in the i3 tree. Workspaces whose name starts with `__` are left out.
- A line is written when i3 reports a workspace or window event (changes within 50 ms are
  merged into one line), and at least once a second otherwise.
- If a refresh fails, that line is skipped and the next refresh tries again.

## What it accepts

A layout sends an intent with `events.<type>({...})`. `tauler-i3` acts on one:

| intent | effect |
|---|---|
| `events.switchWorkspace({ workspace: "1: web" })` | focus that workspace |

`focusWindow` is not handled: `tauler-i3` ignores it.

## The init event and outputs

Tauler sends an `init` event as the first line on the process's stdin. `tauler-i3` reads
two things from it: `output`, the name of the output the process lists workspaces for, and
`gaps`. It writes nothing until the event arrives.

Tauler runs one `tauler-i3` process per `bin` path, however many panels or `<Module>`
declarations name it, and the `output` in its init event is always the primary output.
Moving the declaration into a panel on another output does not change that. So the
workspace list is that of the primary output, and a layout cannot get a second output's
workspaces from this program. If the `output` in the event is empty, the process lists the
workspaces of every output and writes no gaps.

## Gaps

`gaps` is an object with `left`, `right`, `top` and `bottom`, in logical pixels. Every
side is declared by the layout: a side you omit reserves nothing. `<I3Layout>` fills the
object in from your panels.

`tauler-i3` applies the gaps with `gaps <side> current set <px>` for all four sides. The
values reach i3 unconverted, and it applies them again:

- when the process starts;
- on every workspace event;
- when the declared `gaps` change.

When the focused workspace is on a different output than the process's own, it sets all
four sides to `0` instead.

i3 can only set gaps on the workspace that has focus at that moment, so a workspace that was
unfocused when the gaps changed keeps its old gaps until it is next focused. See
[Gaps only reach the focused workspace](/docs/layout/#gaps-only-reach-the-focused-workspace)
for the effect and the static i3 config alternative.

## sway

The process finds the IPC socket from `I3SOCK`, then `SWAYSOCK`, then the output of
`i3 --get-socketpath`, then `sway --get-socketpath`. It lists workspaces and switches to
them under sway the same way as under i3.

Two things were not checked:

- Whether `focused_windows` includes every window under sway. The process counts a node as
  a window only if the i3 IPC tree gives it an X11 window id.
- Whether the `gaps` commands behave the same way under sway.

## When something goes wrong

Problems are logged to stderr, so the JSON lines on stdout stay clean. The log level comes from `RUST_LOG` and defaults to `info`.

| situation | behaviour |
|---|---|
| No socket path can be found | The process logs an error and exits with status 1. |
| i3 or sway restarts, or the socket goes away | The event connection is retried every second. Requests reconnect and retry once; each connection read and write times out after 5 seconds. |
| A refresh fails | The line is skipped and a warning is logged. |
| A gap command fails | A warning is logged, and gaps are tried again on the next workspace event. |
| Stdin closes | The process exits with status 0. |

If the process exits, whatever the status, Tauler starts it again within about half a
second and sends it a new init event. Until a restarted process writes its first line,
`data` keeps its last value.
