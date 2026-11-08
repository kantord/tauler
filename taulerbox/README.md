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

Experimental. It opens a resizable window titled `taulerbox`, draws the layout's Panels in
it at their anchors, and composites the compartment's desktop beside them over VNC. The
mouse is forwarded to the compartment (left, middle and right buttons and vertical
scroll); there is no keyboard input. Streams are empty and the layout context is a fixed
1482x700. It is not built or tested in CI. See the
[published page](https://kantord.github.io/tauler/docs/develop/taulerbox/) for what to
expect.

taulerbox does not create the compartment. The sandbox comes from a `Compartment` unit
declared in the layout (see `fixtures/compartment/layout.op.mdx`); `--vnc` only connects
to a server that is already running.

## Use

```sh
taulerbox <layout.op.mdx> --home <dir> [--vnc <host:port>] [--compartment-name <name>]
```

- `--home` must be an existing directory. It is checked and otherwise unused for now.
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
