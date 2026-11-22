---
title: Show and switch i3 workspaces
description: List the workspaces of an i3 or sway session, switch between them, and keep tiled windows clear of your panels.
template: service
---

`tauler-i3` is a program that ships with Tauler. It prints the workspaces of an i3 or
sway session and switches to one when asked. A layout uses it as a
[service](/docs/data/services/).

```jsx
export default function render() {
  const [i3, dispatch] = useService("~/.cargo/bin/tauler-i3");
  return (
    <root>
      <panel id="workspaces" anchor="top" width={ctx.screen_width} height={32}>
        <div class="flex flex-row gap-2 px-2">
          {(i3?.workspaces ?? []).map((ws) => (
            <div class={ws.focused ? "text-foreground" : "text-muted-foreground"}
                 on_click={[dispatch.switchWorkspace({ workspace: ws.name })]}>
              {ws.name}
            </div>
          ))}
        </div>
      </panel>
    </root>
  );
}
```

`i3` is `null` until the first line arrives, so the example falls back to an empty list.

## Install

```sh
cargo install tauler-i3
```

The binary lands in `~/.cargo/bin/tauler-i3`.

## State it prints

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
| `focused` | `true` for the one workspace that has focus. At most one workspace in the whole session is focused, so a workspace that is visible on another screen reports `false`. |
| `urgent` | the workspace's urgency flag |
| `focused_windows` | window titles on the workspace, most recently focused first |

- The list holds the workspaces of the screen named in the startup event (see
  [Settings it reads](#settings-it-reads)). It follows the order in the i3 tree.
  Workspaces whose name starts with `__` are left out.
- A line is written when i3 reports a workspace or window event (changes within 50 ms are
  merged into one line), and at least once a second otherwise.
- If a refresh fails, that line is skipped and the next refresh tries again.

## Events it accepts

| event | effect |
|---|---|
| `dispatch.switchWorkspace({ workspace: "1: web" })` | focus that workspace |

`focusWindow` is not handled: `tauler-i3` ignores it.

## Settings it reads

`tauler-i3` reads two keys of [the startup event](/docs/data/programs/#the-startup-event):
`output`, the name of the screen it lists workspaces for, and `gaps`. It writes nothing
until the event arrives.

Tauler runs one `tauler-i3` for each `bin` path, however many panels or components name
it, and the `output` in its startup event is always the primary screen. Moving the call
into a panel on another screen does not change that. So the workspace list is that of
the primary screen, and a layout cannot get a second screen's workspaces from this
program. If the `output` in the event is empty, as it is on Wayland, the program lists
the workspaces of every screen and writes no gaps.

### Gaps

To have i3 keep tiled windows clear of your panels, pass the gaps to this program.
[`<I3Layout module="~/.cargo/bin/tauler-i3">`](/docs/layout/#i3layout) computes them from
your panels and sends them for you. To state them yourself, pass them as a setting:

```jsx
const [i3, dispatch] = useService("~/.cargo/bin/tauler-i3", {
  gaps: { left: 300, top: 8 },
});
```

`gaps` is an object with `left`, `right`, `top` and `bottom`, in logical pixels. Every
side is declared by the layout: a side you omit reserves nothing, and screens with no
panel are set to zero. The values reach i3 unconverted; see
[Screen layout](/docs/layout/#a-note-on-units) and
[Doing it by hand](/docs/layout/#doing-it-by-hand).

`tauler-i3` applies the gaps with `gaps <side> current set <px>` for all four sides, and
it applies them again:

- when the program starts;
- on every workspace event;
- when the declared `gaps` change.

When the focused workspace is on a different screen than the program's own, it sets all
four sides to `0` instead.

i3 can only set gaps on the workspace that has focus at that moment, so a workspace that was
unfocused when the gaps changed keeps its old gaps until it is next focused. See
[Gaps only reach the focused workspace](/docs/layout/#gaps-only-reach-the-focused-workspace)
for the effect and the static i3 config alternative.

## sway

The program finds the IPC socket from `I3SOCK`, then `SWAYSOCK`, then the output of
`i3 --get-socketpath`, then `sway --get-socketpath`. It lists workspaces and switches to
them under sway the same way as under i3.

Two things were not checked:

- Whether `focused_windows` includes every window under sway. The program counts a node as
  a window only if the i3 IPC tree gives it an X11 window id.
- Whether setting `gaps` behaves the same way under sway.

## When something goes wrong

Problems are logged to stderr, so the JSON lines on stdout stay clean. The log level comes
from `RUST_LOG` and defaults to `info`.

| situation | behaviour |
|---|---|
| No socket path can be found | The program logs an error and exits with status 1. |
| i3 or sway restarts, or the socket goes away | The event connection is retried every second. Requests reconnect and retry once; each connection read and write times out after 5 seconds. |
| A refresh fails | The line is skipped and a warning is logged. |
| Setting a gap fails | A warning is logged, and gaps are tried again on the next workspace event. |
| Stdin closes | The program exits with status 0. |

If the program exits, whatever the status, Tauler starts it again within about half a
second and sends it a new startup event. Until a restarted program writes its first line,
the state keeps its last value.

## Related

- [Offload state to an external program](/docs/data/services/): how a layout reads a
  service and dispatches to it.
- [Screen layout](/docs/layout/): `<I3Layout>`, which sends the gaps for you.
- [macOS](/docs/macos/): `tauler-aerospace` prints the same fields for AeroSpace.
- [Write a program a layout can control](/docs/data/programs/): the startup event this
  program reads.
