# The design system is enforced by checks, and shadcn components may be edited when logged

ADR 0049 chose the tokens. This one decides how they stay true, and how far brand reaches into
`src/components/ui`.

- **Colours are reproducible and gated.** `scripts/colors.mjs` generates the derived colours with
  Leonardo and, in CI (`pnpm run checks`), fails if a text pair in `global.css` misses WCAG 2 or a
  derived token drifts from the generator. Leonardo's own OKLCH output is malformed in 1.1.0, so
  the script takes HEX and converts with culori. APCA (`apca-w3`) is printed as advice and never
  fails: it is stricter in dark mode, where most of our text passes WCAG 2 but sits at Lc 40 to 50.
- **Spacing is linted, not restricted in Tailwind.** `scripts/check-spacing.mjs` allows a short list
  of padding, margin and gap steps (0, 0.5, 1 to 8, 12, 16) and px values in our CSS, and bans
  arbitrary ones. Restricting `--spacing-*` in the theme would also remove sizing utilities
  (`h-9`, `size-10`) that shadcn components use, so sizing is left alone. `components/ui` is exempt:
  a fresh shadcn set trips the allowlist 75 times in 393 uses (see `design-system-tests.md`).
- **shadcn components may be edited, and every edit is logged** in `src/components/ui/EDITS.md`,
  with `scripts/check-ui-log.mjs` failing CI for a component missing from it. What a token can
  express goes into tokens first (`border-heavy`, `border-thin`, radius, fonts, colours). The
  structural edits the site needs, such as the hatched title bar and uppercase mono controls, stay
  in the component and are logged. This replaces "leave `components/ui` untouched": it cannot hold,
  because the border width and ring width are literals in about 31 of 53 components and cannot be
  reached by a token.
- **The repo is canonical for tokens.** `global.css` is the only source. There is no `tokens.json`
  in the repo, and any copy kept in a design tool is a view generated from it, never edited by hand.
- **A text-safe variant of the brand colour is part of the system.** A bright brand colour fails as
  small text on a light ground (ours is 3.36:1), so the brand fill and the brand text colour are
  separate tokens (`orange`, `orange-ink`). A future brand-colour knob generates both.
- **Status colours are added when a component needs one.** There are no success, warning or info
  tokens until then.

Not decided here: a brand-colour knob, or extracting the system into a package. Both wait for a
second project.
