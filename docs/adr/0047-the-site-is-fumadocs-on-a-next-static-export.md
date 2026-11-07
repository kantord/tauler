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

> Superseded by "the wallpaper is a shader anchored to the page" below: the wallpaper now
> scrolls with the page, on a window-sized canvas that moves with it. The canvas size
> limit still rules out a page-sized canvas.

Code blocks are highlighted with the design's own palette (`src/lib/shiki-theme.ts`),
dark in both site themes. Every axe rule runs again; the four the stock theme failed
were switched off only while the site used it.

Formatting and linting moved to oxc: `oxfmt` (Prettier-compatible, with Tailwind class
sorting) replaces Prettier, and `oxlint` with the TypeScript, React, Next.js and import
plugins runs in Docs CI. ADR 0030 picked Prettier because it was the only mature
formatter for `.astro` templates; with no `.astro` files left, that reason is gone.

## Amendment: the home page is a background, the landing content is on slides

The home page is only the top bar, the logo and a background; the landing content is on
three slide pages (`/overview/`, `/principles/`, `/get-started/`), one per header
workspace button, which share the background. The slides are drafts (below), so the
published home page links to none of them.

The home page background is decided in ADR
[0048](0048-the-home-background-is-a-path-traced-room-drawn-with-the-wallpaper-as-one-pixel-art-image.md).

## Amendment: shadcn/ui as a foundation, not a component set

`docs/components.json` sets up shadcn/ui with the Base UI flavour (`base-lyra`), so
`shadcn add` and `fumadocs add` place components in `src/components/ui` and reuse each
other's primitives, on the Base UI that Fumadocs already ships rather than a second
primitives library. A component is copied in only when a page needs it (the first were
`Button` and `Alert`, in "the site's controls are themed shadcn components" below).
shadcn's theme variables in `global.css` all point at the `--tu-*` tokens and
`--radius` is 0, so whatever is added comes out in the design's colours, fonts and square
corners. Two of the init's additions are left out on purpose: the `@layer base` rule
(it would restyle every border and the body) and `shadcn/tailwind.css` (it redefines
`data-open:` and friends, which changes Fumadocs' own selectors; Base UI components work
with Tailwind's built-in variants).

This does not contradict the choice above not to vendor Fumadocs' layout. That layout is
thousands of lines we would own for no benefit; a shadcn primitive is a small, accessible
component that our tokens already theme, and we own only the ones we use.

## Amendment: the wallpaper is a shader anchored to the page

The wallpaper is a WebGL 2 fragment shader
(`src/components/site/wallpaper/wallpaper-gl.ts`), and it is anchored to the page, so
it scrolls with the content and a long page shows more of the pattern. This replaces the
viewport-fixed host above.

The page-sized canvas ruled out above is still ruled out. One canvas the
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
"background" pixels were a pinkish `rgb(233, 226, 228)` on the `#e6e4de` page.
`WALLPAPER` (`wallpaper/options.ts`) is the one source of the options for the module
and the shader.

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
