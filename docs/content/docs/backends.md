---
title: Backends
description: How panels, wallpapers, scaling and reserved space behave on X11, Wayland and macOS.
---

Tauler draws the same layout on every backend. What differs is how a panel becomes a window,
which `<wallpaper>` works, and whether anything keeps other windows out of a panel's way.
[Running Tauler](/docs/running/#which-display-server-it-uses) says how the backend is picked.

| | X11 | Wayland | macOS |
|---|---|---|---|
| Panel is | an unmanaged window | a layer-shell surface | a window |
| `<wallpaper>` | painted into the root background | not supported yet | a window below all others |
| Anchored panel reserves space | no | yes | no |
| Scale comes from | `Xft.dpi` | the compositor | not covered here |

## X11

**Panels are unmanaged windows.** The window manager does not move, decorate or tile them,
so a panel sits exactly where the layout puts it. It also means a panel does not appear in
`i3-msg -t get_tree`; only the X server knows where it is.

**`above` picks the stacking.** A panel with `above` is raised over other windows. Without
it, the panel is lowered beneath them.

**Outputs are RandR outputs.** The `output` prop takes the name RandR gives (`DP-2`). Omit it
for the primary output. When RandR reports no primary, which is the case on a bare X server
such as Xvfb, Tauler uses the output at the origin, or else the first name in alphabetical
order. Tauler follows output changes while it runs.

**Scale comes from `Xft.dpi`.** Tauler reads `Xft.dpi` from the X resources. Without it, it
computes the DPI from the screen's physical size, and without that it uses 96. The scale is
that DPI divided by 96. `ctx.screen_width` and `ctx.screen_height` are in logical pixels at
that scale. Tauler reads the value again when outputs change.

**Reserving space is your decision.** An unmanaged window cannot ask the window manager to
keep clear of it, so on X11 an `anchor` places a panel and never reserves anything. On i3,
`<I3Layout>` tells i3 the gaps to leave. See [Screen layout](/docs/layout/).

**The wallpaper is the root window's background.** A `<wallpaper>` is painted into the
screen's root pixmap, each one inside the rectangle of its own output. Tauler publishes the
pixmap through `_XROOTPMAP_ID` and `ESETROOT_PMAP_ID`, the way `feh` does, so programs that
read the root background can find it.

## Wayland

**Panels are layer-shell surfaces.** Your compositor must support the layer-shell protocol.

**`above` picks the layer.** A panel with `above` goes on the top layer. Without it, the
panel goes on the bottom layer.

**An anchored panel reserves space.** This differs from X11. On Wayland, a panel with an
`anchor` asks the compositor to keep other windows clear of it: its `width` for `left` and
`right`, its `height` for `top` and `bottom`. A panel without an `anchor` reserves nothing.

The consequence: on Wayland, `anchor` both places and reserves, so the compositor keeps
windows clear without `<I3Layout>`. Using both together, for example `tauler-i3` on sway,
has not been checked and might reserve the space twice. A layout written for X11 and i3, where `anchor` alone leaves windows free to
tile underneath, will push windows aside on Wayland. A panel without an `anchor` reserves
nothing, but its `x` and `y` do not apply: the compositor places it.

**An anchored panel fills its edge.** A `left` or `right` panel spans the full height of the
output, and a `top` or `bottom` panel spans the full width. The compositor decides that
length, so the length set in the layout does not apply.

**There is one output.** Tauler reads the first output the compositor reports. The panel's
`output` prop is not used to choose one.

**Scale comes from the compositor.** Tauler divides the output's physical width by its
logical width. If the compositor gives no logical size, it uses the output's integer scale.
`Xft.dpi` plays no part.

**`<wallpaper>` is not supported yet.** Tauler ignores the node and logs that it is only
supported on X11. Use a
wallpaper program, and note it will not feed `backdrop-filter`; see below.

## macOS

See [macOS](/docs/macos/). It is experimental, and that page covers install, the one manual
measurement, and what to expect.

## Seeing the wallpaper through a panel

A panel gets the wallpaper behind it through `tauler:root-bg`, described in
[the layout file](/docs/layout-file/#seeing-through-a-panel). Only Tauler's own `<wallpaper>`
feeds it. A wallpaper set by `feh` or any other program is not visible to it, even on X11,
where that program paints the same root background Tauler would. A panel then has nothing
behind it to blur.
