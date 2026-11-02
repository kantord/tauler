---
title: Elements and styling
description: The HTML elements a layout file can contain, how text works, and how class and style apply to them.
---

A layout file's content is **HTML elements**. The tag you write is the tag you know, and it
behaves the way that tag behaves — a `<div>` stacks its children, a `<p>` has margins, an
`<h1>` is large and bold.

```jsx
<div class="flex flex-col gap-2 px-3 py-2">
  <h2 class="text-[10px] text-muted-foreground">DISK</h2>
  <p class="text-[13px] text-foreground">
    root <span class="font-bold">{used}%</span> of {total}
  </p>
</div>
```

## Text has no element

There is no tag that means "text". **Writing a bare value in the tree is what makes text**,
and it is the only thing that does:

```jsx
<span class="text-[12px]">{time}</span>
<span class="text-[12px]">battery {level}%</span>
```

Text takes its styling from the element around it, exactly as in HTML. To style part of a
sentence, wrap that part:

```jsx
<p class="text-[11px] text-muted-foreground">
  {count} open <span class="text-foreground font-bold">PRs</span>
</p>
```

Numbers, like strings, become text. `false`, `null` and `undefined` render nothing, so the
usual `{cond && <div/>}` idiom works.

## Which tags exist

Every HTML tag is accepted. What differs between them is the **preset** — the styling a tag
carries from its name alone:

| tags | what the preset gives them |
|---|---|
| `div`, `section`, `nav`, `article`, `header`, `footer`, `main` | `display: block` |
| `p`, `blockquote`, `figure` | block, plus vertical margins |
| `h1`–`h6` | block, bold, sized by level |
| `ul`, `ol`, `li`, `dl`, `dd` | block, with list indentation |
| `hr` | block, a 1px border, auto side margins |
| `b`, `strong` | bold |
| `i`, `em`, `cite` | italic |
| `code`, `kbd`, `samp`, `pre` | monospace |
| `small`, `big`, `sub`, `sup` | relative sizing |
| `s`, `del` / `u`, `ins` | strike-through / underline |
| `span`, and any tag with no preset | `display: inline` |
| `img` | image — `src` is **required** |
| `br` | a line break |

The presets are Chromium's, so anything you know about default HTML rendering holds. A tag
nothing has ever heard of is inline, which is also what a browser does.

`<style>`, `<script>`, `<head>`, `<meta>` and `<link>` are dropped along with their
contents — a `<style>` body would otherwise render as visible text.

### Not supported

- **Inline `<svg>`** is a parse error. To draw from data, use a
  [`<D3Figure>`](/docs/components/#d3figure). A fixed SVG goes in a `data:` URI instead:
  `<img src="data:image/svg+xml,<svg …>" />`
- **`display: table`** does not exist, so `<table>`, `<tr>` and `<td>` lay out as plain
  blocks. Use flex or grid for columns.
- **Nesting deeper than 32 elements** is a parse error rather than a crash. No real bar
  comes close.

## Styling: `class` and `style`

**`class` carries Tailwind utilities**, and doubles as the place theme tokens are written:

```jsx
<div class="flex flex-row items-center gap-2 rounded-lg bg-card px-3 py-2">
```

Utilities neither tauler's theme layer nor the renderer recognizes pass straight through,
so an unknown class is inert rather than an error. Theme tokens — `bg-card`,
`text-muted-foreground`, `border-border`, `rounded-lg` — are substituted for the values in
the active theme before rendering. See [Themes](/docs/themes/) for the names and for how
to choose or write a theme.

**`style` takes an object, not a CSS string.** That is what lets a value be computed per
tick:

```jsx
<div
  class="rounded-md px-2 py-1"
  style={{ backgroundColor: load > 0.9 ? "#f38ba8" : "transparent" }}
/>
```

Property names are camelCase (`backgroundColor`, `maxWidth`), and a bare number means
logical pixels. `style` wins over `class`, and `class` wins over the tag's preset — the
same order CSS uses.

## Fonts

`fonts.primary` and `fonts.emoji` in the layout file's frontmatter (or, on the legacy path,
`config.yaml`) fill fixed roles — the sans-serif default, and the fallback used for emoji
glyphs. `fonts.extra` registers further fonts with no assigned role, each usable by name
from wherever you want it in the layout file:

```yaml
fonts:
  primary: "Inter Variable"
  emoji: "Noto Color Emoji"
  extra:
    - "Lora"
    - "JetBrains Mono"
    - path: "~/.fonts/MyIconFont.ttf"
```

An entry is either a family name — resolved through fontconfig the same way `primary`
is — or a `path:` to one exact file, for a font that isn't installed system-wide.

Reach an extra font with a Tailwind-style arbitrary class, `font-[Name]`. Spaces in a
multi-word family become underscores:

```jsx
<span class="font-[Lora]">Heading</span>
<span class="font-[JetBrains_Mono]">12:45</span>
```

This is takumi's own arbitrary-class parsing, the same mechanism behind `text-[10px]`;
tauler's part is only registering the font so the class has something to resolve to.

### Font roles

A theme can also assign fonts to **roles** — the same idea as `bg-card` or `rounded-lg`
in [Styling: `class` and `style`](#styling-class-and-style), just for fonts. A theme's
`fonts:` map names a role and points it at a font. A layout file reaches that
role with `font-<role>`:

```yaml
# in a theme file
fonts:
  heading: "Lora"
```

```jsx
<h2 class="font-heading">DISK</h2>
```

`font-heading` resolves to `font-[Lora]` at render time — but only because you
registered `"Lora"` above, via `fonts.primary` or `fonts.extra`. A role the active
theme doesn't define passes through unchanged. This includes `font-sans`, `font-serif`,
and `font-mono`. A theme may claim those keys to override the generic fonts. If it
doesn't, they still resolve through takumi's built-in font handling, exactly as before
— with no special-casing.

### Measure text: `measureText`

`measureText(text, { family, size, weight, style })` returns how wide tauler draws `text`,
in logical pixels. Use it to size a margin to its widest label, to drop labels that would
overlap, or to shorten a label that does not fit:

```jsx
const font = { family: "Inter, sans-serif", size: 11 };
const margin = Math.max(...labels.map((label) => measureText(label, font).width)) + 4;
```

| option | type | default | description |
|---|---|---|---|
| `family` | string | required | a CSS `font-family` list, such as `"JetBrains Mono, monospace"` |
| `size` | number | required | the font size in logical pixels, greater than 0 |
| `weight` | number | `400` | from 1 to 1000 |
| `style` | string | `"normal"` | `"normal"`, `"italic"` or `"oblique"` |

It returns `{ width, ascent, descent }`. `ascent` and `descent` are how far the family's
first font reaches above and below the baseline. All three are rounded to 1/64 of a pixel.

The text is measured as one line, exactly as written: spaces are kept, and each tab or
line-break character counts as a space.

`family` is resolved the way a `fontFamily` style is, so the width is the width of what
appears. A family that is not installed is measured in the font tauler falls back to, so
it does not fail. An empty `family`, a `family` that is not a CSS family list (such as
`"a;b"`), a `size` that is not a positive number, or a `text` that is not a string throws a
`TypeError`.

`family` is a CSS family list, so [font roles](#font-roles) do not apply to it. To measure
text set in `font-heading`, pass the family the role points at, `"Lora"` in the example
above, not `"heading"`.

The same call gives the same answer for as long as the fonts stay the same, everywhere in
the layout file, including in a [D3Figure's](/docs/components/#d3figure) `draw`. It
depends on the fonts installed, so another machine can give other widths. In a browser,
`measureText` measures with the browser's font for `family`, which may not be the font your
desktop uses.

## Clicks

`on_click` goes on the element you want clickable, and only fires on elements that have a
box of their own — a `<div>`, or anything that is a flex or grid item. A `<span>` inside a
run of text has no box, so a handler there never fires; the first click logs a warning
naming the element. See [Data and interaction](/docs/data/#on_click).

## Coming from `container` / `text` / `tw`

Earlier versions had three node types and a `tw` attribute. The mapping is mechanical:

| before | now |
|---|---|
| `<container tw="…">` | `<div class="…">` |
| `<text tw="…">x</text>` | `<span class="…">x</span>` |
| `<image src="…" />` | `<img src="…" />` |
| `src="root-bg"` | `src="tauler:root-bg"` |

One thing a rename cannot fix: `<container>` defaulted to `display: inline`, while `<div>`
is `display: block`. Any element you relied on being inline without saying so will lay out
differently, so re-read a converted file rather than trusting the substitution.
