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
        <div class="flex flex-col h-full w-full px-4 py-4">
          <span class="text-[18px] text-foreground">hello</span>
        </div>
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

Layout nodes describe content and get rasterized. **They are HTML elements** — the tag
you write is the tag you know, and it behaves the way that tag behaves:

| node | description |
|---|---|
| `div`, `section`, `nav`, … | block elements |
| `span`, `em`, `strong`, `code`, … | inline elements |
| `p`, `h1`–`h6`, `ul`, `ol`, `li`, `hr` | with their usual margins and sizes |
| `img` | image; `src` is required |
| `br` | line break |

There is no element that means "text". **Text is any bare string in the tree**, and style
it by styling the element around it:

```jsx
<p class="text-[13px] text-foreground">
  cpu <span class="font-bold">{cpu}%</span>
</p>
```

Tags carry their browser default styles, so a `<div>` stacks its children, a `<p>` has
margins, and an `<h1>` is large and bold. A tag with no defaults — `<span>`, or any tag
nothing has heard of — is inline. `<style>`, `<script>`, `<head>`, `<meta>` and `<link>`
are dropped along with their contents.

Not supported: inline `<svg>` (put the SVG in a `data:` URI on an `<img src>`), and
anything needing `display: table` — `<table>` and friends lay out as plain blocks.

Tailwind utilities go in `class`. `style` takes an **object**, not a CSS string, so a
value can be computed:

```jsx
<div class="rounded-md px-2" style={{ backgroundColor: hot ? "#f38ba8" : "transparent" }}>
```

Shell nodes describe structure and never reach the rasterizer. They are the only
lowercase tags that are not HTML:

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
    <div class="flex w-full h-full items-end justify-end p-12"
         style={{ backgroundImage: "linear-gradient(160deg, #0b1020, #1c2b4a)" }}>
      <span class="text-[28px] text-white opacity-30">{time}</span>
    </div>
  </wallpaper>
</root>
```

Everything a wallpaper does is ordinary layout. Scaling or cropping a photo is `<img>`
plus `object-fit`; a solid colour or gradient is a `<div>` with a background. There
is no wallpaper-specific fitting, tiling or colour handling, and none is planned.

**An `<img>` file must be a PNG.** The renderer is built with only PNG and ICO decoding
enabled, so a JPEG decodes to nothing — and a file that cannot be decoded is
indistinguishable from one that is not there, so the symptom is a surface that renders
empty with nothing in the log.

Wallpapers are an X11 feature for now. On Wayland and macOS the node is ignored with a
warning.

## Seeing through a panel

Each panel is rasterized into its own buffer, so there is nothing behind it to show
through — a translucent background paints onto nothing. To give it something, tauler binds
the slice of wallpaper that panel covers as an image named `tauler:root-bg`, for the
duration of one render. The `tauler:` scheme marks it as a resource tauler binds rather
than a file to read, so no file on disk can shadow it:

```jsx
<panel id="sidebar" anchor="left" width={272} height={ctx.screen_height}>
  <div style={{ position: "relative", width: "100%", height: "100%" }}>
    <img src="tauler:root-bg" style={{ position: "absolute", top: 0, left: 0,
                                       width: "100%", height: "100%" }} />
    <div class="h-full w-full p-2" style={{ position: "relative" }}>
      <div class="h-full w-full rounded-2xl"
           style={{ backgroundColor: "rgba(20,20,24,0.55)" }}>
        …
      </div>
    </div>
  </div>
</panel>
```

Two things about that snippet are load-bearing:

**Use an `<img>` node, not `backgroundImage: url(tauler:root-bg)`.** Both work, but the
background-image path redoes per-pixel setup that does not depend on the pixel, and a
full-height panel costs around 19ms per render against a ~6ms floor. The `<img>` node
hoists that work and costs about 5ms.

**Keep the overlaying content `position: relative`, not `absolute`.** One out-of-flow
sibling still paints above the image and avoids a family of layout bugs that appear with
several absolutely-positioned siblings.

The pixels come from tauler's own `<wallpaper>` node, matched by output. A wallpaper set
by another program — `feh`, `xwallpaper` — is not visible here. A panel on an output with
no `<wallpaper>` gets no backdrop at all, rather than borrowing its neighbour's.
