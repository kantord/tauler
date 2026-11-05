# The site is Fumadocs on a Next.js static export

The docs site moved from Astro + Starlight to Fumadocs on Next.js (App Router,
`output: 'export'`, `distDir: 'dist'`). Fumadocs also ships an Astro integration,
which would have kept the landing page as it was; it was `@fumadocs/astro` 0.1.x at
the time, and Fumadocs' layouts can be vendored into the repo shadcn-style
(`fumadocs customise`) only with confidence on Next. That vendoring is the reason for
the move: the docs chrome becomes code we own and edit, not overrides fighting a
theme's own CSS.

The move starts from Fumadocs' stock theme and a stock home page, on purpose. The
landing page (the CRT hero, the flow field, the desktop frame around the docs) and the
design tokens were deleted rather than ported; they are in git history at the commit
before this ADR. Consequences for earlier decisions:

- ADR 0029 (tokens in a Tailwind `@theme` block) and ADR 0030 (layered, machine-checked
  site styling) have nothing left to govern. `check-class-layers.ts` is gone with the
  layers it checked. Both return, translated, when the site gets its own design again.
- ADR 0031's pixel gate covered the landing page only; with it gone, `docs/tests/` no
  longer has a `toHaveScreenshot`. The pinned-container arrangement stays for when it
  comes back.
- ADR 0032's example (Starlight's GitHub icon) is gone; the rule — assets from npm,
  never hand-copied — stands.

What stays the same: the wasm module, the component screenshots and the stylesheet are
still `just docs` build products, the static site is still `docs/dist`, and URLs still
end in a slash. Two things had to change shape:

- The component reference page can no longer carry raw `<script>`/import-map HTML
  (React does not run it). `tauler-docgen` emits MDX instead — `<TaulerExample>` per
  component and one `<TaulerExamples />` that boots the runtime — and writes the import
  map to `.tauler/importmap.json`, which the root layout inlines into every page's
  `<head>`, because a client-side navigation never re-renders the `<head>`.
- GitHub Pages has no server redirects, and a static export's `redirect()` only works
  with JavaScript; the pre-`/docs` URLs are meta-refresh pages instead.

## Amendment: the site's own design

The stock theme was replaced by the "tauler — website" design (framed tiles with hatched
title bars, Archivo/DM Sans/IBM Plex Mono, light and dark). Its layout is not Fumadocs'
`DocsLayout`, so the docs pages render their own chrome (`src/components/site/`) from
Fumadocs' headless parts — the page tree, the TOC, `findNeighbour`, the search dialog —
rather than vendoring Fumadocs' layout with `fumadocs customise`. That keeps the code we
own to what the design actually draws. The design's dithered wallpaper is
a first-party module (as ADR 0032 already counted it) in `src/lib/wallpaper/`, linted
and formatted like the rest of the site. It is mounted on a viewport-fixed host, not on
the page itself: a page-sized canvas costs width × height × dpr² × 4 bytes and the longest
docs pages pass the browser's canvas size limit, so the wallpaper does not scroll with
the page.

Code blocks are highlighted with the design's own palette (`src/lib/shiki-theme.ts`),
dark in both site themes. Every axe rule runs again; the four the stock theme failed
were switched off only while the site used it.

Formatting and linting moved to oxc: `oxfmt` (Prettier-compatible, with Tailwind class
sorting) replaces Prettier, and `oxlint` with the TypeScript, React, Next.js and import
plugins runs in Docs CI. ADR 0030 picked Prettier because it was the only mature
formatter for `.astro` templates; with no `.astro` files left, that reason is gone.

## Amendment: a temporary path-traced background on the home page

The home page is now only the top bar, the logo and this background; the landing
content is on three slide pages (`/overview/`, `/principles/`, `/get-started/`), one per
header workspace button, which share the background.

The home page's background is, for now, "White Room, Pixel Traced": a white room
path-traced live by Erich Loftis's
[THREE.js PathTracing Renderer](https://github.com/erichlof/THREE.js-PathTracing-Renderer)
(CC0, credited on the page anyway), then reduced to dithered pixel art on the
wallpaper's palette: its pixel pass quantizes on the wallpaper's own grid, read from the
same source the wallpaper uses (`WALLPAPER` in `src/components/site/wallpaper/options.ts`). It began
as a standalone page, made for the site and copied in once: its shaders
(`public/whiteroom/shaders/`) and its scene description
(`src/components/site/whiteroom/room-scene.js`) are first-party code. The only
third-party code is the renderer's GLSL chunks and blue-noise texture, taken from
upstream at a pinned commit (`whiteroom/vendor/PathTracingCommon.js`, with the CC0
licence beside it, and `public/whiteroom/BlueNoise_R_128.png`).
three.js is r185 from npm, the revision the page was written for. The page's render loop
is ported into `src/components/site/whiteroom/whiteroom.ts`, without its tweak panel and
drag controls.

The render does not fill the window: the standard wallpaper does, as on the docs pages,
and the room fills the bottom half of the window on top of it, edge to edge. These pages drop the site's `scrollbar-gutter: stable`
(`<Wallpaper edgeToEdge />`): Chrome clips fixed elements at the root's gutter, so the
render and wallpaper could not otherwise reach the window's right edge.

The render's aspect therefore runs from about 0.9:1 (a phone held upright) to 12:1 (a
short, very wide window) instead of the scene's own 3:2. The camera keeps the page's rule:
a 40° vertical view, widened horizontally to fit, and on boxes narrower than 3:2 the 3:2
horizontal view, widened vertically. Those wider and taller views see past the scene,
which was built for 3:2: beyond the left end of its back wall and floor, and over its
walls. So the scene runs the back wall, its window band and the floor further left
and raises the walls; it adds no ceiling or left wall, which would shut out the open sky
that lights most of the room. It also gives the junctions of walls and floor, and of
the two walls, a small cove (3 cm), so they read as a soft shading change rather than
a harsh one-pixel line. The cove is a primitive kind of the site's own, the inside of a
cylinder between its axis and the corner, which `whiteroom.ts` teaches the room's
shader; it meets both planes tangentially, so unlike a skirting it makes no edge above
itself. Otherwise, at 3:2 the room looks as before.

The render's dither pixels are the wallpaper's: 3 device px at any zoom (the size is read
from the wallpaper module), on one grid counted from the window's top-left, with the
Bayer pattern running on from the wallpaper's. The page counts its 4 CSS px pixels from
the canvas's bottom-left instead, so `whiteroom.ts` edits two lines of the page's pixel
shader as it loads it, as it already edits the tone-mapping shader.

The room follows the site's theme: its walls and floor start from the wallpaper's page
colour (read from `--wallpaper-bg` and the ring variables, as the wallpaper reads them),
with the page's "Tauler light" or "Tauler dark" backdrop preset, while the light stays
the same in both. The page lights the room with its backdrop colour, so `whiteroom.ts`
gives the sky (what escaping rays pick up) a fixed colour of its own, the page's "Full
colour" backdrop, by the same kind of load-time shader edit; the dark theme raises the
exposure (3.5 against 1.9) because dark walls bounce less of that light back.

The render fades into the wallpaper in its pixel shader, not under a CSS mask, so the
canvas is opaque. `whiteroom.ts` computes the wallpaper's own pixels under the fade
with the wallpaper module (same options, colours and grid) and hands them to the pixel
pass as a texture; before the one quantize, each OKLCH channel eases from the
wallpaper's to the render's over its own share of the render's height (`ROOM_FADE` in
`room-size.ts`). The palettes being one, a wallpaper pixel quantizes to itself, so the
render's top row is the wallpaper exactly.

Three choices a reader might "fix":

- **The loop stops** once the image has its 48 samples. The page idles on
  `requestAnimationFrame` from there; a background has no reason to.
- **Software WebGL is refused**, by `failIfMajorPerformanceCaveat` and by renderer name
  (headless Chrome hands out SwiftShader regardless of the flag). A path tracer there
  takes seconds per frame and froze the page in testing; those visitors get the
  wallpaper alone. CI has no GPU, so CI only sees the fallback; the test that checks the render
  itself skips there.
- **Shaders compile before the canvas joins the page**, each against the render target it
  draws into, because three.js picks a shader variant from the current target: compiled
  against the wrong one, it compiles again, blocking, on the first frame.

## Amendment: shadcn/ui as a foundation, not a component set

`docs/components.json` sets up shadcn/ui with the Base UI flavour (`base-lyra`), so
`shadcn add` and `fumadocs add` place components in `src/components/ui` and reuse each
other's primitives, on the Base UI that Fumadocs already ships rather than a second
primitives library. No components are installed: one is copied in only when a page needs
it. shadcn's theme variables in `global.css` all point at the `--tu-*` tokens and
`--radius` is 0, so whatever is added comes out in the design's colours, fonts and square
corners. Two of the init's additions are left out on purpose: the `@layer base` rule
(it would restyle every border and the body) and `shadcn/tailwind.css` (it redefines
`data-open:` and friends, which changes Fumadocs' own selectors; Base UI components work
with Tailwind's built-in variants).

This does not contradict the choice above not to vendor Fumadocs' layout. That layout is
thousands of lines we would own for no benefit; a shadcn primitive is a small, accessible
component that our tokens already theme, and we own only the ones we use.

## Amendment: the wallpaper is a shader anchored to the page

The background is on its way to being one WebGL pipeline, the wallpaper and the room
drawn together. The first step is done: the wallpaper is a WebGL 2 fragment shader
(`src/components/site/wallpaper/wallpaper-gl.ts`), and it is anchored to the page, so
it scrolls with the content and a long page shows more of the pattern. The room is
still its own renderer on top, on the home page and the slides, unchanged: fixed to the
window, it fades into the wallpaper's pixels as they are at the top of the page, so on a
slide that scrolls (on a phone) the two part once it scrolls, until they are one
pipeline.

The page-sized canvas the section above rules out is still ruled out. One canvas the
window's width and height plus a margin above and below sits in a host covering the page
(the body is positioned for it) and is drawn for the part of the page it covers; the
browser scrolls it with the content, so it never lags behind it, and only when the
window nears its edge is it moved and drawn again (`page-wallpaper.ts`). A canvas fixed
to the window and redrawn on scroll would be drawn from the main thread's scroll
position, a frame or more behind the compositor's on a fast scroll.

The CPU module (`src/lib/wallpaper/tauler-wallpaper.js`) stays, as the
reference and the fallback. The shader draws its pixels byte for byte for the same
options, colours, device pixel ratio and page position, and `tests/wallpaper.spec.ts`
holds it to that; the palette, the hue lattice (a dynamic-programming table) and each
row's and column's lattice weights are computed with the module's own code on the CPU
and uploaded, so the shader's single precision touches little. Where the shader cannot
run the module draws the same slices on the CPU: no WebGL 2, a lost context, and software
WebGL, which is slower than the CPU path here (headless Chromium's SwiftShader, so CI
draws the wallpaper on the CPU, and checks the shader on SwiftShader only in the tests
that ask for it).

The module gained three things for this: it can compute a slice of the page by itself
(`col0`, `row0`), it exports the internals the shader reuses, and the background level
is the background colour itself. The module's first version drew it at the
background's lightness and chroma but at hue 0°, not the background's own hue, so the
"background" pixels were a pinkish `rgb(233, 226, 228)` on the `#e6e4de` page. The
room's pixel pass follows the same rule. `WALLPAPER` (`wallpaper/options.ts`) is the one
source of the options for the module, the shader and the room.


## Amendment: the room and the wallpaper are one pipeline

The background is now one WebGL pipeline. The room no longer has a canvas or a pixel
pass of its own: it renders with three.js into the wallpaper's own WebGL context, one
texel per wallpaper cell, up to continuous colour (path trace, accumulate, filmic tone
map), and the wallpaper's cells pass composites it. Per cell it takes the wallpaper's
continuous OKLCH colour and, where the room covers the cell, the room's, eases from one
to the other channel by channel, and only then quantizes and dithers, once, with the
wallpaper's rule, threshold matrix and palette (a palette texture filled by the module's
own `oklchToRgb()`, now spanning every lightness and chroma the room can reach). One
grid, one palette, one dither: the room's cells are the wallpaper's by construction, and
where every channel is still the wallpaper's the cell is the wallpaper's own
computation, byte for byte, so `tests/wallpaper.spec.ts` holds as before and the room's
edges are the wallpaper exactly.

Gone with it: the page's pixel shader (`fs-pix.glsl`) and the four load-time edits that adapted it (window grid, wallpaper
palette, fade, single quantize); the JS wallpaper texture computed for the fade on every
resize (about 100 ms on the main thread at 1440 × 900); `RoomBackground.tsx` and its
fixed host. `<Wallpaper edgeToEdge room />` asks for the room; `page-wallpaper.ts`
adds it where the shader runs on hardware WebGL 2 with float targets, and anywhere else
(no WebGL 2, software WebGL, a lost context, assets or shaders that fail) the wallpaper
is drawn alone, by the shader or the module, with no empty box.

**The room is anchored to the page.** It fills the bottom 50svh of the *first screen*
(page y from 100svh − `ROOM_HEIGHT` to 100svh) and scrolls away with the content, like
the wallpaper it is drawn with: no drift between the two, and no phase jitter while
scrolling, its cells being the page's. On the home page, which does not scroll, it
looks as before. On a slide that scrolls the image runs on below the first screen,
repeating the floor (below), and fades into the wallpaper there the way its top does,
so the floor never stops at an edge.

**The fade is made not to show.** Per channel as before (`ROOM_FADE` in `room-size.ts`,
now along a smootherstep), with three additions, each because a band showed without it:

- *A wavy edge* (`ROOM_WAVE`): the fades start along a fractal sine wave across the
  page's x, five octaves (lacunarity 2.13, persistence 0.5, first wavelength 760 CSS
  px), each with a seeded phase that smooth noise bends and an amplitude smooth noise
  scales, all from the wallpaper's hash, soft-limited to 30% of the room's height peak to
  peak. It lies mostly above the room's box (there the room is the wide mean of its top
  row of wall), so its dips stay clear of the lamp, the picture and the furniture. A first try,
  seeded noise of 12% depth every 160 px, still read as a straight line.
- *Matched means* (`ROOM_MATCH`): near each edge, the room's local mean lightness and
  chroma (a wide, short window of the room image at the edge row) are pulled to the
  wallpaper's smooth colour there, fully at the edge, easing to none. The light theme's
  walls are a good deal darker than its page colour; without this, a smooth ramp still
  read as a band. The room's own detail is kept. (The window's taps sit on a fixed
  column grid and it slides between two placements: taps that moved with each column
  sampled fresh grain and drew vertical streaks.)
- *Detail eased in* (`ROOM_DETAIL`): over the top 15% of the room its detail fades in
  over its wide mean, so neither the rows above the image (where the wave rises) nor the
  first rows of grain draw a line along the room's top.
- One quantize keeps the dither's density and pattern continuous across the seam.

`tests/docs.spec.ts` measures it: the row-to-row change of the screen's mean OKLab
colour, row by row down through the fade's start, stays a gentle gradient.

**The window band is framed out, not built over.** The hard light band near the room's
top in the dark theme was not open sky: it was the window, an emitter near white in both
themes, at the top of the view. A ceiling or a raised window would have changed how the
room is lit (the open sky and the window light most of it). Instead the camera's view
is held below the sill: the image plane is shifted down rather than the camera tilted,
so walls stay upright, and tall boxes (a phone) see more floor rather than the window
and the wall above it (a load-time edit of the renderer's camera ray, `uVShift`). The
view also never looks lower than just above the window's reflection in the glossy floor
(`uVMin`), which tall boxes and the image's run below the first screen would otherwise
reach; rows below it continue the floor at that depth. The scene, its light and the
exposure are unchanged, so the lighting is too; at 3:2 the frame is the old one less its
top eighth, more floor at the bottom.

## Amendment: a Monstera in the room

The room has a plant: a Monstera deliciosa in a low ochre bowl on a chrome plinth
(the chairs' colour and steel), a little way out from the back wall at the left of
the scene (the 8th and last group the renderer takes, at x −425), where every wide
frame shows it, clear of the logo above and of the fade, and the page's own 3:2 frame and
phones do not reach. Its habit follows photographs of real potted plants of its size: a
compact, upright cluster of steep petioles, blades turned out and down at the geniculum,
the oldest leaves small and whole, the newest large and split, one still furled, and a few
faults on purpose (a sagging leaf, a turned one, a torn margin). The uplighter's cable
used to stop on the open floor; it now curves back to the wall and runs along the cove
at its foot behind the planter, up to a wall socket the pot hides, so the lamp does not
read as plugged into the plant.

It goes in the way the room's other changes do. The scene adds it as its 8th group,
from the plant's generator (`whiteroom/monstera-scene.js`). The shader side is load-time
edits of `fs-room` at single anchors (`whiteroom/monstera-shader.ts`, applied in
`whiteroom.ts`): three primitive kinds (a leaf blade, an open frustum for the pot, and a
cluster box that lets a ray skip a leaf and its stalk when it misses them), the plant's
materials, and light through the leaves. Its colours are fixed albedos, like the
furniture's, so it follows the theme only through the light the walls bounce, and it
quantizes onto the wallpaper's palette like everything else.

Its cost is what shaped it. Code in the room shader is paid on every pixel whether or
not the plant is on screen, because the scene's intersection code is inlined at each
call site. So two things changed that a reader might "fix": the page's path loop is
replaced (also an edit) by the same estimator traced from one intersection call site
instead of three, and the leaves' outlines, splits, holes, veins and drooping straps
are not computed per ray but baked once into a small texture (one 128 × 128 map per
leaf, by a shader of its own, before the first sample) that the blade's intersection
samples. Leaves are matte with a sheen rather than glossy: at 48 samples a glossy coat's
rare reflections of the window were white dots. Measured on an RTX 4060 Ti, per frame
of the home page at 1440 × 900: about 3.7 ms before, 4.7 ms with the first plant
(2560 × 1440: 8.8 and 11.3), about 5.0 ms with the rebuilt one (11.8), within the
measurements' noise of the first in a side-by-side bench; the 48 frames still take about
0.8 s, bound by the display's refresh.

Follow-up, judged on 1000-sample renders against photographs: the plant read as a
cartoon (flat, saturated leaves on straight, parallel stalks) and the cable turned
square corners. Now each cutting is a thick stem that leans out of the soil and curves
up, ringed at its nodes, with brown aerial roots; each petiole leaves its node in a
sheath, rises in an uneven S of its own length, thickness and sway, and turns at a paler
knee to meet its blade from below, so stalks cross and blades clump, droop and turn
unevenly. The leaf maps are 256 × 256 and add asymmetric halves, a fold along the
midrib, a rippled margin, sunk veins with the blade puffed between them (the shading
takes that relief), secondary veins, and blemishes: a yellowing margin and a dry tip on
the oldest leaves, a few spots. The blade picks its face from the true slope, not the
exaggerated one, which showed a strap's upper face from below. Leaves are a darker,
bluer, less saturated green, glossier when new. The cable is a centripetal Catmull-Rom
spline through the old route, cut into capsules wherever a chord would leave the curve
by more than 0.3 mm. Two things a reader might "fix": the stalks' lengthwise streaks
read the noise off the surface normal, because fetching the capsule's axis in the
material cost 0.6 ms a sample; and the plant and the cable are cut into nested cluster
boxes (a few capsules each), which is why three times the primitives trace faster:
about 4.26 → 4.10 ms a sample at 1440 × 900, 8.6 → 7.9 ms for a close view of the
plant, and +2.3 KB gzipped.

## Amendment: the room fades in, and fireflies are clamped

**The room fades in from the wallpaper as it converges.** It used to join the
wallpaper with its first sample, and the first frames flashed dark: the renderer
starts an image by halving its first sample (a motion trail for a dragged camera, kept
from the page), so the first frame was half as bright, and the next few were raw
noise, many cells near black where no path had found light yet. Now each new image
starts from nothing with the whole first sample (a load-time edit of the renderer's
accumulation), and the room's weight in the composite follows its samples: a weight
per OKLCH channel multiplied into the fade, the same mix, rising along a smootherstep
from 0 with no samples (the room is then not drawn, every cell the wallpaper's own,
byte for byte) to 1 at `ROOM_EASE_IN` samples for lightness, sooner for chroma and hue
in the ratio of their `ROOM_FADE` shares. 32 of the 48, about half a second: over 12,
dark specks of the early noise still showed on the light walls. A resize or theme
switch starts the image over, so the room fades in again from the wallpaper.

**Reflected and indirect light is clamped against fireflies** (`clampFireflies()` in
`whiteroom.ts`, a load-time edit of the path loop). Clusters of lone bright cells
around the candle lamp and the right-hand chair, on the wall behind and in the glossy
floor below, survived every sample count: rare paths reaching a candle bulb, the
uplight or the window through a rough reflection or after a bounce, each worth up to
24 times the scene's light. Each such contribution is now limited in luminance to
`FIREFLY_MAX` over the theme's exposure (the same displayed brightness in both themes):
light reaching the camera by any reflection, and all light after the first diffuse hit.
Lights the camera sees directly and the direct light at the first hit keep their full
value; those are the real highlights. Clamping whole samples instead turned the
candle bulbs grey; clamping only the light after a diffuse bounce left the floor's
reflections sparkling. `tests/docs.spec.ts` counts the isolated bright cells there:
99 and 144 (light, dark) before, 27 and 79 after.

## Amendment: the room is denoised

**The room's accumulated image goes through an edge-aware à-trous filter before it is
tone-mapped** (`whiteroom/denoise.ts`): the spatial half of SVGF (Schied et al. 2017).
The room shader also accumulates a G-buffer from its first hits (normal, hit distance,
albedo, a luminance moment) in two more attachments of its target; the colour is divided
by the albedo, the linear irradiance left is smoothed by four passes of a 5 × 5 kernel at
steps 1, 2, 4 and 8, each tap stopped by unlike normals, distances, albedos or a
luminance difference beyond the cell's noise, and the albedo is multiplied back in. The
page's copy shader is no longer used (the copy pass copies all three attachments), so it
is gone.

The grain it removes was path-tracing variance at 48 samples, not quantization. Measured
as the detrended standard deviation of OKLab lightness, the raw 48-sample image against a
render of about 1000 samples: .025 against .006 on the floor, .021 against .005 on the
right wall. The quantize then turned that jitter into specks of the wrong hue, where it
crossed palette thresholds: 243 on the floor unfiltered, 94 filtered, and 103 for the
1000-sample render, so the filtered image is as clean as converging would make it.

It costs about 0.45 ms per sample at 1440 × 900, and nothing once the loop stops at 48;
the room still converges in about 0.8 s. It runs after every sample, so the room fades in
already smooth, with no pop at the end.

Cut, and why: temporal reprojection, SVGF's other half, because the camera never moves;
object ids as an edge stop, because an id averaged over samples means nothing where a
cell straddles two objects (the accumulated normal, distance and albedo say the same
thing and degrade gracefully); and filtering in OKLab, because demodulation is a product
in linear light and a weighted mean of nonlinear values is biased dark where the noise is
worst. The normal test's exponent is 32 rather than SVGF's 128, which left the 3 cm cove
where the walls meet the floor unfiltered.

Known leftovers: the cove line is still slightly bright and dotted, and big surfaces
keep faint low-frequency blotches, which the wallpaper's dither hides.

## Amendment: the room's materials are physically based and procedural

Every material in the room is now a load-time edit of the room shader
(`whiteroom/materials.ts`, applied after the cove's and the plant's edits and
before the firefly clamp and the G-buffer's, whose anchors it keeps). No
textures, images or tables: the only texture is still the renderer's blue
noise.

**The BSDF.** The page's lobes (a mirror blurred toward a cosine lobe, its
weight a Fresnel term scaled by hand per material) are replaced by GGX
microfacets, sampled from their visible normals (Dupuy and Benyoub 2023) and
weighted by Smith's G1, with Schlick's Fresnel on a measured F0 for metals.
A dielectric picks its specular lobe with Schlick's probability at the
shading normal (roughness-aware, Fdez-Agüera 2019) and its diffuse lobe
otherwise, so the two share the light that came in; a clearcoat (the teak's
lacquer) is one more such layer on top. Albedos come from measured or
published values and none is above 0.85 (sources in the module); chrome is
chromium's F0, (0.55, 0.56, 0.55), not the page's 0.9. The walls and floor
still take their colour from the theme's page colour, and the light stays the
same in both themes.

**The leaves' sheen is a real specular lobe.** The plant's amendment made
leaves matte with a sheen painted into the albedo, because a glossy coat's rare
window reflections were white dots at 48 samples. With the firefly clamp and
the denoise in place, a GGX cuticle (roughness 0.24 on new leaves to 0.34 on
old, duller where dust lies) holds up, so the albedo trick is gone.

**The noise.** Value noise on the pcg3d integer hash (Jarzynski and Olano
2020), three channels at once, in object space, its octaves rotated so no
lattice lines up: it has no period anywhere. Each hit evaluates the same three
noise calls whatever its material: a one-octave warp, an fBm (four octaves on
camera and mirror paths, two after a diffuse bounce, where only the mean albedo
matters) and a fine detail octave on camera and mirror paths only, each at a
frame its material picks; the materials only map those numbers. The room's
intersection and material code is inlined at its call site and costs every
pixel, so one call site per noise is the budget. Seeds are per group, so the
two chairs differ, and per primitive for things made of pieces (each teak
board, each leaf), not for surfaces built of many primitives (a sling of quads,
a tube of capsules), where the seams would show.

**What reads at 3 px cells is roughness.** Most detail is sub-cell at the
site's scale; what survives is mid-frequency: the floor's trowelled clouds and
its sealer's gloss, hazed along two walking paths and dusty along the walls,
the window's reflection wavering in a slab not quite flat, each teak board's
tone, burnished leather, smudged glass and chrome. Close-ups, rendered with a
debug camera outside the repo, are how the materials were judged against
photographs.

**The denoise keeps it.** The G-buffer's albedo is what the material
returns, so the procedural colour is demodulated out before filtering and
multiplied back after: measured on the 1440 × 900 image, the 48-sample
filtered image has the same local contrast as a 1024-sample one (sideboard,
rug, floor), so the filter smears none of it.

**Cost.** RTX 4060 Ti, home page at 1440 × 900, path-trace pass per sample,
alternating runs, minimum of eight batches of 60: 3.96–4.06 ms before,
3.95–4.01 ms after; within noise, no measurable cost. The denoise is
unchanged. About 4.4 KB gzipped of shader strings added to the room's bundle.

Cut to stay simple: true bump mapping from a height field's gradient (the
shading normal is tilted by noise vectors instead), wood figure below 1 cm and
the rug's individual fibres (sub-cell everywhere but in close-ups), and
next-event estimation for the specular lobes (they find the lights by their
own rays, as before).

Follow-up, judged at site scale: the floor is now a little darker than the
walls (page colour × 0.76), its trowelled clouds strong enough to read at 3 px
cells, and its sealer near mirror in patches, so chair legs, the plant and the
sideboard mirror softly in it; a pale floor mostly reflects the equally pale
wall, so the clouds are what reads in the light theme. The walls drift a few
per cent in tone over a metre or two and have touched-up, glossier patches;
the rug is wool pile in weft-row streaks rather than flecks; the teak's rings
are cut at a slant into cathedral arches, upright panels book-matched; the
slings wrinkle where they sag. About +0.06 ms a sample and +0.6 KB gzipped
over the first version (+5.0 KB in all).

Second follow-up, judged on a 1000-sample render of the whole room against
photographs: the rug, the teak and the leather read as painted. The teak
now darkens through each ring's latewood and breaks sharply into the next,
with long straight streaks of tone, the odd mineral line and teak's open
pores as dark dashes along the grain (a hashed grid that follows the
rings); its interlocked fibres tilt the oiled wood's normal in ribbons, so
the sheen swaps with the view, and the lacquer has its own orange-peel
normal. The arches are flatter, so most boards are near straight grain.
The leather is tan veg-tan, not ochre: burnished darker and glossier where
it is sat on and held, lighter where it bends, with pebbled hide grain up
close and a saddle stitch along its edges. The rug is a knotted wool pile
in soft tufts with heathered flecks and pile patches that change with the
view, flat-woven green-striped ends, and a fringe of separate cords with
the floor between them. Pebbles and tufts are 2D cellular noise, nine
hashes, on camera and mirror paths only. About +0.14 ms a sample
(3.96 → 4.10 ms) and +1.7 KB gzipped. Each cord is then cream or a dull cotton
green, chosen by the scene's fringe's own hash over its 1.1 cm bars, so its
irregular runs of green read again from across the room; the end band's
stripes take the same green (one hash per fringe hit, +0.1 KB gzipped).

## Amendment: the room's hue grid is finer

The room's colours are quantized on the wallpaper's grid, and that grid's
hue step is 60°. That step was chosen for the wallpaper, whose colours are
near grey, where hue barely shows. With the room's saturated colours, the
dither spread each cell over the two hue levels around its colour, up to 60°
away. Green leaves got cyan and yellow specks, and the tan rug fringe and the
floor got pink ones.

Room cells now take hue on a 15° grid (`ROOM_HUE_SUB` = 4 in
`wallpaper-gl.ts`), with the same threshold as every other channel, so the
dither still blends the two levels around a colour. Lightness and chroma
are unchanged. The palette is a superset of the old one: it has
`nHq × 4` hue columns (24 × 319 texels, was 6 × 319), and column `q × 4` is
the wallpaper's level `q`, the same colour as before. Wallpaper cells use only
those columns, so their pixels are unchanged to the byte
(`tests/wallpaper.spec.ts` still asks for every byte). A room cell that fades
into the wallpaper picks from a superset of the wallpaper's colours, so the two
stay one palette. The room has no path in the module
(`tauler-wallpaper.js`), so it is unchanged.

Measured on 48-sample renders at 1440 × 900, light theme (dark in brackets),
cells whose hue lands more than 30° off their continuous colour went to zero:
leaves 240 → 0 (271 → 0), rug fringe 269 → 0 (286 → 0), floor 409 → 0
(590 → 0), and the bowl and both chairs also went to zero. Mean OKLab a/b
error dropped by 15–55% per region (bowl 37.1 → 15.9, leaves 19.7 → 12.9,
floor 15.9 → 14.5, ×1000). The rows above the room were byte-identical in
both themes. Cost: the cells pass measured 0.103–0.108 ms before and
0.099–0.110 ms after (median of 7 × 100 passes), so no measurable change.

`tests/wallpaper.spec.ts` checks two things on the CPU, because CI's software
WebGL draws no room. First, every 4th palette column is the old palette.
Second, a green leaf's colours, quantized with every threshold of the matrix
by the shader's room rule, land within one 15° step of their hue. On the 60°
grid they land 45° or more off.

Rejected:

- *Rounding hue to the nearest 60° level, with no dither* (fixB). It moved
  whole areas to the nearest of six hues: the bowl turned orange, the rug
  pink and the leaves olive. Some off-hue cells stayed (floor 7, fringe 15),
  and its mean error was higher than the 15° grid's in every region.
- *Rounding hue to the nearest 15° level, with no dither* (fixC). Its error
  was within about 1/1000 of the dithered 15° grid. But it adds a second
  quantize rule to a pipeline that is built on one, and it turns a slow hue
  gradient into hard steps where the dither blends them.
- A 30° grid (fixA30) left a few off-hue cells (one each in the floor and
  chair) and had a higher error than 15°, at the same cost.

## Amendment: the slides are drafts

The three workspace slides (`/overview/`, `/principles/`, `/get-started/`) are
unfinished, so they are drafts: `next dev` serves them, the published site does
not have them. The published site is the home page and the docs.

A draft route file is named `page.draft.tsx`. `next.config.mjs` adds `draft.tsx`
to `pageExtensions` only when `NODE_ENV` is `development`, so in the build the
file is not a page and the export has no route, HTML or chunk for it. There is no
runtime check to forget. Links to the slides check `DRAFTS` (`src/lib/drafts.ts`,
the same `NODE_ENV` test): the header's workspace buttons render nothing and the
sitemap leaves the slides out. Search and `llms.txt` come from the docs collection
and never had them.

The slides' Playwright tests run only with `DOCS_DEV_URL` set to a running dev
server; against the build they skip, and one test checks that the build has no
slide routes and no workspace buttons.

Rejected:

- *Keep the routes and hide only the links.* The pages would still be published
  and reachable by URL.
- *An explicit env var such as `DOCS_DRAFTS=1`.* It can disagree with the mode:
  a dev server without it hides the slides, a build with it publishes them.
  The mode itself already says which one is wanted.

## Amendment: the site's controls are themed shadcn components

The site's buttons, button-styled links and callouts are shadcn's `Button` and
`Alert` (base-lyra, added with `shadcn add`), in `src/components/ui/`. They used
to be hand-built, each repeating the same inline classes. The look lives in two
places. The colours are the shadcn slots in `global.css`, which point at the
`--tu-*` tokens (above). The shape is the components' own `cva` classes, edited
to the design: a 1.5px rule, square corners, 12px uppercase mono with wide
tracking, and an orange `focus-visible` outline. The variants are `default`
(orange fill), `outline`, `outline-primary` (the terminal's copy button) and
`nav`, a link whose current page (`aria-current="page"`) is lit. The sizes are
`default`, `lg` (the slides' calls to action), `inline` (the header's text links)
and `icon` (the workspace keys).

A link that looks like a button is a Next `Link` with `buttonVariants()` as its
class, not a `Button` with a `render` prop: Base UI's Button puts button
semantics (`role="button"`) on any element it renders, and its docs say not to
render links with it. `buttonVariants()` merges its classes with `cn`, so a size
like `inline` replaces the base border rather than racing it in CSS order.

The callout's title reuses the code panels' title bar (`.sbar`), lit orange. A
code panel inside a callout now keeps its own grey bar: the old `.sub.note .sbar`
rule lit every bar in the callout, the nested panel's too.

The docs chrome (sidebar, table of contents, search dialog) and the tiles' title
bars stay hand-built CSS: they are window chrome, not controls.
