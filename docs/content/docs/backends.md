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
| `<wallpaper>` | painted into the root background | a background-layer surface | a window below all others |
| Anchored panel reserves space | no | yes | no |
| Scale comes from | `Xft.dpi`, or the output's physical size for a panel with `output` | the compositor | the primary display's scale factor |

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

**Scale has two sources.** A panel without an `output` prop, and `ctx.screen_width` and
`ctx.screen_height`, use the context scale. Tauler reads `Xft.dpi` from the X resources;
without it, it computes the DPI from the screen's physical height, and without that it uses
96. The context scale is that DPI divided by 96, and `ctx.screen_width` and
`ctx.screen_height` are in logical pixels at that scale. A panel with an `output` prop
instead uses that output's own density, taken from the physical size RandR reports for it, so
`Xft.dpi` does not affect it. On a mixed-DPI setup, a panel with `output` therefore renders
at that monitor's real density while the panel's declared size, if it comes from
`ctx.screen_width` arithmetic, follows `Xft.dpi`. Tauler recomputes both when outputs change.

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
So `anchor` both places and reserves, and the compositor keeps windows clear without
`<I3Layout>`. A layout written for X11 and i3, where `anchor` alone leaves windows free to
tile underneath, pushes windows aside on Wayland.

**The compositor places the panel.** `x` and `y` do not apply on Wayland, with or without an
`anchor`.

**An anchored panel fills its edge.** A `left` or `right` panel spans the full height of the
output, and a `top` or `bottom` panel spans the full width. The compositor decides that
length, so the length set in the layout does not apply.

**The compositor chooses the output.** Tauler does not pass a panel's `output` prop to the
compositor, so the compositor decides which output shows each panel.

**Tauler measures the first output.** `ctx.screen_width`, `ctx.screen_height` and the scale
come from the first output the compositor reports, whichever output shows the panel.

**Scale comes from the compositor.** Tauler divides the output's physical width by its
logical width. If the compositor gives no logical size, it uses the output's integer scale.
`Xft.dpi` plays no part.

**`<wallpaper>` is a background-layer surface.** It covers the whole output and never makes
room for, or moves out of the way of, other surfaces. Like a panel, it appears on the
compositor's choice of output: the `output` prop is not routed yet. The wallpaper's pixel
scale is rounded to a whole number, so a fractional output scale does not apply to it.

## macOS

See [macOS](/docs/macos/). It is experimental, and that page covers install, the one manual
measurement, and what to expect.

**Only the primary display exists.** Tauler places panels and wallpapers on the primary
display, and `ctx.output`, `ctx.screen_width` and `ctx.screen_height` describe it. An
`output` that does not match `ctx.output` removes the panel or wallpaper without an error.
There is no way to target a second display yet.

## Seeing the wallpaper through a panel

A panel gets the wallpaper behind it through `tauler:root-bg`, described in
[the layout file](/docs/layout-file/#seeing-through-a-panel). Only Tauler's own `<wallpaper>`
feeds it. A wallpaper set by `feh` or any other program is not visible to it, even on X11,
where that program paints the same root background Tauler would. A panel then has nothing
behind it to blur.
