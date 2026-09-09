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
| `--input` | required | path to the JSX source |
| `--output` | — | path to write the SVG to |
| `--html-out` | — | path to write the layout's `<dom>` surface as markup (see below) |
| `--stream-values` | — | JSON file of stream values (see below) |
| `--screen` | `1920x1080` | what `ctx.screen_width` and `ctx.screen_height` report |
| `--theme` | `dark` | `dark` or `light` |
| `--width` | `400` | render width in CSS pixels, including the 16 px margins |
| `--font-path` | — | TTF/OTF file to use as the primary sans-serif font |

The layout is rendered onto a padded canvas whose height follows the content, so the
document is sized to the component plus a 16 px margin rather than to a fixed canvas.
Text comes out as glyph-outline paths, so the SVG looks the same everywhere regardless
of which fonts a viewer has installed.

Streams are not run. `useStringStream` and `useJSONStream` resolve to the values in
`--stream-values`, a JSON array of `{"bin", "script", "line"}` objects keyed the way the
layout declares each stream (`script` may be omitted, and a `<Module>`'s data is the
`bin` alone), and to empty values for streams not listed. So a layout that reads live
data renders in a state you choose:

```json
[{ "bin": "/bin/sh", "script": "date +%H:%M", "line": "09:41" }]
```

A layout that declares a `<dom>` surface next to its `<panel>`s can also be written out
as markup with `--html-out`: the same tree the SVG paints, walked the way the browser
runtime walks it. Either output may be omitted; the docs site uses `--html-out` alone
to paste Tauler's own render of a file into a page.

## Use as a library

The same pipeline is one function call:

```rust
let shot = tauler_screenshot::render(
    r#"export default () => <span class="text-foreground">Hello, world</span>;"#,
    &tauler_screenshot::Options::default(),
)?;
std::fs::write("hello.svg", &shot.svg)?;
```

`Options` carries the theme, width and font path the flags above set; `Screenshot` also
exposes the Tailwind class harvest (`classes`) and the painted boxes (`geometry()`).

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
