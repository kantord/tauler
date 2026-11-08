---
title: The layout file
description: The shape of a layout file, the nodes it can contain, and the props each one takes.
---

Your layout is one file at `~/.config/tauler/layout.op.mdx`. It is an ES module, and its
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

Every file your layout loads, the layout file and everything it imports, can read where it
lives: `import.meta.dirname` (its directory), `import.meta.filename` (its absolute path)
and `import.meta.url` (the same path as a `file://` URL). They are plain strings and
open nothing. In a browser preview there is no file on disk: `dirname` and `filename`
are `undefined`, and `url` is the address the browser loaded the file from.
[Packages](/docs/packages/#shipping-a-program-with-a-package) use them to run a program
they ship.

Theme and font choice live in a YAML frontmatter block at the top of the file:

```jsx
---
theme:
  name: tokyonight-night
fonts:
  primary: JetBrains Mono
---
import DateTimeCard from './components/DateTimeCard.jsx';

export default function render() {
  return (
    <root>
      ...
    </root>
  );
}
```

Everything about *what* Tauler draws lives in the layout file's body, never its
frontmatter — the same distinction a `_layout.jsx` vs `config.yaml` split used to make
across two files now lives inside one.

A `layout.op.mdx` with no frontmatter block at all is valid — it just means the shipped
defaults for theme and fonts apply, the same as an absent `config.yaml` always meant. The
theme choices are on the [Themes](/docs/themes/) page.

Despite the extension, nothing here parses markdown into content — everything after the
closing `---` line is ordinary JSX, passed through exactly as written. A bare heading or
paragraph at flow position is a JS syntax error, not content: markdown syntax is only
safe inside a comment, where it's just text nobody renders. You can still *write* it in
markdown format there, for a person reading the file — headings, bold, lists all work as
markdown once you're reading the comment as markdown, Tauler just never converts it to
HTML:

```jsx
/*
# Sidebar

Workspace list on top, status cards below.
*/
```

**Legacy path:** `~/.config/tauler/layout.jsx` plus a sibling `~/.config/tauler/config.yaml`
still works, and is checked when `layout.op.mdx` doesn't exist. New setups should use
`layout.op.mdx`.

## Nodes

Layout nodes describe content and get rasterized. **They are HTML elements** — `div`,
`span`, `p`, `img` — and text is any bare value you write in the tree:

```jsx
<div class="flex flex-col px-3 py-2">
  <span class="text-[10px] text-muted-foreground">CPU</span>
  <span class="text-[14px] text-foreground">{load}%</span>
</div>
```

[Elements and styling](/docs/elements/) covers which tags exist, what each one's
default styling is, and how `class` and `style` apply.

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
| `output` | string | X11: RandR output name, e.g. `"DP-2"`; omit for the primary output. Wayland: not used; the compositor picks the output. macOS: Tauler sees only the primary display, and a name other than `ctx.output` removes the panel. See [Backends](/docs/backends/) |

`anchor` *places* a panel. Whether it also reserves space depends on the display server:

- **X11:** no. A window manager will happily tile other windows underneath. Reserving space
  is a separate decision; on i3, `<I3Layout>` makes it. See [Screen layout](/docs/layout/).
- **Wayland:** yes. The compositor keeps other windows clear of an anchored panel.
- **macOS:** no. Reserve space in AeroSpace's own configuration; see [macOS](/docs/macos/).

[Backends](/docs/backends/) has the details for each.

## `<wallpaper>` props

| prop | type | description |
|---|---|---|
| `id` | string | surface id; **required** — without it the whole root fails to parse |
| `output` | string | X11: RandR output name, e.g. `"DP-2"`; omit for the primary output. Wayland: not routed yet. macOS: only the primary display; any other name removes the wallpaper |

A wallpaper has no geometry props. It always covers its output exactly, and its subtree is
laid out against those dimensions. It reserves nothing and receives no clicks. On X11 it is
pixels painted into the root background; on macOS it is a window pinned below all others.

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

Wallpapers work on X11, macOS and Wayland. On Wayland the wallpaper is a surface on the
compositor's background layer, so the compositor must implement `wlr-layer-shell` (Sway,
Hyprland and river do). It lands on the compositor's choice of output — `output` is not
routed yet. A fractional pixel scale is exact when the compositor has `wp_viewporter`
(Sway, Hyprland and river do); otherwise it is rounded to a whole number. See
[Backends](/docs/backends/).

## Seeing through a panel

Each panel is rasterized into its own buffer, so there is nothing behind it to show
through — a translucent background paints onto nothing. To give it something, Tauler binds
the slice of wallpaper that panel covers as an image named `tauler:root-bg`, for the
duration of one render. The `tauler:` scheme marks it as a resource Tauler binds rather
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

The pixels come from Tauler's own `<wallpaper>` node, matched by output. A wallpaper set
by another program — `feh`, `xwallpaper` — is not visible here. A panel on an output with
no `<wallpaper>` gets no wallpaper behind it at all, rather than borrowing its neighbour's.
