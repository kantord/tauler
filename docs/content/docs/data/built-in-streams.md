---
title: Streams Tauler provides
description: Read the connected screens without running a program.
template: article
---

Some values come from Tauler itself, so there is no program to name. A stream whose name
starts with `tauler:` is a **built-in stream**. There is one, `tauler:outputs`, the
connected screens.

```jsx
export default function render() {
  const outputs = useJSONStream("tauler:outputs") ?? [];
  return (
    <root>
      {outputs.map((o) => (
        <panel id={`bar-${o.name}`} output={o.name} anchor="top"
               width={o.screen_width} height={32}>
          <div class="flex h-full items-center px-2">{o.name}</div>
        </panel>
      ))}
    </root>
  );
}
```

## How it works

A name that starts with `tauler:` is answered by Tauler. It is never a file to read or a
program to run, so no file on disk can take its place. A built-in stream is read with
`useJSONStream` like any other stream and follows the same rules: one value, the latest,
and a new run of the layout when it changes. `tauler:outputs` reads the screens from the
X server through RandR and gives a new value whenever a screen is connected, removed or
resized.

## Common tasks

### Draw one panel on each screen

Map over the array, and give each panel an `id` and an `output` from the screen's name,
as the example above does.

```jsx
{outputs.map((o) => (
  <panel id={`bar-${o.name}`} output={o.name} anchor="top"
         width={o.screen_width} height={32}>…</panel>
))}
```

### Read only the primary screen

Use `ctx`, which needs no stream:

```jsx
<panel id="bar" anchor="top" width={ctx.screen_width} height={32}>…</panel>
```

### Show a picture of the wallpaper behind a panel

`tauler:root-bg` is the other name with this prefix. It is an image, not a stream. See
[Seeing through a panel](/docs/layout-file/#seeing-through-a-panel):

```jsx
<img src="tauler:root-bg" style={{ position: "absolute", top: 0, left: 0,
                                   width: "100%", height: "100%" }} />
```

## Rules

**`tauler:outputs` works on X11 only.** It asks an X server. On macOS there is none, so
the stream never has a value and the example above draws nothing. On Wayland it has a
value only when Xwayland is running, and then the names are the ones Xwayland reports,
which need not be the names the compositor uses for `output`.

**Fall back to an empty array.** The value is `null` until the first one arrives.

**Pass no script.** A built-in stream takes one argument, its name.

**Give each panel its own `id`, and a `width` and a `height`.** A panel that lacks one
of them is an error for the whole layout. `anchor` supplies no size.

## Reference

### `useJSONStream("tauler:outputs")`

Returns an array with one object for each connected screen:

| field | description |
|---|---|
| `name` | the name RandR gives the screen, such as `DP-2` |
| `x`, `y` | the screen's position, in physical pixels |
| `width`, `height` | the screen's size, in physical pixels |
| `screen_width`, `screen_height` | the screen's size, in logical pixels |
| `dpr` | device pixels per logical pixel |

### Names that start with `tauler:`

| Name | Kind | What it gives you |
|---|---|---|
| `tauler:outputs` | stream | The connected screens, as a JSON array |
| `tauler:root-bg` | image | The slice of wallpaper behind a panel. See [Seeing through a panel](/docs/layout-file/#seeing-through-a-panel) |

### Behaviour

- **Before the first value.** `null`.
- **When it updates.** When a screen is connected, removed, moved or resized.
- **On errors.** With no X server to ask, the stream has no value and stays `null`.
  Tauler logs the reason.
- **On a reload.** Saving the layout file clears the value. A new one arrives at once.

## Related

- [Read a value from an external program](/docs/data/streams/): the rules every stream
  follows.
- [The layout file](/docs/layout-file/#ctx): `ctx.screen_width` and `ctx.screen_height`
  for the primary screen.
- [Backends](/docs/backends/): how `output` names a screen on each display server.
