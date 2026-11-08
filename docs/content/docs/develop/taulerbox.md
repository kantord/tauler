---
title: taulerbox
description: An experimental window that shows a layout's Panels next to a sandboxed desktop. What runs today and what does not.
---

:::danger[Experimental]
`taulerbox` is a prototype for [issue #582](https://github.com/kantord/tauler/issues/582).
It is not part of the `tauler` binary, and nothing here is stable.

**What works today**

- One resizable window titled `taulerbox` opens and shows every Panel of a layout, drawn as
  pixels at the position its `anchor` gives.
- With `--vnc`, the window also shows a live VNC picture of a desktop and forwards your
  mouse to it: movement, left/middle/right buttons and the scroll wheel.
- A layout can start a microVM sandbox through the `msb` CLI. The repository's
  `taulerbox/fixtures/compartment` layout does this.

**What does not work today**

- Panels are drawn once, when the program starts. They never update, and they ignore clicks.
- Stream data does not reach the layout.
- The sandboxed desktop gets no keyboard input.
- Only Panels are drawn. Wallpapers and Dom surfaces are ignored, and Reservations do nothing.
- `taulerbox` does not start the sandbox on its own. Your layout file has to.
:::

`taulerbox` opens one window instead of one desktop window per Panel. It draws every Panel
into that window and, if you ask for it, a sandboxed desktop beside them. Native Tauler
draws Panels in separate windows, and it has no sandbox feature. Sandboxing exists in
`taulerbox` only, and Tauler will not gain it.

## Run it

```sh
taulerbox <layout.op.mdx> --home <dir> [--vnc <host:port>] [--compartment-name <name>]
```

| Argument | Required | Meaning |
| --- | --- | --- |
| `<layout.op.mdx>` | yes | The layout file to evaluate. It must be an existing file. |
| `--home <dir>` | yes | A directory. `taulerbox` stops with an error if it is not one. See [Known limitations](#known-limitations). |
| `--vnc <host:port>` | no | The address of a VNC server, such as the `wayvnc` inside the sandbox. `taulerbox` connects to it and shows its picture. |
| `--compartment-name <name>` | no | The name of the `msb` sandbox to resize when you resize the window. It has an effect only together with `--vnc`. |

Without `--vnc`, the window shows the Panels and nothing else.

Set `RUST_LOG` to change how much `taulerbox` logs. The default is `info`.

## Requirements

- Linux or macOS. The window code has no per-platform branches, but the repository's
  continuous integration does not build or test `taulerbox`.
- A Rust toolchain. Build it from a checkout with `cargo build --release -p taulerbox`.
- For the sandbox only:
  - the `msb` CLI, which runs the microVM sandboxes;
  - `docker buildx`, to build the desktop image.

The desktop image is Debian with `sway` and `wayvnc` installed. Build it and load it into
`msb` from a checkout:

```sh
just taulerbox-image
```

This leaves an image named `taulerbox-box` in the local `msb` image cache.

## Try it without a sandbox

The checkout has a one-Panel layout. It needs neither `msb` nor Docker:

```sh
taulerbox taulerbox/fixtures/hello/layout.op.mdx --home /tmp
```

A 300 by 200 blue Panel reading `hello taulerbox` appears against the left edge of the
window.

## Try it with the sandboxed desktop

The `taulerbox/fixtures/compartment` layout defines a `Compartment` element at the top of
the layout file. When the layout is evaluated, that element creates an `msb` sandbox named
`taulerbox-verify-582` from the `taulerbox-box` image, with 2 CPUs, 1 GB of memory and guest
port 5900 published to host port 5900. It then starts `sway` and `wayvnc` inside it. Run:

```sh
just taulerbox-image
taulerbox taulerbox/fixtures/compartment/layout.op.mdx --home /tmp \
  --vnc 127.0.0.1:5900 --compartment-name taulerbox-verify-582
```

The window opens at once. `taulerbox` retries the VNC connection every 2 seconds for up to
200 seconds while the sandbox boots. The desktop appears when the connection succeeds. If
every attempt fails, `taulerbox` logs the failure and the window keeps showing the Panels.

`Compartment` is defined in that layout file, not in Tauler. To sandbox a desktop from your
own layout, copy the definition from the fixture.

## What the window shows

- The window opens at 1482 by 700 pixels and you can resize it.
- Each Panel is drawn at the position its `anchor` gives, measured against the window as if
  the window were a single Output at the origin. Panels use the same placement rule as
  native Tauler.
- A Panel that falls outside the window is not drawn. A Panel that sticks out past an edge
  is cut off at that edge.
- The sandboxed desktop is drawn to the right of the Panel that touches the window's left
  edge. If no Panel touches it, the desktop starts at the left. It keeps a 40 pixel margin
  from the other three edges.
- The picture keeps its aspect ratio. Space the picture does not cover stays black.
- The area behind everything is black.

## Resize the window

Resizing the window moves the Panels at once. It also refits the sandboxed desktop's last
picture into the new space.

If you passed `--compartment-name`, `taulerbox` also changes the sandbox's own `sway`
output resolution after you stop resizing for about 300 ms. The desktop is then redrawn at
the new size. Without `--compartment-name`, the picture is only scaled, so text in it gets
blurry or small instead of being redrawn.

The desktop area never gets larger than 1280 by 720 pixels, however large the window is.
The rest of the window stays black.

## Use the mouse in the sandboxed desktop

While the pointer is over the desktop area, the pointer is sent to the sandboxed desktop
and the sandbox's own cursor is shown. Left, middle and right buttons and the vertical
scroll wheel work. Other mouse buttons are ignored. Keyboard input is not forwarded.

## Known limitations

- **Panels are static.** They are rendered once and never repaint. Clock, workspace and
  other live content does not change, and clicks on a Panel do nothing.
- **`--home` does nothing yet.** `taulerbox` checks that the directory exists and uses it for
  nothing else. The bind mount into the sandbox comes from the `home` argument of the
  `Compartment` definition in your layout, and the fixture sets it to `null`.
- **`--vnc` does not start a sandbox.** It only connects to a VNC server that is already
  listening. `taulerbox` does not discover the sandbox for you.
- **No keyboard.** Typing in the window does not reach the sandboxed desktop.
- **High CPU use.** `taulerbox` asks the VNC server for a complete new picture about 30
  times a second, whether or not anything changed. This is the reason for the 1280 by 720
  cap and for the sandbox's 2 CPUs.
- **No VNC authentication.** `taulerbox` connects without a password, as the `wayvnc`
  default setup expects.
- **Sandbox name.** `--compartment-name` must match the name your layout gave the
  sandbox. `taulerbox` does not check it.
- **Resize problems are only logged.** If `msb` cannot be run or does not answer within 20
  seconds, `taulerbox` writes the error to the terminal and keeps the old resolution.
- **Closing the window ends the program.** It does not stop the sandbox.
- **Only Panels.** Wallpapers and Dom surfaces in the layout are skipped.
- **Single Output.** The context passed to the layout reports an Output named `taulerbox`
  with a scale of 1.0 and the size 1482 by 700. It does not change when you resize.
