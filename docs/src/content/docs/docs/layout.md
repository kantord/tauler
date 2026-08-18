---
title: Screen layout
description: Placing panels around a screen, and reserving the space they occupy.
---

A `<panel>` is a window at a position. Nothing about creating one tells the window
manager to keep other windows out of the way — that is a separate, and surprisingly
awkward, problem.

## Why reserving space is not automatic

The usual mechanism is `_NET_WM_STRUT_PARTIAL`: a window declares "I occupy 300px of the
left edge" and the window manager tiles around it. Tauler cannot use it, for two
independent reasons.

Panels are **override-redirect** windows. That tells X11 the window manager should not
manage them at all — no moving, no decorating, no reading their properties. Tauler needs
that, because it places panels itself, per-output and DPR-aware. But an unmanaged window's
struts are never read.

And i3 would not honour them anyway. It recognises only `W_DOCK_TOP` and `W_DOCK_BOTTOM`
(`include/data.h`), and classifies a dock by its top/bottom struts alone (`src/manage.c`).
There is no left or right dock. A full-height sidebar — the most common bar there is —
cannot reserve space this way even in principle.

So on i3, space is reserved by **telling i3 directly**, via its gaps. That is a decision
you make, not something tauler can infer from a panel's size.

## `<I3Layout>`

Writing the sizes twice — once as panel geometry, once as gaps — is easy to get wrong and
hard to notice, because a stale gap just leaves dead space or lets windows slide under the
bar. `<I3Layout>` derives one from the other:

```jsx
<I3Layout module="~/.cargo/bin/tauler-i3">
  <Panel id="sidebar"   anchor="left"   size={272}>…</Panel>
  <Panel id="topbar"    anchor="top"    size={26}>…</Panel>
  <Panel id="bottombar" anchor="bottom" size={26}>…</Panel>
</I3Layout>
```

Each `<Panel>` takes `size` pixels off one edge of whatever rectangle is left, then the
next panel sees the smaller one. The four running totals become the gaps handed to i3.
There is no second place to keep in sync.

### Order is the API

Declaration order decides who owns each corner:

```jsx
<Panel id="sidebar" anchor="left" size={300}>…</Panel>
<Panel id="topbar"  anchor="top"  size={50}>…</Panel>
```

Here the sidebar is full height, and the top bar starts 300px in — it spans the space
*beside* the sidebar. Swap the two lines and the top bar spans the full width instead,
with the sidebar starting 50px down. Neither is more correct; pick the corner you want.

Panels can stack on the same edge. A second `anchor="left"` sits beside the first, and
below any top bar declared before it — which is how you get a sidebar that starts under a
bar rather than beside it.

### Props

| prop | description |
|---|---|
| `id` | surface id, as on `<panel>` |
| `anchor` | `"left"`, `"right"`, `"top"` or `"bottom"`. Anything else reserves nothing |
| `size` | thickness along the anchored axis, in logical pixels. The other axis fills what is left |
| `output` | RandR output name, e.g. `"DP-2"`. Omit for the primary output |

`<I3Layout>` itself takes one prop, `module`: the binary to send the computed gaps to,
normally `tauler-i3`. Omit it and `<I3Layout>` becomes pure geometry, registering nothing —
useful on a compositor that reserves space by other means.

## Doing it by hand

`<I3Layout>` is a convenience over `tauler-i3`'s `gaps` prop, which remains available:

```jsx
<Module bin="~/.cargo/bin/tauler-i3"
        gaps={{ left: 272, right: 60, top: 26, bottom: 26 }}>
```

Reach for this when your arrangement is not a stack of edges — a gap that does not
correspond to any panel, say. An omitted side reserves nothing.

## A note on units

Every length here is a **logical** pixel, the same unit as `width` and `height` on a
`<panel>`, and the values reach i3 unconverted.

That is not an oversight. i3's `cmd_gaps` begins with `logical_px(atoi(value))`, and
`logical_px` is `ceil(dpi/96 × value)` above a 1.25 DPI threshold and the identity below
(`libi3/dpi.c`). i3's gap unit *is* the logical pixel, so scaling on the way in would only
have to be undone on the way out — with different rounding. There is no physical-pixel
path: i3's grammar accepts a trailing `px` keyword, but it is discarded, and `cmd_gaps`
has no unit parameter.
