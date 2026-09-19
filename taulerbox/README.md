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
taulerbox <layout.op.mdx> --home <dir>
```

`--home` is bind-mounted as the compartment's home directory.
