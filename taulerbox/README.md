# taulerbox

A native window (Linux and macOS) that shows a [tauler](https://crates.io/crates/tauler)
layout's Panels as raw pixel buffers, drawn alongside a microVM compartment running a
nested desktop. See issue [#582](https://github.com/kantord/tauler/issues/582).

Where native tauler creates one desktop window per Panel, taulerbox creates one window
and blits every Panel's frame into it at its declared position — the same
`SurfaceCommand`/`SurfaceFrame` protocol tauler's X11, Wayland and macOS presenters
already speak, with a single synthesized output standing in for a real monitor.

taulerbox depends on `tauler` as a plain library, the way `tauler-screenshot` does. It
does not reuse `tauler`'s tick loop (`src/app.rs`), which is private to the `tauler`
binary; it drives its own, thinner one.

Sandboxing is deliberately taulerbox-only. Native tauler never gains a compartment
feature.

## Status

Early scaffolding. No window yet.

## Use

```sh
taulerbox <layout.op.mdx> --home <dir> [--vnc <host:port>] [--compartment-name <name>]
```

- `--home` is bind-mounted as the compartment's home directory.
- `--vnc` connects to a compartment's `wayvnc` server and composites its live desktop into
  the window next to the panel(s). The window is resizable: resizing repositions panels
  instantly, and — with `--compartment-name` also given — resizes the compartment's own
  Sway output (via `msb exec ... swaymsg output ... resolution ...`) after the resize
  settles, then re-fetches a frame at the new resolution. Without `--compartment-name`,
  a resize still repositions panels and re-fits the existing VNC frame into the new
  destination rect; it just cannot make Sway itself change resolution.
- `--compartment-name` names the `msb` sandbox `--vnc` is talking to. For the
  `fixtures/compartment` layout, that sandbox is named `taulerbox-verify-582` (see that
  fixture's `Compartment(...)` call).
