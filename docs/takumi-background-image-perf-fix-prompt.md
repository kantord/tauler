# Task: fix a per-pixel hot-loop regression in takumi's background-image rasterizer

You are contributing a performance fix to **takumi** (`kane50613/takumi`). Work on a
new branch off `master`. The bug is in `takumi-raster`, is reproducible with a
benchmark you will write, and is a pure optimisation — **output pixels must not
change**.

## The defect

`BackgroundTile::SampledBitmap` is rasterized one pixel at a time, and each pixel
rebuilds state that does not depend on the pixel's coordinates.

**File:** `takumi-raster/src/background_drawing.rs`

`rasterize_row` has this arm:

```rust
Self::SampledBitmap { .. } => {
  for (x, chunk) in pixels.iter_mut().enumerate() {
    let p = self.get_pixel(x as u32, y);          // <-- re-enters get_pixel per pixel
    *chunk = [p.red(), p.green(), p.blue(), p.alpha()];
  }
}
```

and `BackgroundTile::get_pixel`'s `SampledBitmap` arm does, **for every pixel**:

```rust
let logical_width  = (*width).max(1);                        // loop-invariant
let logical_height = (*height).max(1);                       // loop-invariant
let source_width   = source.width().max(1);                  // loop-invariant
let source_height  = source.height().max(1);                 // loop-invariant
let mapped_x = (x as f32 + 0.5) * source_width as f32 / logical_width as f32;
let mapped_y = (y as f32 + 0.5) * source_height as f32 / logical_height as f32;
let footprint = SamplingFootprint::new(                      // loop-invariant
  source_width as f32 / logical_width as f32,
  source_height as f32 / logical_height as f32,
);
let Some(pixmap_ref) = pixmap_ref_from_buffer(source.as_ref()) else { ... };  // loop-invariant
let source = PaintSource::from(pixmap_ref);                  // loop-invariant
interpolate_with_footprint(source, *algo, mapped_x, mapped_y, footprint)
```

Only `mapped_x` / `mapped_y` and the final `interpolate_*` call actually depend on
`x`/`y`. Everything else is recomputed millions of times per frame.

`pixmap_ref_from_buffer` is the worst offender — it is
`PixmapRef::from_bytes(buffer.data(), buffer.width(), buffer.height())`, tiny-skia's
**validating** constructor, which re-checks that the byte length matches
width × height × 4. For a 397×2160 background that is 857,520 redundant validations
per render.

**This is demonstrably an oversight, not a design constraint**: the sibling arm in
the same `match` gets it right —

```rust
Self::Pixmap(t) => {
  let ps = PaintSource::from(t.as_ref());     // hoisted ONCE
  for (x, chunk) in pixels.iter_mut().enumerate() {
    let p = ps.get_pixel(x as u32, y);
    ...
  }
}
```

Note also `takumi-raster/src/image_drawing.rs`, which handles the `<image>` *node*
path correctly: it calls `pixmap_ref_from_buffer` once and passes the result into
`canvas.overlay_sampled_pixmap`. So the same image is fast as a node and slow as a
background — that asymmetry is the bug.

## Reproducing it with real numbers

This was found in a real application (a status bar) that draws a
desktop-wallpaper crop behind a full-height sidebar panel: a **397 × 2160** RGBA
bitmap drawn at exactly 1:1 into a 397 × 2160 viewport.

Write a benchmark that renders a container with a `background-image` referencing a
pre-decoded `ImageSource::Bitmap` of those dimensions, at that viewport, and
compare it against the same layout with no background image.

Measured on the current `master` (release build, single render, averaged over 10):

| variant | ms/render |
|---|---|
| no background image (floor) | ~6.2 |
| `background-image` at 1:1, `image-rendering: auto` (Catmull-Rom) | ~19.0 |
| `background-image` at 1:1, `image-rendering: pixelated` (nearest) | ~13.6 |
| the *same bitmap* as an `<image>` node instead | ~7.0 |

Two things to notice:

- Switching Catmull-Rom → nearest only recovers ~5 ms of the ~13 ms gap. The
  remaining ~8 ms is the per-pixel setup above, not the interpolation.
- The identical image via the node path costs ~1 ms over the floor. That is the
  number the background path should be able to reach.

**Sanity check that proves the interpolation is pointless at 1:1:** rendering the
same layout with `image-rendering: auto` and with `image-rendering: pixelated`
produced **byte-identical output** — 0 differing bytes out of 3,430,080. When
`source_scale == (1.0, 1.0)` and the source and destination dimensions match,
every sampler returns the source pixel unchanged.

## What to implement

**1. Hoist the invariants.** Restructure so `rasterize_row`'s `SampledBitmap` arm
computes the dimensions, the `SamplingFootprint`, the `PixmapRef` and the
`PaintSource` **once per row at the very least, ideally once per tile**, then loops
only over the coordinate-dependent work. Mirror how the `Self::Pixmap` arm is
written. Keep `get_pixel` working for any external callers (e.g. the
`BackgroundTile::get_pixel` used elsewhere) — add the hoisted path rather than
breaking the per-pixel API.

**2. Add a 1:1 fast path.** When `source_width == logical_width &&
source_height == logical_height` (equivalently `source_scale == (1.0, 1.0)`), the
destination row is exactly a source row. Copy it directly — `copy_from_slice` over
the row — with no sampling, no footprint, and no per-pixel branch. This should
apply regardless of `ImageScalingAlgorithm`, since all algorithms provably agree at
1:1 (see the byte-identical check above).

## Constraints

- **Output must be bit-identical** for every existing test and for scaled cases.
  The 1:1 fast path must only trigger when it is provably equivalent — be careful
  with the half-pixel offset (`(x as f32 + 0.5) * scale`) when deciding that
  `mapped_x` lands exactly on the source pixel centre.
- Do not change any public API or CSS semantics.
- Keep `image-rendering` honoured for genuinely scaled images.
- Add a regression test asserting that a 1:1 background image renders
  byte-identically under `auto`, `smooth` and `pixelated`.
- Include the benchmark, or at minimum before/after numbers, in the PR description.

## Definition of done

- `background-image` at 1:1 lands within ~1–2 ms of the `<image>` node path on the
  397 × 2160 case (i.e. roughly 7 ms, down from 19 ms).
- Existing snapshot/visual tests pass unchanged.
- The new 1:1 regression test passes.
