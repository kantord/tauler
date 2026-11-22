---
title: Show desktop notifications
description: Receive desktop notifications in place of dunst or mako, and draw the list in your layout.
template: service
---

`tauler-notify` is a program that ships with Tauler. It receives desktop notifications
and prints the current list, and your layout decides how to show it. A layout uses it as
a [service](/docs/data/services/).

```jsx
export default function render() {
  const [notify, dispatch] = useService("~/.cargo/bin/tauler-notify");
  return (
    <root>
      <panel id="notifications" anchor="right" width={320} height={ctx.screen_height}>
        <div class="flex flex-col gap-1">
          {(notify?.notifications ?? []).map((n) => (
            <div class="flex flex-col px-2 py-1" on_click={[dispatch.dismiss({ id: n.id })]}>
              <span class="text-[11px] text-muted-foreground">{n.app_name}</span>
              <span class="text-[13px] text-foreground">{n.summary}</span>
            </div>
          ))}
        </div>
      </panel>
    </root>
  );
}
```

To read the list without dismissing anything, take only the first half of the pair, or
call `useJSONStream("~/.cargo/bin/tauler-notify")`.

:::caution
Call `useService` for it on every run of the layout, never inside a branch that
sometimes does not run. A call that comes and goes restarts the program on every
transition. That drops the notifications held in memory and briefly releases the D-Bus
name. See [the rule on the streams page](/docs/data/streams/#rules).
:::

## Install

```sh
cargo install tauler-notify
```

It is a real freedesktop notification daemon: it claims `org.freedesktop.Notifications`
on the session bus, so applications send it notifications the usual way. Because it takes
that D-Bus name, it replaces daemons like dunst or mako. It cannot run beside them: if
another daemon already owns the name, `tauler-notify` exits at startup and logs
`D-Bus connection failed (session bus unavailable, or another daemon owns
org.freedesktop.Notifications)`. Stop the other daemon first.

## State it prints

One JSON object per line:

```json
{
  "notifications": [
    { "id": 1, "app_name": "Slack", "summary": "Build finished", "body": "…", "urgency": 1, "enwiro_env": null }
  ]
}
```

| Field | Meaning |
|---|---|
| `id` | Notification id, a number. Clients can reuse an id to replace a notification. |
| `app_name` | The sending application's name. |
| `summary` | The notification's title line. |
| `body` | The notification's body text, as sent. |
| `urgency` | Freedesktop scale: `0` low, `1` normal, `2` critical. `1` when the client sends no urgency hint. |
| `enwiro_env` | Value of `ENWIRO_ENV` in the sending process's environment, or `null` when it cannot be read or is not set. Linux only: it is read from `/proc`. |

An empty list, `{"notifications": []}`, is written as soon as the daemon is running, so
the state has a value from the start. A new line is written whenever a notification is
added, replaced, closed, or expires. Notifications are listed in arrival order. A
notification that reuses the id of a live one replaces it in its existing position.

Some clients, such as volume and brightness OSDs, send a stack tag with their
notifications (the `x-canonical-private-synchronous` or `x-dunst-stack-tag` hint). Notifications
with the same tag share one id, so each new one replaces the previous.

## Events it accepts

| Event | Effect |
|---|---|
| `dispatch.dismiss({ id })` | Closes the notification with that `id` and emits the `NotificationClosed` signal on D-Bus, with reason 2 (dismissed). |

The notification disappears from the next list. Dismissing an id that is not live does
nothing and emits no signal. Any other line on stdin, including
[the startup event](/docs/data/programs/#the-startup-event), is ignored.

## Timeouts

Notifications expire on the timeout the client requests:

- A client that asks for the server default (`-1`) gets 5 seconds.
- A timeout of `0` means the notification never expires.
- Any other value is taken as milliseconds.

When one expires it is removed from the list and `NotificationClosed` is emitted with
reason 1 (expired). A client that calls `CloseNotification` gets reason 3 (closed).

## What is not supported

The daemon advertises a single capability, `body`. Specifically:

- Action buttons are accepted from clients but ignored; no action is ever reported back.
- The application icon and image hints are ignored and do not appear in the JSON.
- Only `urgency` is read from the hints, plus the stack tags above.
- `summary` and `body` are passed through as plain strings. Markup is not interpreted.

## Exit behaviour

When stdin closes, the parent Tauler process is gone, and `tauler-notify` exits at once
rather than keep holding the D-Bus name.

## Related

- [Offload state to an external program](/docs/data/services/): how a layout reads a
  service and dispatches to it.
- [Read a value from an external program](/docs/data/streams/#rules): why the call must
  not come and go.
- [Do something on a click](/docs/interaction/clicks/): the handler that dismisses.
