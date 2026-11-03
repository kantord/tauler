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
(CC0, credited on the page anyway), then reduced to dithered pixel art with the
wallpaper's palette rules. It runs the page it came from, not an imitation:
`docs/scripts/vendor-whiteroom.sh` takes the page's zip, checks that its copies of the
renderer's GLSL chunks and blue noise are byte-identical to upstream at a pinned commit,
and copies the page's five shaders and its scene description unchanged, but for one
small patch to the scene (below). three.js is r185 from npm, the revision the page ships. Only the page's render loop is ported by hand, into
`src/components/site/whiteroom/whiteroom.ts`, without its tweak panel and drag controls.

The render does not fill the window: the standard wallpaper does, as on the docs pages,
and the room fills the bottom half of the window on top of it, edge to edge, fading in
from transparent over its top 20%. These pages drop the site's `scrollbar-gutter: stable`
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
