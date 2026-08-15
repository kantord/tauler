# tauler-notify

A desktop notification data source for the [tauler](https://crates.io/crates/tauler)
status bar.

It is a real freedesktop notification daemon: it claims
`org.freedesktop.Notifications` on the session bus, so applications send it notifications
the usual way. Instead of drawing popups it emits the current notification list on
stdout, leaving the presentation entirely to your layout.

Because it takes the D-Bus name, it replaces daemons like dunst or mako rather than
running alongside them.

## Install

```sh
cargo install tauler-notify
```

## Use from a layout

```jsx
<Module bin="/home/you/.cargo/bin/tauler-notify">
  {(data, events) =>
    data?.notifications?.map(n => (
      <div class="flex flex-col px-2 py-1" on_click={events.dismiss({ id: n.id })}>
        <span class="text-[11px] opacity-60">{n.app_name}</span>
        <span class="text-[13px] text-white">{n.summary}</span>
      </div>
    ))
  }
</Module>
```

## Protocol

Emitted on stdout, one object per line, whenever the set of live notifications changes:

```json
{
  "notifications": [
    { "id": 1, "app_name": "Slack", "summary": "New message", "body": "…", "urgency": 1 }
  ]
}
```

`urgency` follows the freedesktop scale: `0` low, `1` normal, `2` critical.

Accepted on stdin:

| intent | effect |
|---|---|
| `{"type": "dismiss", "id": 42}` | close notification 42 and emit `NotificationClosed` |

Notifications expire on their own timeout (the spec's default of 5 s when the client
asks for the server default, never for a timeout of `0`).

When stdin closes the parent bar is gone, so the daemon exits rather than lingering and
holding the D-Bus name.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
