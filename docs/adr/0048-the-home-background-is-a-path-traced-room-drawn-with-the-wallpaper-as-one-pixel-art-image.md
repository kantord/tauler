# The home background is a path-traced room, drawn with the wallpaper as one pixel-art image

The home page (and the draft slides, ADR
[0047](0047-the-site-is-fumadocs-on-a-next-static-export.md)) shows a room path-traced
live in the browser, inspired by a photo by Urban Vintage on Unsplash (see "Credit and
ownership"), in the bottom of the first screen, turned into the same dithered pixel art
as the wallpaper above it. It is not a picture
laid over the wallpaper. The room is a layer of the wallpaper's own WebGL 2 shader
(`src/components/site/wallpaper/wallpaper-gl.ts`, its GLSL in `wallpaper-shaders.ts`, its
palette and lattices in `wallpaper-grid.ts`): three.js renders it into the
wallpaper's context, one texel per wallpaper cell, up to continuous colour (path trace,
accumulate, denoise, filmic tone map), and the wallpaper's cells pass mixes it with its
own colour and quantizes both **once**.

- **One palette.** The wallpaper's OKLCH grid, filled by the wallpaper module's own
  `oklchToRgb()`. Room cells take hue on a 15° grid (`ROOM_HUE_SUB` = 4) where the
  wallpaper's is 60°. Every fourth hue column is the wallpaper's level, so the room's
  palette is a superset and wallpaper cells are unchanged to the byte.
- **One dither.** The wallpaper's ordered (Bayer 8 × 8) threshold, the same threshold for
  every channel, room or not.
- **One grid.** 3 device px per cell at any zoom, counted from the page's top-left
  (`WALLPAPER` in `wallpaper/options.ts` is the one source of these options).

Where every channel is still the wallpaper's, a cell is the wallpaper's own computation,
so `tests/wallpaper.spec.ts` still asks for every byte, and the room's edges are the
wallpaper exactly.

## Why one pipeline

The room began with a canvas and a pixel pass of its own, fixed to the window, with the
wallpaper's pixels under its fade computed in JS. That needed four load-time edits of
the renderer's pixel shader to imitate the wallpaper's grid, palette and quantize, a
main-thread texture rebuilt on every resize (about 100 ms at 1440 × 900), and it still
parted from the wallpaper on any page that scrolled. Quantizing in one place removes all
of that: the two cannot disagree on a pixel because only one thing draws pixels.

The single quantize is also why the room's colours had to join the wallpaper's palette
rather than bring their own: two palettes would draw a seam where the fade crosses them.
The 60° hue step was chosen for the wallpaper's near-grey colours. On the room's
saturated ones the dither spread each cell over hues up to 60° apart (cyan and yellow
specks in green leaves, pink in the tan floor), so the room gets the finer superset.
Rejected: rounding hue without dither, at 60° (whole areas jumped hue: an orange bowl,
an olive plant) or at 15° (as accurate, but a second quantize rule, and slow gradients
become hard steps); a 30° grid (a few off-hue cells left, same cost).

## Where it sits

The wallpaper is anchored to the page and scrolls with it (ADR 0047). The room is too: it
fills the bottom `ROOM_HEIGHT` (50svh) of the *first screen*, the full width, and scrolls
away with the content, so the two never drift. Below the first screen the image runs on,
repeating the floor, and fades into the wallpaper again. These pages drop the
`scrollbar-gutter` (`<Wallpaper edgeToEdge room />`) so the canvas reaches the window's
right edge.

The box runs from about 0.9:1 (an upright phone) to 12:1 (a short, wide window) against
the scene's own 3:2, so the scene extends its back wall and floor to the left and raises
its walls. It adds no ceiling and no left wall: the open sky lights most of the room.
The camera keeps a 40° vertical view (the 3:2 horizontal view on narrower boxes), and
its image plane is shifted down, not tilted, so the frame stays below the window's sill
and above its reflection in the glossy floor (`uVShift`, `uVMin`). The window is an
emitter near white, and in view it read as a hard band.

**The fade is made not to show** (`room-size.ts`). Each OKLCH channel eases from the
wallpaper's to the room's along a smootherstep, over its own share of the room's height
(`ROOM_FADE`: lightness longest). Three additions, each because a band showed without
it:

- the fade starts along a fractal sine wave across the page's x (`ROOM_WAVE`: four
  octaves from 760 CSS px, seeded from the wallpaper's hash, 30% of the room's height
  peak to peak, mostly above the room's box so its dips miss the furniture);
- near each edge the room's local mean lightness and chroma are pulled to the
  wallpaper's (`ROOM_MATCH`), because the light theme's walls are darker than its page
  colour;
- the room's detail eases in over its top 15% (`ROOM_DETAIL`).

## Hardware WebGL only

`page-wallpaper.ts` adds the room only on hardware WebGL 2 with float render targets
(`roomSupported()`). Software WebGL (SwiftShader, llvmpipe) is refused by renderer name:
a path tracer there takes seconds per frame and froze the page in testing. Everywhere
else (no WebGL 2, software WebGL, a lost context, an asset or shader that fails) the
wallpaper is drawn alone, by the shader or the CPU module, with no empty box. A shader
that fails to compile counts too: three.js checks a program's link status only on its
first use, so `createRoom()` uses every compiled program once (`getUniforms()`, no draw)
before it resolves, and a broken shader rejects with a `tauler:` warning instead of
fading in a black room. CI has no GPU; see "Testing without a GPU" for what it checks.

## How an image is made

- **48 samples, then the loop stops.** The renderer's page idled on
  `requestAnimationFrame` after converging; a background has no reason to. At the
  display's refresh the 48 take about 0.8 s. A resize or theme switch starts over.
- **A fresh start, eased in.** Each image starts from nothing (the renderer kept half
  the old image as a motion trail, which made the first frame dark), and the room's
  weight in the fade rises with its samples, from 0 (every cell the wallpaper's own) to
  1 at `ROOM_EASE_IN` = 32 samples. At 12 the early noise's dark specks still showed.
- **Fireflies are clamped.** Light reaching the camera by a reflection, and all light
  after the first diffuse hit, is limited to `FIREFLY_MAX` = 2 in luminance over the
  theme's exposure. Lights seen directly and the direct light at the first hit keep
  their value: those are the real highlights. Clamping whole samples turned the candle
  bulbs grey; clamping only post-diffuse light left the floor sparkling.
- **An edge-aware à-trous denoise** (`room/denoise.ts`), the spatial half of SVGF.
  The room shader also accumulates a G-buffer of first hits (normal, distance, albedo,
  luminance moment); the colour is divided by the albedo, the irradiance smoothed by
  four 5 × 5 passes (steps 1, 2, 4, 8) that stop at unlike normals, distances, albedos
  or a luminance step beyond the cell's noise, and the albedo multiplied back. Without
  it, 48 samples' variance crossed palette thresholds as specks of the wrong hue (243 on
  the floor, against 94 filtered and 103 for a 1000-sample render). Cut: temporal
  reprojection (the camera never moves), object ids (meaningless once averaged over a
  cell that straddles two objects), and filtering in OKLab (a mean of nonlinear values
  is biased dark). The normal test's exponent is 32, not SVGF's 128, so the cove is
  filtered too.

## Materials are physically based and procedural

Every material is a load-time edit of the room shader (`room/materials.ts`): GGX
microfacets sampled from their visible normals, Schlick's Fresnel on measured F0 for
metals, dielectrics choosing specular or diffuse by Fresnel's odds, a clearcoat for the
lacquered teak, albedos from measured values and none above 0.85. There are no textures
or images; the only texture is the renderer's blue-noise sampling texture. Colour,
roughness and shading normals come from non-periodic value noise on a pcg3d hash, in
object space, seeded per group and per piece, so the two chairs and every teak board
differ. Roughness matters most: at 3 px cells most detail is sub-cell, and what reads
is the floor's trowelled clouds and patchy sealer, worn leather, smudged chrome.

The budget is the inlining: the scene's intersection and material code is inlined at
each call site and costs every pixel, so each hit gets at most three noise calls (warp,
fBm, a detail octave on camera and mirror paths only) and the materials only map their
results. The denoise keeps the detail, because the G-buffer's albedo is the procedural
colour and is demodulated out before filtering.

## The Monstera, the cable, the cove

The room has a Monstera in a low ochre bowl on a chrome plinth, the scene's 8th and last
group, at the left (x −425), where wide frames show it and the 3:2 frame and phones do
not. Its generator is `room/monstera-scene.js`; its primitives (leaf blade, open
frustum, cluster box) and materials are shader edits in `room/monstera-shader.ts`.
The uplighter's cable, a Catmull-Rom spline cut into capsules, runs back along the
wall's foot to a socket the pot hides, so the lamp does not read as plugged into the
plant. Two choices a reader might "fix", both for cost:

- the path loop is replaced by the same estimator traced from **one** intersection call
  site instead of three, since every call site inlines the whole scene;
- each leaf's outline, splits, holes and veins are baked once into a 256 × 256 map per
  leaf before the first sample, not computed per ray; and the plant and cable sit in
  nested cluster boxes, which is why they trace faster with more primitives.

Walls meet the floor and each other in a 3 cm cove (`KIND.COVE`: the inside of a
cylinder), which meets both planes tangentially, so the junction reads as a soft change
of shading rather than a one-pixel line, and unlike a skirting it makes no edge above
itself.

## The theme changes the walls, not the light

The walls and floor take their albedo from the theme's page colour (`--wallpaper-bg`,
the floor at × 0.76, the walls at × 0.93); the sky, which lights most of the room, keeps
one fixed colour in both themes (`SKY`). A dark theme makes a dark room in the same
light, with the exposure raised (3.5 against 1.9) because dark surfaces bounce less of
it back. The furniture and the plant have fixed albedos.

## Cost

Measured on an RTX 4060 Ti, home page at 1440 × 900: about 4.1 ms a sample for the path
trace (3.7 ms before the plant and materials), 0.45 ms for the denoise, 0.1 ms for the
wallpaper's cells pass; nothing once the loop has stopped. The materials, plant and hue
grid each came in within or near measurement noise; that is what bought them a place.

## Testing without a GPU

CI has no GPU, and the room refuses software WebGL, so the room's GPU tests skip there.
One smoke test, `tests/room-smoke.spec.ts`, covers the room code in CI anyway: it sets
the test hooks `roomSoftwareGl` and `wallpaperSoftwareGl` so the room
runs on SwiftShader, in a 192×120 window (a 64×38-cell image), and stops it at
`ROOM_EASE_IN` (32) samples, the fewest at which the room shows at full weight. It checks
that every shader compiles and every load-time edit finds its anchor (the room joins,
with no `tauler:` warnings), that the image converges, and that the box shows a room:
varied, not black, the wall lighter than the plant. The last check also covers the
denoise, because the tone-map pass reads only its output, and shader compile failures,
which three.js reports only on first use and so draw black. It takes about 5–10 s and
does not judge how the picture looks; that stays with the GPU-only tests.

**The test hooks ship in production, behind one seam.** CI's Playwright tests run against
the production static build (`DOCS_PREBUILT`), so the hooks the tests set (software
WebGL, a sample cap, fades off, a broken shader) are in the production bundle. Stripping
them would mean testing a different artifact than the one deployed, or a second build in
CI. Instead every hook is a field of one object, `window.__taulerTest`, typed and read
only in `src/components/site/test-hooks.ts`, whose doc lists them. Nothing on the page
writes it and no URL or user input reaches it; absent, every hook is off at the cost of
one property lookup.

## Credit and ownership

The room is inspired by a
[photo](https://unsplash.com/photos/a-white-room-with-yellow-chairs-and-a-table-k_CZ0eldzvM)
by [Urban Vintage](https://unsplash.com/@urban_vintage) on Unsplash; no pixel of it is in the
site. The Unsplash License requires no attribution; the
page credits it anyway, in Unsplash's recommended form, its links tagged
`utm_source=tauler&utm_medium=referral`.

The path tracer is Erich Loftis's [THREE.js PathTracing
Renderer](https://github.com/erichlof/THREE.js-PathTracing-Renderer), CC0, credited on
the page anyway. The only third-party code is its GLSL chunks and blue-noise texture,
taken at a pinned commit (`room/vendor/PathTracingCommon.js` with the licence
beside it, `public/room/BlueNoise_R_128.png`), on three.js r185 from npm. The scene
is first-party: it began as a standalone page made for this site by its author, and its
shaders (`public/room/shaders/`), scene description (`room/room-scene.js`),
the plant, the materials and the denoise are the site's own code. The renderer's page
loop is ported into `room/room.ts`; every change to its shaders is a load-time
edit (`room/room-shader.ts`) at an anchor that must appear exactly once, so an upstream change fails loudly.
