# Docs site design-system audit

Audited 2026-11-10 against `docs/DESIGN.md`, ADR 0049, `docs/src/app/global.css`, `CONTEXT.md` and
`AGENTS.md`. Mechanical checks with ripgrep, every hit read in context; WCAG 2 ratios computed from
the oklch values; the site built (`cargo run -p tauler-docgen`, `pnpm run build`) and the home page,
a docs page and the search dialog screenshotted in light and dark.

## Scope and limits

- **Out of scope:** the original design the tokens were derived from; the wallpaper and room code
  (`wallpaper*`, `room/`); the Rust UI.
- **Contrast pass limits:** the computed-style pass walked text on the home page, `/docs/layout-file/`
  and `/docs/elements/`. It did **not** cover typed search results (the match highlight only exists
  after a query), hover states, or the draft pages. Search-result contrast was found by looking at a
  screenshot and computing the ratio by hand.
- **Drafts are not in the build**, so draft findings come from reading the code.
- **Home credit line** sits on the path-traced room; its real background cannot be computed, only
  checked against the bare page colour.
- `content/` hits (`text-green-500`, `text-white`, `text-[11px]`, `gap-[8px]`) are Tauler layout
  examples in fenced code, which `global.css:7` excludes from the site's Tailwind. Not findings.

## A. The doc is wrong

These are errors in `DESIGN.md` or comments, not code that bypasses a token.

| # | Where | Problem | Fix |
|---|---|---|---|
| D1 | `DESIGN.md` `@theme` snippet | `--font-display: var(--font-display)` is circular, so it resolves to nothing. `global.css` has no `--font-display` in `@theme` either; the real variables are `--font-tu-display/text/mono`. | Fix the snippet to match the real names (or rename the variables to `--font-*`). |
| D2 | `DESIGN.md` type section | `text-xs` is listed as 11px; Tailwind's is 12px and `@theme` has no `--text-xs`. The scale steps (display-xl, display-lg, label, label-strong) have no px values anywhere. | Define the scale in `@theme` (decision 1). |
| D3 | `DESIGN.md:81`, `(home)/page.tsx:29` | `ink-muted` stated as 5.3:1 light, 6.5:1 dark. Real: 6.24 light, 6.22 dark on page. | Correct the numbers. |
| D4 | `global.css:11` | Comment says "twelve base colours"; there are eighteen. | Correct the comment. |
| D5 | `DESIGN.md` Shape | `border-heavy` and `border-thin` are named but never defined. | Define them (decision 4). |

## B. Code bypasses a token that exists

### Contrast and orange

| # | Where | What | Fix | Kind |
|---|---|---|---|---|
| B1 | `get-started/Terminal.tsx:14` | `text-orange-ink` `$` on the code ground: 2.60:1 in light. A regression from switching `$` to `orange-ink`. Hidden only because of B2. **Most urgent.** | `text-code-orange` (5.98:1). | draft |
| B2 | `global.css` `@theme inline` | No `--color-code*`, although ADR 0049 says base tokens are Tailwind colours. `bg-code` and `text-code-ink` in `Terminal.tsx:12` generate nothing. **Inferred from the CSS, not rendered.** | Add `--color-code`, `--color-code-ink`, `--color-code-muted/soft/orange/orange-soft/yellow`. | draft |
| B3 | `global.css:84`, Fumadocs search dialog | `--color-fd-primary` is `orange`; the dialog's `text-fd-primary` (match highlight, result titles) is orange text: 3.36:1 on raised, 2.96:1 on page in light. Seen in a screenshot. | Keep `fd-primary` as the fill; override the dialog's text uses to `orange-ink`. | shared, published |
| B4 | `alert.tsx:42`, `(home)/page.tsx:83,86` | Arbitrary `[background:…var(--primary)…]` and `size-(--hero)` instead of utilities. | Move the hatch to a class. | shared |

### Shape

| # | Where | What | Fix |
|---|---|---|---|
| B5 | search dialog | `shadow-2xl` (measured `0 25px 50px -12px`). The system has no shadows. | Remove. |
| B6 | `global.css:285` | `.tile.on` fakes a second ring with `box-shadow`. | Replace with a border width (done as an `outline`, so layout does not shift). |
| B7 | `(home)/page.tsx:36` | `rounded-sm` (no-op, radius is 0) and `backdrop-blur-[2px]`. | Remove `rounded-sm`; decide on the blur. |
| B8 | `global.css:532` | `.toc a` border 2px. | 1.5px. |
| B9 | `global.css:633` | `blockquote` border 3px. | 1.5px. |
| B10 | `global.css:433` | `.hero-strip` hatch 8px; the system's hatch is 4px. | 4px. |
| B11 | `SiteHeader.tsx:36`, `.wslabel::after` | Heavy width (1.5px) in the thin color (`line`). | Pick one pairing. |
| B12 | `Terminal.tsx:10`, `alert.tsx:9` | `border-rule` at 1px; `border-primary` at 1px. Colour and width disagree. | Heavy rule is 1.5px. |

Kept on purpose: keycap 4px radius and its 3px bottom edge (in the spec); conic-gradient hatches
(allowed).

### Type and spacing

- Off-scale sizes: 15px (`.nav` `global.css:505`, `.art h3/h4` `:597`, alert body `alert.tsx:58`,
  `Terminal.tsx:13`); 17px (`.art p/ul/ol` `:603,618`, `.pn` `:554`); 20px `.lead`; 24px `.art h2`;
  h1 `clamp(40px,4.4vw,64px)`; drafts: 19px, 26px, 88px, two `clamp`s. `0.86em` inline code.
- On-scale but written as arbitrary px: `text-[12px]` (`button.tsx:12`, `SiteHeader.tsx:25,70`,
  `DocsTiles.tsx:57`), `text-[16px]` (`button.tsx:32`), `text-[11px]` (`page.tsx:36`), `text-[13px]`
  (drafts).
- `text-xs` renders 12px (`ScreenshotGallery.tsx:37`).
- `global.css` has 37 declarations off the 4px grid: 6px ×8, 14px ×6, 10px ×5, 18px ×4, 3px ×2,
  7px ×2, 9px ×2, 1px ×2, 5px ×2, 11px, 26px, 38px, 42px. TSX: `[18px]` in `alert.tsx:58`,
  `SiteHeader.tsx:28,71`, `Terminal.tsx:12`; 14 half-step utilities (`gap-3.5`, …);
  `max-w-[1320px]` ×6.

### Focus

`global.css` has no `:focus-visible` rule. Only `Button` gets the `ring` outline; `.nav`, `.pn`,
`.toc a`, `.art a` and `button.bar` fall back to the browser default.

## C. Exceptions to document

- **Lit-tile orange frame** (`.tile.on`, `.sub.on`, alert frame): 2.96:1 on the light page, under the
  3:1 border rule; passes in dark. Decorative, paired with an orange title bar.
- **Orange link underline** (`global.css:657`): 2.96:1 on page, a decoration, the text is ink.

## D. Checked and clean

- No literal colors outside the token block; no Tailwind palette classes in site code.
- Three font families only, always through variables.
- Radius 0 everywhere in practice (dialog `rounded-xl` measured 0px); keycaps 4px.
- `ink`, `ink-muted`, `red`, `orange-ink`, `on-orange`, code colors, dark `rule` pass on `page` and
  `raised` in both themes.
- Hatches are the only gradients.
- Not traced: where `--color-fd-error` and `--color-fd-overlay` get their values.

## Counts

| Rule | Findings |
|---|---|
| Doc wrong | 5 |
| Color, orange, contrast | 4 (B1–B4) |
| Shape | 8 (B5–B12) |
| Type scale | 4 groups |
| Spacing | 37 CSS declarations + about 18 TSX uses |
| Focus | 5 elements |
| Documented exceptions | 2 |

## Decisions (made 2026-11-11)

1. Type scale defined in `@theme`: display-xl, display-lg, label (12), label-strong (13), 11, 14,
   16, 20, 24. 15→14, 17→16, 19→20, 26→24. `text-xs` becomes 11px. Screenshot a docs page before
   and after (the 17→16 prose change is visible).
2. Search highlight: `fd-primary` stays the fill; the dialog's text uses go to `orange-ink`.
3. Orange frame: exempt on light, document in `DESIGN.md`, do not darken.
4. `border-heavy` (1.5px) and `border-thin` (1px) become real utilities; replace `[1.5px]`.
5. Spacing snaps to the 4px scale; 2px allowed for hairline offsets; 6/10/14/18px decided case by
   case against screenshots.
6. Remove dialog `shadow-2xl`; `.toc` border to 1.5px; keep keycap 3px; hero-strip hatch 4px.

## Fix order

B1 and B2 first, then the doc errors (D1–D5), then decisions 1–6.

## Status after fixes (2026-11-11)

- **Done:** B1, B2, a second B1 case (the terminal's copy button), D1–D5, decisions 1–6.
- **Verified:** build, `format:check`, `lint`, `check` and `validate:html` pass. A computed-style
  contrast pass over home, `/docs/layout-file/` and `/docs/elements/` finds no text under 4.5:1 in
  either theme. The search highlight now measures `orange-ink`; the dialog shadow measures `none`;
  `/get-started/` (draft, via `next dev`) shows the code ground and a `code-orange` prompt.
- **Not verified:** `tests/console.spec.ts` could not run here (Playwright's headless shell,
  chromium 1254, is not installed). Run it where it is.
- **Left as is:** the 88px numerals in `principles/page.draft.tsx` (decorative display, no scale
  step); the `8vw` mobile h1 floor; `leading-[1.6]` arbitrary line heights; the focus-ring gap
  (finding "Focus") is not addressed; B4 (`alert.tsx` arbitrary background, `page.tsx` `--hero`
  utilities) and B7's `backdrop-blur-[2px]` are not addressed.
- **`cn` merger:** `src/lib/utils.ts` now registers the type-scale and `border-heavy/thin`
  utilities. Without it `tailwind-merge` drops `text-label` next to `text-foreground` and
  `border-heavy` next to `border-input`.

## Follow-up decisions (2026-11-11)

- `text-xs` stays Tailwind's 12px: Fumadocs' search dialog uses it, and 11px there was too small.
  Our 11px step is `text-caption`.
- The `fd-primary` text override carries a comment to recheck it on Fumadocs upgrades; the typed-query
  highlight measures `orange-ink` in light.
- Lit-tile frame exemption keeps its reason in `DESIGN.md`; alert border stays thin (1px, as before).
- Heavy for top-level tiles, header/footer rules and controls; thin for anything nested.
- `--width-page` (1320px) as `max-w-page`; the 88px numerals are a documented draft-page exception.
- `tests/console.spec.ts`: 19 of 20 pass. `/docs/component-reference/` 404s on `/tauler/tauler_web.js`
  because the wasm assets are not built here; the baseline 404s on its runtime files too.
