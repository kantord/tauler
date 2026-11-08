---
title: Screenshots
description: Rendering a layout to an SVG with tauler-screenshot, for docs, tests and quick iteration.
---

`tauler-screenshot` renders a layout to an SVG file without starting the bar. It uses the
same evaluation, theming and layout steps as the bar, so the picture is what the bar draws.
Use it to produce images for documentation, to compare a layout before and after a change,
and to try a layout without restarting Tauler.

## Install it

```sh
cargo install tauler-screenshot
```

## Render a layout

```sh
tauler-screenshot --input card.jsx --output card.svg --theme light --width 300
```

`--input` is a JavaScript module whose default export returns the layout:

```jsx
export default () => (
  <div class="flex flex-col gap-2 rounded-lg bg-card p-4 text-card-foreground">
    <span class="font-bold">Battery</span>
    <span class="text-muted-foreground">82%</span>
  </div>
);
```

The tool prints `wrote card.svg (<n> bytes)` to stderr when it finishes.

The input is plain JSX. A layout file with frontmatter, such as `layout.op.mdx`, is not
accepted: it fails with `evaluating the layout`. Move the part you want to render into a
plain `.jsx` file.

## Know what the output is

- **Vector.** The SVG is sized to the content. Its height follows the layout, and the width
  is `--width`.
- **Text is paths.** Text is drawn as glyph outlines, not `<text>` elements, so a viewer
  does not need the font installed.
- **Padded.** The layout sits on a canvas with a 16 px margin on every side. `--width`
  includes both margins.
- **Streams are empty.** Streams do not run. `useStringStream` and `useJSONStream` return
  empty values, so a layout that reads [live data](/docs/data/) renders its empty state. A
  screenshot cannot show real data. To show populated values, render a layout that takes
  them as props or constants.

## Options

| Flag                  | Default           | Meaning                                                                                  |
| --------------------- | ----------------- | ---------------------------------------------------------------------------------------- |
| `--input`             | none              | Path to the JSX source. Required unless `--swatch` is given.                             |
| `--output`            | none, required    | Path to write the SVG to.                                                                |
| `--theme`             | `dark`            | `dark` or `light`. Selects the default theme of that mode.                               |
| `--theme-name`        | the default theme | A [built-in theme](/docs/themes/) by name, such as `tokyonight-night`.                   |
| `--theme-file`        | none              | A theme YAML file. Wins over `--theme-name`. It must define every core and ANSI token.   |
| `--width`             | `400`             | Document width in CSS pixels, including the 16 px margins.                               |
| `--font-path`         | none              | A TTF or OTF file to use as the primary sans-serif font.                                 |
| `--symbol-font-path`  | none              | The symbol font file used for `<Icon>` glyphs. Without it, fontconfig picks one.         |
| `--files-only`        | off               | Register only `--font-path` and `--symbol-font-path`; ignore the fonts on the machine.   |
| `--swatch`            | off               | Render a swatch scene for the theme instead of `--input`.                                |
| `--classes-out`       | none              | Also write every Tailwind class in the rendered tree, one per line.                      |
| `--geometry-out`      | none              | Also write the box of every painted node as JSON.                                        |

An unknown `--theme` value is rejected with `expected dark or light`. An unknown
`--theme-name` stops the tool and lists the built-in names.

## Pick a theme

`--theme` chooses between the default dark and default light themes. `--theme-name` chooses
any built-in theme. When you give both, `--theme` overrides the mode the named theme
carries.

```sh
tauler-screenshot --input card.jsx --output card.svg --theme-name catppuccin-mocha
```

With neither flag, the render uses `default-dark`.

### Render a swatch

`--swatch` replaces `--input` with a generated scene that shows every theme token, every
foreground and background pair, the ANSI colours and a few components. Use it to check a
theme you are writing:

```sh
tauler-screenshot --swatch --theme-file my-theme.yaml --output swatch.svg
```

## Make the output reproducible

Without font flags, the tool takes fonts from the machine through fontconfig. Two machines
with different fonts produce different SVGs from the same input. To get the same SVG on
every machine, pin the fonts and turn the lookup off:

```sh
tauler-screenshot --input card.jsx --output card.svg \
  --font-path ./fonts/Inter.ttf \
  --symbol-font-path ./fonts/symbols.ttf \
  --files-only
```

With `--files-only` and no `--font-path`, there is no font to draw text with, and the
output contains no text.

## Use it as a library

The `tauler-screenshot` crate exposes the same render as one function:

```rust
let shot = tauler_screenshot::render(
    r#"export default () => <span class="text-foreground">Hello, world</span>;"#,
    &tauler_screenshot::Options::default(),
)?;
std::fs::write("hello.svg", &shot.svg)?;
```

`Options` has one field for each rendering flag above: `theme` (a `ThemeMode`), `width`,
`font_path`, `symbol_font_path`, `files_only` and `palette` (a theme, the counterpart of
`--theme-name` and `--theme-file`). `Options::default()` is the dark theme at width 400
with fonts from the machine. `render` returns an `Error` when the source does not parse or
evaluate.

The returned `Screenshot` holds:

- `svg`, the document as a string.
- `classes`, the set of Tailwind classes in the rendered tree.
- `geometry()`, the box of every painted node in CSS pixels. Each box has a `path` of
  dot-joined child indices (`"0.1"`), and `x`, `y`, `width` and `height`.

The font settings are shared by the whole process. A later `render` call with a different
`font_path` replaces the fonts of the earlier one.

## Put screenshots in docs

Render each example layout in a script and commit the SVGs, or generate them in your docs
build. Pin the fonts with `--font-path`, `--symbol-font-path` and `--files-only` so a
regenerated image does not change when the machine does. Render once per theme by running
the tool with `--theme light` and `--theme dark`.
