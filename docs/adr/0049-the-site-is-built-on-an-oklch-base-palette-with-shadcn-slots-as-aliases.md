# The site is built on an OKLCH base palette, with the shadcn slots as aliases

The docs site's colours are eighteen base tokens in OKLCH (`page`, `raised`, `ink`, `ink-muted`,
`line`, `rule`, `orange`, `orange-deep`, `orange-ink`, `on-orange`, `red`, the code ground and its
text, and five syntax colours), declared once in `docs/src/app/global.css`. Every shadcn and Fumadocs
slot (`--primary`, `--border`, `--ring`, ...) is an alias of a base token and never holds a value of
its own. The base tokens are also Tailwind colours (`bg-raised`, `text-ink-muted`), so components use
utilities and not arbitrary `(--var)` values. `docs/DESIGN.md` is the reference.

This replaces the hex `--tu-*` tokens, and supersedes the token layout in ADR 0029 (its
`@theme` block and file path are gone). The values were not redesigned: the anchors are exact
conversions of the old ones.

Three rules came out of it and are easy to undo by accident:

- **Orange is a fill.** On the light page the brand orange is 2.96:1, so text, link hovers and
  focus rings use `orange-ink`, a darker orange generated for 5:1. Large display type may still use
  `orange`.
- **Derived colours are generated, not tuned.** `ink-muted`, `line`, `red`, `orange-ink` and the dark
  `rule` are solved with Leonardo against `page`, because perceived contrast drifts when someone
  nudges a lightness by eye. The light `rule` is `ink` on purpose: the heavy rules are black ink.
- **The code panel is dark in both themes**, so its syntax colours are theme-independent
  `--code-*` tokens that the shiki theme reads as `var()`.

There are three font families: Archivo (display), DM Sans (text) and IBM Plex Mono. Oswald was dropped; the
workspace keys use mono. All three are SIL OFL and come from npm (ADR 0032).
