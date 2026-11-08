# tauler-screenshot

Renders a [tauler](https://crates.io/crates/tauler) JSX layout to a vector SVG, using the
same evaluation, theming and layout pipeline as the bar itself.

Because it shares the pipeline rather than reimplementing it, the output is what the bar
would actually draw — which makes it useful for documentation screenshots, visual
regression tests, and iterating on a layout without restarting the bar.

## Install

```sh
cargo install tauler-screenshot
```

## Use

```sh
tauler-screenshot --input card.jsx --output card.svg --theme dark --width 400
```

| flag | default | meaning |
|---|---|---|
| `--input` | — | path to the JSX source; required unless `--swatch` is given |
| `--output` | required | path to write the SVG to |
| `--theme` | `dark` | `dark` or `light`: the default theme of that mode, or the mode of a named theme |
| `--theme-name` | — | a built-in theme by name, such as `tokyonight-night` |
| `--theme-file` | — | a theme YAML file; wins over `--theme-name` and must define every core and ANSI token |
| `--width` | `400` | render width in CSS pixels, including the 16 px margins |
| `--font-path` | — | TTF/OTF file to use as the primary sans-serif font |
| `--symbol-font-path` | — | symbol font file for `<Icon>` glyphs; without it fontconfig picks one |
| `--files-only` | off | register only `--font-path` and `--symbol-font-path`, ignoring the host's fonts |
| `--swatch` | off | render a swatch of every theme token, pair and ANSI colour instead of `--input` |
| `--classes-out` | — | also write every Tailwind class in the rendered tree, one per line |
| `--geometry-out` | — | also write every painted node's box as JSON |

The input is plain JSX with a default-exported render function, not a `layout.op.mdx`
file with frontmatter.

The layout is rendered onto a padded canvas whose height follows the content, so the
document is sized to the component plus a 16 px margin rather than to a fixed canvas.
Text comes out as glyph-outline paths, so the SVG looks the same on every viewer. The
render itself uses the host's fonts unless `--font-path` and `--files-only` pin them.

Streams are not run: `useStringStream` and `useJSONStream` resolve to empty values, so a
layout that reads live data renders in its empty state. Pass data as props instead when
you want a populated screenshot.

## Use as a library

The same pipeline is one function call:

```rust
let shot = tauler_screenshot::render(
    r#"export default () => <span class="text-foreground">Hello, world</span>;"#,
    &tauler_screenshot::Options::default(),
)?;
std::fs::write("hello.svg", &shot.svg)?;
```

`Options` carries what the flags above set (theme, width, fonts, `files_only` and the
palette); `Screenshot` also exposes the Tailwind class harvest (`classes`) and the
painted boxes (`geometry()`).

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
