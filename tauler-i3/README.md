# tauler-i3

An i3 and sway workspace data source for the [tauler](https://crates.io/crates/tauler)
status bar.

It speaks tauler's module protocol: newline-delimited JSON in on stdin, newline-delimited
JSON out on stdout. It connects to the i3/sway IPC socket, subscribes to workspace and
window events, and emits the current workspace list whenever it changes.

## Install

```sh
cargo install tauler-i3
```

## Use from a layout

```jsx
<Module bin="/home/you/.cargo/bin/tauler-i3">
  {(data, events) =>
    data?.workspaces?.map(ws => (
      <span
        class={ws.focused ? "text-white" : "text-white/50"}
        on_click={events.switchWorkspace({ workspace: ws.name })}
      >
        {ws.name}
      </span>
    ))
  }
</Module>
```

## Protocol

tauler sends an `init` event first, carrying the output name, DPI and bar geometry.
Nothing is emitted before it arrives.

Emitted on stdout, one object per line:

```json
{
  "workspaces": [
    { "name": "1: web", "focused": true, "urgent": false, "focused_windows": [] }
  ]
}
```

Accepted on stdin:

| intent | effect |
|---|---|
| `{"type": "switchWorkspace", "workspace": "1: web"}` | focus that workspace |
| `{"type": "focusWindow", "workspace": "1: web"}` | focus a window on that workspace |

Workspaces are filtered to the output the bar instance is running on, so a multi-monitor
setup runs one process per bar.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
