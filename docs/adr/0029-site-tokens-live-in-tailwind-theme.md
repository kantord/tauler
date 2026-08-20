# Site tokens live in a Tailwind @theme block

The docs site's design values — type ramp, ink and surface colors, hairlines,
spacing — come from the tauler design package, which instructs implementers to
copy its `tokens.css` verbatim and never rename a token. We broke that rule:
the canonical home of the tokens is now the `@theme` block in
`docs/src/styles/global.css`, using Tailwind v4's namespaces
(`--color-ink-1`, `--text-display-xl`, `--spacing-s4`), and `tokens.css` is
gone.

Tailwind v4's CSS-first configuration made the verbatim file a second source
of truth: every token would exist once in the design package's names and again
in the namespace Tailwind derives utilities from, and nothing would hold the
two in sync. Renaming at the boundary keeps one authority and gives every
value a working utility class for free. The cost is real and accepted: a
future design-package update is a manual translation into the `@theme` block,
not a wholesale copy. The token set is also pruned to what the site uses —
the five unused workspace accents, the two shadows, and the motion tokens
return when the full scroll-story page needs them, translated the same way.

The Playwright visual baselines pin the translation: they were generated from
the hand-written CSS and passed unchanged against the Tailwind rebuild.
