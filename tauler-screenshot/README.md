# tauler-screenshot

Renders a [tauler](https://crates.io/crates/tauler) JSX layout to a PNG, using the same
evaluation, theming and rasterisation pipeline as the bar itself.

Because it shares the pipeline rather than reimplementing it, the output is what the bar
would actually draw — which makes it useful for documentation screenshots, visual
regression tests, and iterating on a layout without restarting the bar.

## Install

```sh
cargo install tauler-screenshot
```

## Use

```sh
tauler-screenshot --input card.jsx --output card.png --theme dark --width 400
```

| flag | default | meaning |
|---|---|---|
| `--input` | required | path to the JSX source |
| `--output` | required | path to write the PNG to |
| `--theme` | `dark` | `dark` or `light` |
| `--width` | `400` | render width in CSS pixels, including the 16 px margins |
| `--font-path` | — | TTF/OTF file to use as the primary sans-serif font |

The layout is rendered onto a padded canvas and then cropped to the component's measured
bounds plus a 16 px margin, so the image is sized to the content rather than to the
canvas.

Streams are not run: `useStringStream` and `useJSONStream` resolve to empty values, so a
layout that reads live data renders in its empty state. Pass data as props instead when
you want a populated screenshot.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
