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
the designer's own module (first-party, as ADR 0032 already counted it), vendored
unchanged in `src/lib/wallpaper/` and kept outside lint and format so it stays diffable
against the next drop. It is mounted on a viewport-fixed host, not on the page as its
README shows: a page-sized canvas costs width × height × dpr² × 4 bytes and the longest
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
same source the wallpaper uses (`WALLPAPER` in `src/components/site/wallpaper/options.ts`). It runs the page it came from, not an imitation:
`docs/scripts/vendor-whiteroom.sh` takes the page's zip, checks that its copies of the
renderer's GLSL chunks and blue noise are byte-identical to upstream at a pinned commit,
and copies the page's five shaders and its scene description unchanged, but for one
small patch to the scene (below). three.js is r185 from npm, the revision the page ships. Only the page's render loop is ported by hand, into
`src/components/site/whiteroom/whiteroom.ts`, without its tweak panel and drag controls.

The render does not fill the window: the standard wallpaper does, as on the docs pages,
and the room fills the bottom half of the window on top of it, edge to edge. These pages drop the site's `scrollbar-gutter: stable`
(`<Wallpaper edgeToEdge />`): Chrome clips fixed elements at the root's gutter, so the
render and wallpaper could not otherwise reach the window's right edge.

The render's aspect therefore runs from about 0.9:1 (a phone held upright) to 12:1 (a
short, very wide window) instead of the scene's own 3:2. The camera keeps the page's rule:
a 40° vertical view, widened horizontally to fit, and on boxes narrower than 3:2 the 3:2
horizontal view, widened vertically. Those wider and taller views see past the scene,
which was built for 3:2: beyond the left end of its back wall and floor, and over its
walls. So `scripts/whiteroom-scene.patch`, applied by the vendoring script and failing it
if it no longer applies, runs the back wall, its window band and the floor further left
and raises the walls; it adds no ceiling or left wall, which would shut out the open sky
that lights most of the room. At 3:2 the room looks as before.

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

The designer's module (`src/lib/wallpaper/tauler-wallpaper.js`) stays, as the
reference and the fallback. The shader draws its pixels byte for byte for the same
options, colours, device pixel ratio and page position, and `tests/wallpaper.spec.ts`
holds it to that; the palette, the hue lattice (a dynamic-programming table) and each
row's and column's lattice weights are computed with the module's own code on the CPU
and uploaded, so the shader's single precision touches little. Where the shader cannot
run the module draws the same slices on the CPU: no WebGL 2, a lost context, and software
WebGL, which is slower than the CPU path here (headless Chromium's SwiftShader, so CI
draws the wallpaper on the CPU, and checks the shader on SwiftShader only in the tests
that ask for it).

The module is no longer vendored unchanged. Three changes, marked `[tauler]` in it:
it can compute a slice of the page by itself (`col0`, `row0`), it exports the internals
the shader reuses, and one deliberate deviation from the designer's file: the
background level is the background colour itself. The designer's file drew it at the
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

Gone with it: the vendored pixel shader (`fs-pix.glsl`, no longer copied by the
vendoring script) and the four load-time edits that adapted it (window grid, wallpaper
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

The room has a plant: a Monstera deliciosa in a terracotta pot at the left of the scene
(the 8th and last group the renderer takes, at x −383), where every wide frame shows
it, clear of the logo above and of the fade, and the page's own 3:2 frame and phones do
not reach. It stands over the end of the uplighter's cable, which used to stop on the
open floor; the cable's last metre is rerouted to end behind the saucer.

It goes in the way the room's other changes do. The scene change is a second patch,
`scripts/whiteroom-monstera.patch`, applied by the vendoring script after the first
and failing it the same way; the plant's generator is the site's own code
(`whiteroom/monstera-scene.js`), which the patch imports. The shader side is load-time
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
of the home page at 1440 × 900: about 3.7 ms before, 4.7 ms with the plant (2560 × 1440:
8.8 and 11.3); the 48 frames still take about 0.8 s, bound by the display's refresh.

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
