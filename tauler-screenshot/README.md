# tauler-screenshot

Renders a [tauler](https://crates.io/crates/tauler) JSX layout to a vector SVG, using the
same evaluation, theming and layout pipeline as the bar itself. Use it for documentation
screenshots, visual regression tests, and iterating on a layout without restarting the bar.
It is also a library: `tauler_screenshot::render` is the same pipeline as one function call.

```sh
cargo install tauler-screenshot
```

Documentation: <https://kantord.github.io/tauler/docs/develop/screenshots/>

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
