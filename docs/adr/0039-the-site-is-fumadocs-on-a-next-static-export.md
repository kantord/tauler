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
own to what the design actually draws. The design's generative wallpaper canvas was not
ported: its module was not part of the exported file.

Code blocks are highlighted with the design's own palette (`src/lib/shiki-theme.ts`),
dark in both site themes. Every axe rule runs again; the four the stock theme failed
were switched off only while the site used it.

Formatting and linting moved to oxc: `oxfmt` (Prettier-compatible, with Tailwind class
sorting) replaces Prettier, and `oxlint` with the TypeScript, React, Next.js and import
plugins runs in Docs CI. ADR 0030 picked Prettier because it was the only mature
formatter for `.astro` templates; with no `.astro` files left, that reason is gone.
