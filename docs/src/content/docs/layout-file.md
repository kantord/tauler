---
title: The layout file
description: The shape of a layout file, the nodes it can contain, and the props each one takes.
---

A bar is one `.jsx` file at `~/.config/tauler/layout.jsx`. It is an ES module, and its
**default export is the render function**:

```jsx
export default function render() {
  return (
    <root>
      <panel anchor="left" width={272} height={ctx.screen_height}>
        <container tw="flex flex-col h-full w-full px-4 py-4">
          <text tw="text-[18px] text-foreground">hello</text>
        </container>
      </panel>
    </root>
  );
}
```

A file that ends in a bare `<root>` expression instead of exporting a function will not
load — it fails with a type error about `undefined` not being a function.

The file is watched and hot-reloaded. On reload every subprocess is restarted and all
stream values are cleared, so a reload is a cold start, not a refresh.

Components are plain JS functions that take props and return a tree. There is nothing to
register: JSX handles `<Card />` as a function call already.

Theme mode and font choice live separately, in `~/.config/tauler/config.yaml`. Everything
about *what* a bar contains lives in the layout file.

## Nodes

Layout nodes describe content and get rasterized:

| node | description |
|---|---|
| `container` | flex container |
| `text` | text |
| `image` | image |

Shell nodes describe structure and never reach the rasterizer:

| node | description |
|---|---|
| `root` | mandatory top-level node; contains `panel` and `wallpaper` nodes |
| `panel` | one desktop surface — an X11 window or a Wayland layer surface |
| `wallpaper` | paints its subtree into the desktop background of one output |

Both surface kinds parse into the same shape; only the destination of the finished pixels
differs. `<surface type="panel">` and `<surface type="wallpaper">` are accepted long-hand
spellings. A bare `<surface>` names no kind and is a parse error.

## `<panel>` props

| prop | type | description |
|---|---|---|
| `anchor` | `"left" \| "right" \| "top" \| "bottom"` | stick to this screen edge; omit for a free-floating panel |
| `width` | number | width in logical pixels |
| `height` | number | height in logical pixels |
| `x` | number | x position, ignored when `anchor` is set |
| `y` | number | y position, ignored when `anchor` is set |
| `above` | boolean | stack above other windows, for overlays like notifications |
| `output` | string | RandR output name, e.g. `"DP-2"`; omit for the primary output |
| `outer_gap` | number | gap reserved around screen edges |

`anchor` *places* a panel. It does not reserve space for it, and a window manager will
happily tile other windows underneath. Reserving space is a separate decision — see
[Screen layout](/tauler/layout/).

## `<wallpaper>` props

| prop | type | description |
|---|---|---|
| `id` | string | surface id; **required** — without it the whole root fails to parse |
| `output` | string | RandR output name, e.g. `"DP-2"`; omit for the primary output |

A wallpaper has no geometry props. It always covers its output exactly, and its subtree is
laid out against those dimensions. It has no window, reserves nothing, and receives no
clicks — there are only pixels handed to the desktop background.

```jsx
<root>
  <wallpaper id="desktop" output="DP-2">
    <container tw="flex w-full h-full items-end justify-end p-12"
               style={{ backgroundImage: "linear-gradient(160deg, #0b1020, #1c2b4a)" }}>
      <text tw="text-[28px] text-white opacity-30">{time}</text>
    </container>
  </wallpaper>
</root>
```

Everything a wallpaper does is ordinary layout. Scaling or cropping a photo is `<image>`
plus `object-fit`; a solid colour or gradient is a `<container>` with a background. There
is no wallpaper-specific fitting, tiling or colour handling, and none is planned.

**An `<image>` file must be a PNG.** The renderer is built with only PNG and ICO decoding
enabled, so a JPEG decodes to nothing — and a file that cannot be decoded is
indistinguishable from one that is not there, so the symptom is a surface that renders
empty with nothing in the log.

Wallpapers are an X11 feature for now. On Wayland and macOS the node is ignored with a
warning.

## Seeing through a panel

Each panel is rasterized into its own buffer, so there is nothing behind it to show
through — a translucent background paints onto nothing. To give it something, tauler binds
the slice of wallpaper that panel covers as an image named `root-bg`, for the duration of
one render:

```jsx
<panel id="sidebar" anchor="left" width={272} height={ctx.screen_height}>
  <container style={{ position: "relative", width: "100%", height: "100%" }}>
    <image src="root-bg" style={{ position: "absolute", top: 0, left: 0,
                                  width: "100%", height: "100%" }} />
    <container tw="h-full w-full p-2" style={{ position: "relative" }}>
      <container tw="h-full w-full rounded-2xl"
                 style={{ backgroundColor: "rgba(20,20,24,0.55)" }}>
        …
      </container>
    </container>
  </container>
</panel>
```

Two things about that snippet are load-bearing:

**Use an `<image>` node, not `backgroundImage: url(root-bg)`.** Both work, but the
background-image path redoes per-pixel setup that does not depend on the pixel, and a
full-height panel costs around 19ms per render against a ~6ms floor. The `<image>` node
hoists that work and costs about 5ms.

**Keep the overlaying content `position: relative`, not `absolute`.** One out-of-flow
sibling still paints above the image and avoids a family of layout bugs that appear with
several absolutely-positioned siblings.

The pixels come from tauler's own `<wallpaper>` node, matched by output. A wallpaper set
by another program — `feh`, `xwallpaper` — is not visible here. A panel on an output with
no `<wallpaper>` gets no `root-bg` at all, rather than borrowing its neighbour's.
