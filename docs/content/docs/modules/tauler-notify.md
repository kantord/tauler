---
title: tauler-notify
description: A notification data source that replaces dunst or mako and hands the notification list to your layout.
---

`tauler-notify` is a [Module](/docs/data/#module) that reads desktop notifications. It is a
real freedesktop notification daemon: it claims `org.freedesktop.Notifications` on the
session bus, so applications send it notifications the usual way. Instead of drawing
popups it prints the current notification list on stdout, and your layout decides how to
show it.

Because it takes that D-Bus name, it replaces daemons like dunst or mako. It cannot run
beside them: if another daemon already owns the name, `tauler-notify` exits at startup
with the message `D-Bus connection failed (session bus unavailable, or another daemon owns
org.freedesktop.Notifications)`. Stop the other daemon first.

## Install

```sh
cargo install tauler-notify
```

## Use it in a layout

Declare it with `<Module>`. The child function receives the parsed JSON as `data` and the
intent proxy as `events`:

```jsx
<Module bin="/home/you/.cargo/bin/tauler-notify">
  {(data, events) =>
    data?.notifications?.map(n => (
      <div class="flex flex-col px-2 py-1" on_click={[events.dismiss({ id: n.id })]}>
        <span class="text-[11px] opacity-60">{n.app_name}</span>
        <span class="text-[13px] text-white">{n.summary}</span>
      </div>
    ))
  }
</Module>
```

To read the list without sending intents, `useJSONStream("/home/you/.cargo/bin/tauler-notify")`
works the same way. See [Data and events](/docs/data/) for both.

:::caution
Declare the module unconditionally, at the same level of the same component. A module that
sits inside a branch which sometimes does not render restarts the process on every
transition. That drops the notifications held in memory and briefly releases the D-Bus
name. See [Declare subprocesses unconditionally](/docs/data/#declare-subprocesses-unconditionally).
:::

## Read the notification list

`tauler-notify` writes one JSON object per line on stdout:

```json
{
  "notifications": [
    { "id": 1, "app_name": "Slack", "summary": "New message", "body": "…", "urgency": 1, "enwiro_env": null }
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
`data` has a value from the start. A new line is written whenever a notification is added,
replaced, closed, or expires. Notifications are listed in arrival order. A notification
that reuses the id of a live one replaces it in its existing position.

Some clients, such as volume and brightness OSDs, send a stack tag with their
notifications (the `x-canonical-private-synchronous` or `x-dunst-stack-tag` hint). Notifications
with the same tag share one id, so each new one replaces the previous.

## Dismiss a notification

`tauler-notify` accepts one intent on stdin:

| Intent | Effect |
|---|---|
| `events.dismiss({ id })` | Closes the notification with that `id` and emits the `NotificationClosed` signal on D-Bus, with reason 2 (dismissed). |

The notification disappears from the next list. Dismissing an id that is not live does
nothing and emits no signal. Any other line on stdin, including the init event from
`<Module>`, is ignored.

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
