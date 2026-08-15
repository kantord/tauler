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

- **Inline `<svg>`** is a parse error. Put the SVG in a `data:` URI instead:
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
your `config.yaml` before rendering. See [Screen layout](/layout/) for where those
come from.

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

## Clicks

`on_click` goes on the element you want clickable, and only fires on elements that have a
box of their own — a `<div>`, or anything that is a flex or grid item. A `<span>` inside a
run of text has no box, so a handler there never fires; the first click logs a warning
naming the element. See [Data and interaction](/data/#on_click).

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
