# Design system tests

Results of the checks behind ADR 0050, run 2026-11-11 against Tailwind 4.3, Leonardo 1.1.0 and
apca-w3 0.1.9. Re-run them before relying on any of these in a different version.

## How the shadcn set was produced

The registry host (`ui.shadcn.com`) was not reachable, so the CLI was not run. The base component
sources and the style stylesheets were fetched from shadcn's public GitHub repository
(`apps/v4/registry/bases/base/ui/*.tsx` and `apps/v4/registry/styles/style-{lyra,vega,nova}.css`).
The sources carry placeholder classes (`cn-button`); each style maps them to real utilities with
`@apply`, and the CLI substitutes them when it adds a component. The substitution was reproduced
with a script. 433 rules resolved. 68 placeholders have no rule and stay in the markup as hooks.
This is a close reproduction of `shadcn add`, not the CLI itself.

## Spacing

| Question | Result |
| --- | --- |
| Does `--spacing-*: initial` restrict numeric utilities? | Yes: `p-3`, `p-5` and `gap-7` stop generating while listed steps work. Arbitrary values (`p-[13px]`) still generate. |
| Should it be used? | No. The namespace also drives sizing, so `h-*`, `size-*` and `w-*` utilities outside the list disappear and shadcn components break. 209 numeric sizing utilities across 50 distinct values appear in the set. |
| How much of a fresh shadcn set is off an allowlist of 0, 0.5, 1 to 8, 12, 16? | 75 of 393 padding, margin and gap uses (19%): `1.5` (37), `2.5` (30), `3.5`, `18`, `28`, and about ten arbitrary values. 79 uses are half-steps. |
| What does the lint need? | A script: oxlint has no Tailwind plugin. `scripts/check-spacing.mjs` scans `src/` and exempts `components/ui`. Run it with `--root <dir>` to scan another tree with no exemption. |

Our own code needed three changes to pass: a 44px and a 40px CSS value and one `mt-10`, snapped to 48px,
48px and `mt-12` (4 to 8px more space in each).

## Colour

| Question | Result |
| --- | --- |
| Does Leonardo output OKLCH directly? | No. `output: 'OKLCH'` returns malformed strings (`oklch(1, 0, 9141%)`). OKLCH works only as the interpolation space. Take HEX and convert. |
| Does it solve against the colour you give it? | Not exactly. Its `lightness` setting moves the background it solves against (asked `#e6e4de`, got `#e3e1db`). Re-measure every value against the real page, as `scripts/colors.mjs` does. |
| Does `formula: 'wcag3'` agree with a reference APCA implementation? | Yes, to within 0.2 Lc at Lc 45, 60 and 75, on a light and a dark page. Lc 90 is unreachable on the light page: it returns black, which measures 88.4. |
| APCA thresholds (Lc 75 body, 60 content, 45 large)? | Not verified. They are used as advice only. |
| What does APCA say about our dark mode? | Dark `ink-muted` is 6.22:1 but Lc 44, dark `red` is 5.8:1 but Lc 42, dark `rule` is Lc 24. Light mode is closer to its advisory floors. |

## Finish: what a token can restyle without editing components

Each of the 53 components was classified for lyra (the style this site uses), vega and nova.

| Property | Follows a token? | Evidence |
| --- | --- | --- |
| Colour | Yes | Utilities read the semantic variables. |
| Font family | Yes | `font-sans` compiles to `var(--font-sans)`. Only 2 components name a family; the rest inherit from the body. |
| Radius | Depends on the style | `rounded-md` compiles to `var(--radius-md)`. **lyra hardcodes `rounded-none`** (131 uses, 43 components), so a radius token does nothing there. vega and nova follow the token (100 and 101 uses, 35 components). |
| Shadow | Yes, at build time | `--shadow-sm` overridden to `0 0 0 9px red` shows up inside the compiled `shadow-sm` rule. 15 to 32 uses in 11 to 25 components, depending on style. The value is inlined at build, so changing it needs a rebuild. |
| Border width | **No** | `border`, `border-t` and the like compile to a literal `1px` (45 to 46 uses in 31 to 32 components). An `@utility border` override does not replace it: both declarations are emitted and the built-in `1px` comes last and wins. |
| Ring width | **No** | Literal (`ring-1`, `ring-3`, about 70 uses in 31 components). |

So colour, font and shadow can be re-skinned without touching `components/ui`. Radius can if the style
follows tokens, which lyra does not. Border and ring width cannot, which is why the heavy rule
(`border-heavy`) is applied by editing the component and logging it.

## Amendments to the handover brief

- Leonardo's OKLCH output cannot be used; convert from HEX.
- Add a text-safe variant of the brand colour (see ADR 0050).
- Restrict spacing by lint on padding, margin and gap, not through `--spacing-*`.
- "Leave `components/ui` untouched" becomes "edit only when a token cannot do it, and log it".
