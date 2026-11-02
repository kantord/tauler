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

The axe check runs with four rules off (`color-contrast`, `region`, `landmark-unique`,
`landmark-no-duplicate-banner`) that the stock theme fails out of the box. Turn each back
on as the theme is customised.
