# Design system tests

Results of the checks behind ADR 0050, run 2026-11-11 against Tailwind 4.3, Leonardo 1.1.0 and
apca-w3 0.1.9. Re-run them before relying on any of these in a different version.

## How the shadcn set was produced

Re-run 2026-11-14 with the real CLI. The registry (`ui.shadcn.com`) was reachable, so the first run
(a rebuild from shadcn's GitHub sources, because the registry was not reachable that day) is
replaced. A scratch Vite and Tailwind 4.3.5 project per style, a `components.json` like ours with
`style` set to `base-lyra`, `base-vega` or `base-nova`, then
`pnpm dlx shadcn@latest add --all -y -o` (shadcn 4.23.0). Each run wrote 63 components, 10 more than the
53 of the first run (`attachment`, `bubble`, `code-block`, `direction`, `marker`, `message`,
`message-scroller`, `questionnaire`, `toast`, `trace`). Leaving those 10 out reproduces the first
run's numbers closely, so the figures below are for the 53 unless a row says otherwise. Each count
is `rg`-style pattern matching over the class strings: it is not a script in this repo.

## Spacing

| Question | Result |
| --- | --- |
| Does `--spacing-*: initial` restrict numeric utilities? | Yes: `p-3`, `p-5` and `gap-7` stop generating while listed steps work. Arbitrary values (`p-[13px]`) still generate. |
| Should it be used? | No. The namespace also drives sizing, so `h-*`, `size-*` and `w-*` utilities outside the list disappear and shadcn components break. 209 numeric sizing utilities across 50 distinct values appear in the set. |
| How much of a fresh shadcn set is off an allowlist of 0, 0.5, 1 to 8, 12, 16? | lyra, 53 components: 75 of 394 uses (19%): `1.5` (36), `2.5` (31), `3.5`, `18`, `28`, and a few arbitrary values. 80 uses are half-steps. vega: 92 of 407 (23%). nova: 110 of 413 (27%). All 63 components, lyra: 95 of 488 (19%). |
| What does the lint need? | A script: oxlint has no Tailwind plugin. `scripts/check-spacing.mjs` scans `src/` and exempts `components/ui`. Run it with `--root <dir>` to scan another tree with no exemption. |

Our own code needed three changes to pass: a 44px and a 40px CSS value and one `mt-10`, snapped to 48px,
48px and `mt-12` (4 to 8px more space in each).

## Colour

| Question | Result |
| --- | --- |
| Does Leonardo output OKLCH directly? | No. `output: 'OKLCH'` returns malformed strings (`oklch(1, 0, 9141%)`). OKLCH works only as the interpolation space. Take HEX and convert. |
| Does it solve against the colour you give it? | Not exactly. Its `lightness` setting moves the background it solves against (asked `#e6e4de`, got `#e3e1db`). Re-measure every value against the real page, as `scripts/colors.mjs` does. |
| Does `formula: 'wcag3'` agree with a reference APCA implementation? | Yes, to within 0.2 Lc at Lc 45, 60 and 75, on a light and a dark page. Lc 90 is unreachable on the light page: it returns black, which measures 88.4. |
| APCA thresholds (Lc 75 body, 60 content, 45 large)? | Checked against [APCA in a Nutshell](https://git.apcacontrast.com/documentation/APCA_in_a_Nutshell.html). 75, 60 and 45 are the published levels, but with conditions: 75 is for body text at 18px/400 or larger, 60 for content text that is not body text, 45 for large heavy text (36px/400 or 24px/700). Non-text elements such as control borders are Lc 30 (15 is the absolute floor), so `rule` is now checked against 30, not 45. Our 16px/400 prose is below the 18px/400 row, so its true target is above 75. Advice only; the checker exits 0. |
| What does APCA say about our dark mode? | Dark `ink-muted` is 6.22:1 but Lc 44, dark `red` is 5.8:1 but Lc 42, dark `rule` was Lc 24 at 3.4:1, below the Lc 30 floor for control borders; it is now generated at 4.4:1 (Lc 31 on `page`, 30 on `raised`). Light mode is closer to its advisory floors. |

## Finish: what a token can restyle without editing components

Each of the 53 components was classified for lyra (the style this site uses), vega and nova. Uses
are class-string matches in `components/ui`.

| Property | Follows a token? | Evidence |
| --- | --- | --- |
| Colour | Yes | Utilities read the semantic variables. |
| Font family | Yes | `font-sans` compiles to `var(--font-sans)`. Only 2 components name a family; the rest inherit from the body. |
| Radius | Depends on the style | `rounded-md` compiles to `var(--radius-md)`. **lyra hardcodes `rounded-none`** (122 uses, 40 components), so a radius token does nothing there. vega and nova follow the token (100 and 101 uses, 35 components) and use `rounded-none` in 11 uses. |
| Shadow | Yes, at build time | `--shadow-sm` overridden to `0 0 0 9px red` shows up inside the compiled `shadow-sm` rule. 17 to 34 uses in 11 to 25 components, depending on style. The value is inlined at build, so changing it needs a rebuild. |
| Border width | **Yes, through `--default-border-width`**, not through a `border` utility override (see below) | `border`, `border-t` and the like compile to the theme value, `1px` by default (44 to 47 uses in 29 to 32 components). Setting `--default-border-width: 1.5px` in `@theme` turned all but one `1px` width in the compiled lyra set into `1.5px`; the one left is preflight's `hr`. |
| Ring width | **No** | `ring-1`, `ring-3` and the like are literal (64 to 65 uses in 26 components). `--default-ring-width` changes only a bare `ring`. |

### Border width override

Measured with Tailwind 4.3.5 (the site's version), on a minimal page and on the lyra set. The
question is whether the class `border` ends up 1.5px without editing a component.

| Approach | `border` becomes 1.5px? | Notes |
| --- | --- | --- |
| `@utility border { border-width: 1.5px }` | Yes, for `border` and for variants (`after:border`, `md:border`) | The built-in `.border` is still emitted; the override is emitted after it (a separate rule for the plain class, a later declaration in the same rule for variants), so it wins. It fails if the override also declares `border-style` first: then the built-in's `1px` came last in the one test run. Which order you get depends on the declared properties, so check the output. `border-t`, `border-x` and the other sides stay `1px` until each gets its own override. |
| Unlayered `.border { border-width: 1.5px }` | Yes, plain `border` only | An unlayered rule beats every layered rule. Variants such as `md:border` and `after:border` stay `1px`, and it also beats `border-0` if both classes are present. |
| Custom layer after utilities (`@layer theme, base, components, utilities, overrides;`) | Yes, plain `border` only | Same as unlayered, with the same gaps for variants. |
| `@theme { --default-border-width: 1.5px }` | **Yes, for every width utility without a value** | `border`, `border-x`, `border-y`, `border-s`, `border-e`, `border-t`, `border-r`, `border-b`, `border-l` and variants of them, and `divide-x` and `divide-y`. `border-0`, `border-2` and the like keep their literal values, and so does preflight's `border: 0 solid` reset. No component edit is needed. Side effect: every `border` in the page, ours and Fumadocs', becomes 1.5px, so anything that wants 1px has to say so (`border-thin`). |
| `@theme { --border-width-heavy: 1.5px }` | No, it adds utilities | Generates `border-heavy`, `border-{x,y,s,e,t,r,b,l}-heavy` and `divide-{x,y}-heavy` reading `var(--border-width-heavy)`. `border` is unchanged. This could replace the hand-written `@utility border-heavy` in `global.css`. |

Minimal case for the approach that changes every width utility:

```css
@import 'tailwindcss';
@theme { --default-border-width: 1.5px; }
/* output: .border { border-style: var(--tw-border-style); border-width: 1.5px; } */
/* output: .border-t { border-top-style: ...; border-top-width: 1.5px; } */
```

So colour, font, shadow and border width can be re-skinned without touching `components/ui`. Radius
can if the style follows tokens, which lyra does not. Ring width (`ring-1`, `ring-3`) cannot. The
site uses the theme route: `--default-border-width: 2px` in `global.css` makes every `border` the heavy
rule, and the shadcn components carry the upstream `border` class again. The nested thin rule is
`border-thin`, a logged edit in `alert.tsx`.

Border widths are whole pixels (heavy 2px, thin 1px). The first version used 1.5px, which Chromium lays out as 1px at 1x, 2x and 3x device scale (a 50px box with a 1.5px border measures 52px wide, with a 2px border 54px), so heavy and thin looked the same there. Firefox and WebKit were not tested. The examples above use 1.5px because that is the value the test used; the mechanism is the same for 2px.

## Amendments to the handover brief

- Leonardo's OKLCH output cannot be used; convert from HEX.
- Add a text-safe variant of the brand colour (see ADR 0050).
- Restrict spacing by lint on padding, margin and gap, not through `--spacing-*`.
- "Leave `components/ui` untouched" becomes "edit only when a token cannot do it, and log it".
