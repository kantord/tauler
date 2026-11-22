---
title: Built-in names
description: The names Tauler answers itself, and how a layout reads them.
---

A name that starts with `tauler:` is answered by Tauler itself. It is never a file to
read or a program to run, so no file on disk can shadow it.

| Name | Kind | What it gives you |
|---|---|---|
| `tauler:outputs` | stream | The connected screens, as a JSON array |
| `tauler:root-bg` | image | The slice of wallpaper behind a panel |

## `tauler:outputs`

Pass it to `useJSONStream` in place of a program. There is no second argument. The value
is an array with one object per connected screen, and a new one arrives whenever the screens
change. The stream reads RandR, so it exists on X11 only.

| field | description |
|---|---|
| `name` | the name RandR gives the screen, such as `DP-2` |
| `x`, `y` | the screen's position, in physical pixels |
| `width`, `height` | the screen's size, in physical pixels |
| `screen_width`, `screen_height` | the screen's size, in logical pixels |
| `dpr` | device pixels per logical pixel |

The call returns `null` until the first value arrives, so fall back to an empty array. This
layout draws one panel on each connected screen:

```jsx
const outputs = useJSONStream("tauler:outputs") ?? [];

<root>
  {outputs.map((o) => (
    <panel id={`bar-${o.name}`} output={o.name} anchor="top" height={32}>
      <div class="flex h-full items-center px-2">{o.name}</div>
    </panel>
  ))}
</root>
```

To read the primary screen only, use `ctx.screen_width` and `ctx.screen_height` from
[`ctx`](/docs/data/#ctx).

## `tauler:root-bg`

An `<img>` source. It shows the slice of wallpaper behind the panel, which is how a panel
looks translucent. See [Seeing through a panel](/docs/layout-file/#seeing-through-a-panel).
