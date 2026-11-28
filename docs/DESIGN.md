# tauler design system

How the docs site looks and where each value lives. The site's tokens are
`docs/src/app/global.css`; this file says what they mean and how to use them. The
decision behind it is [ADR 0049](adr/0049-the-site-is-built-on-an-oklch-base-palette-with-shadcn-slots-as-aliases.md).

tauler's brand is a tiling window manager drawn in print: square panels, heavy ink rules, one orange accent, hatched title bars. It is built on Tailwind v4 and shadcn/ui: eighteen base colors in OKLCH, three font families, Tailwind's 4px spacing scale, and the full shadcn semantic slot set as aliases of the base colors.

## How the tokens layer

- **Base** (edit these): `page`, `raised`, `ink`, `ink-muted`, `line`, `rule`, `orange`, `orange-deep`, `on-orange`, `red`, `code`, `code-ink`, `orange-ink`, and five syntax colors (`code-muted`, `code-soft`, `code-orange`, `code-orange-soft`, `code-yellow`). Each has a light and a dark value.
- **Semantic** (never edit, never give values): the shadcn slots `background`, `foreground`, `card`, `popover`, `primary`, `secondary`, `muted`, `accent`, `destructive`, `border`, `input`, `ring`, `chart-1` to `chart-5`, and `sidebar-*`, each an alias of a base color. Components use only these.
- Dark mode is the `.dark` class on `<html>`; one theme is one mode.

Wire it in Tailwind v4 with the shadcn convention: declare base and semantic values as CSS variables on `:root` and `.dark`, then map them in an inline theme.

```css
@theme inline {
  --font-display: var(--font-tu-display); /* Archivo Variable */
  --font-sans: var(--font-tu-text); /* DM Sans Variable */
  --font-mono: var(--font-tu-mono); /* IBM Plex Mono */
  --color-background: var(--background);
  --color-foreground: var(--foreground);
  --color-primary: var(--primary);
  --color-primary-foreground: var(--primary-foreground);
  /* …one --color-* line per semantic slot, as in shadcn's template */
  --radius-sm: calc(var(--radius) * 0.6);
  --radius-lg: var(--radius);
}
```

## Color science

- Every color is `oklch()`. Lightness (L) is perceptual, so a step of 0.1 looks like the same step in either theme; chroma and hue stay put between themes.
- Brand anchors are exact conversions of the site and logo: `page`, `raised`, `ink`, `orange`, `orange-deep`, `on-orange`, `code`, `code-ink`.
- The derived colors, `ink-muted`, `line`, `red` and the dark `rule`, are generated with Adobe's Leonardo (`@adobe/leonardo-contrast-colors`, WCAG 2 contrast) as a scale from warm-gray and red key colors, solved against `page` in each theme. Targets: `ink-muted` 6:1, dark `rule` 4.4:1, `line` 1.4:1, `red` 5.2:1 light and 5.6:1 dark. To change one, change its ratio and regenerate; do not hand-edit the lightness.
- Checked on `page` and `raised` in both themes: `ink` 14:1 or more, `ink-muted` 5.6:1 or more, `red` 5.2:1 or more, `rule` 3.9:1 or more (light `rule` is `ink`), `on-orange` on `orange` 5:1 or more. `orange` itself is 2.96:1 on the light `page`, kept to match the logo, so it is only ever a fill; text and focus rings use `orange-ink`.
- Use WCAG 2 ratios as the gate; for text on orange or small body text, also eyeball it, since APCA would rate light-on-dark text more strictly. `scripts/colors.mjs` also prints APCA advice, never a failure, using the published levels: Lc 75 for body text columns (18px/400 and up), Lc 60 for other content text, Lc 45 for large heavy text, Lc 30 for control borders and other non-text elements. Our 16px/400 prose is below the 18px/400 row of that table, so its true target is higher than 75.

## Using the colors

- Page and app ground is `bg-background`; cards and popovers `bg-card`; text `text-foreground`, secondary text `text-muted-foreground`.
- Light `rule` equals `ink`, so frames and control borders read as black ink; dark `rule` is a mid grey.
- `primary` (orange) marks whatever has focus or the one main action per view. Its text is always `primary-foreground` (near-black), never white. Orange as text or a link hover on light grounds is `orange-ink` (`text-orange-ink`), never `orange`. Fumadocs' search dialog sets its match highlight and result titles in `fd-primary` (orange); `global.css` overrides those to `orange-ink`, so `fd-primary` stays a fill. The override exists because Fumadocs uses one slot for both fill and text; recheck it when Fumadocs is upgraded. The orange link underline on article links is a decoration (the text is `ink`) and is also exempt; large display type (24px and up) may use `orange`.
- Controls take a heavy `border-input` border; panels and separators a thin `border-border`.
- Focus is the `ring` color as a solid ring, no glow. Hover and active states change border or ring, not hue.
- Errors: `destructive`, with a word or icon as well as color.

## Type

- Three families only; there is no fourth (workspace numerals are mono). The `--font-tu-*` variables in `:root` hold the stacks; `@theme` maps them to `font-display`, `font-sans` and `font-mono`. `display` (Archivo Variable) is for headlines, `sans` (DM Sans) for body text, `mono` (IBM Plex Mono) for labels, nav, keycaps and code. The text styles below bundle them.
- Write the name lowercase, `tauler`; the capitals come from the display style.
- Letter-spacing is part of a text style, never a separate token or an arbitrary `tracking-[…]`. Sizing (`w-*`, `max-w-*`, flex bases) is exempt; font sizes and spacing are not.
- The fonts are loaded from npm (`@fontsource`), not vendored.

### Text styles

Five custom utilities in `global.css` (`@utility`), each bundling family, size, weight, case and letter-spacing. Colour and line-height stay with the element. Use these; never `text-[Npx]` or `tracking-*`. `cn()` (`src/lib/utils.ts`) registers them as font styles so tailwind-merge does not read them as colours.

| Utility | Style | Use |
| --- | --- | --- |
| `text-display` | Archivo 900, width 125%, uppercase, 40 to 64px (`clamp`) | Page title |
| `text-heading` | Archivo 900, width 125%, uppercase, 24px | Section headings |
| `text-body` | DM Sans 400, 16px | Body text |
| `text-label` | IBM Plex Mono 500, uppercase, 12px, 0.1em tracking | Header, footer, buttons, breadcrumb, title bars, group labels, table heads |
| `text-code` | IBM Plex Mono 400, 14px | Code |

`text-sm` (14px), `text-xl` (20px, page lead) and `text-caption` (11px, fine print that is not a label) stay as plain sizes. `text-xs` is deliberately not redefined: it stays Tailwind's 12px, because Fumadocs' search dialog uses it and 11px muted DM Sans is too small to read there.

## Shape and spacing

- Page width is one token, `--width-page` (1320px), used as `max-w-page`. Spacing is Tailwind's 4px scale (`space-1` … `space-12`); use Tailwind's `p-*`, `gap-*` utilities. No half-steps (`gap-3.5`) and no arbitrary px; 2px is allowed for hairline offsets only (inline code padding). **Exception:** the 88px workspace numerals on `principles/page.draft.tsx` have no scale step, because they exist only on a draft page. Add a step when that page ships.
- `radius` is 0, so every shadcn `rounded-*` is square. The one exception is keycaps (`radius-key`, 4px).
- Borders: heavy (2px) for top-level tiles, the header and footer rules, and controls; thin (1px) for anything nested inside a tile: sub-windows, mini windows, callouts such as alerts, and separators. Heavy is Tailwind's default width here (`--default-border-width` in `global.css`), so a plain `border`, `border-t` or `border-b` is heavy, in shadcn components too. Thin is `border-thin` (and `border-t-thin` and the like). Both widths are variables, `--border-width-heavy` and `--border-width-thin`. No other widths, except the keycap's 3px foot.
- The framed `.sub` panel is for code blocks and callouts only; navigation never uses it. The sidebar is flat: mono group labels with a hairline, 36px rows, and the current page in bold ink with a 2px orange left edge instead of an orange fill.
- No shadows or gradients, and that includes third-party components: the search dialog's shadow is removed in `global.css`. Depth is rules and the 4px checker hatch (`line` on `page`, or `orange-deep` on `orange` when focused); the hatch is 4px everywhere. One exception: below 768px a gradient mask fades the wallpaper band and a `page`-coloured halo surrounds the title and lead, both only to keep text legible (see Narrow screens).
- Tiles: a `card` panel with a heavy `rule` frame and a hatched title bar holding a `text-label` title. The focused tile gets an orange frame and bar. **Exception:** the orange frame on the light page is 2.96:1, under the 3:1 border rule. The frame is not the only cue: the lit tile also has the orange title bar and its text, and the frame passes in dark, so it is exempt. Do not use `orange-ink` for it; that would turn a bright accent into a dull one.

## Logo

Use `assets/Logos/tauler-logo.svg` (light colors baked in). Do not redraw it, recolor the orange cell, or crop its grid rules.

## Iconography

No icon set. Use Lucide (shadcn's default) at a 2px stroke (the heavy rule width) when an icon is needed, in `foreground` or `muted-foreground`; otherwise mono text.


## Base colours

Light on `:root`, dark on `.dark`. `orange-ink` is `{orange}` in dark.

| Token | Light | Dark | Use |
| --- | --- | --- | --- |
| `page` | `oklch(0.919 0.008 91.5)` | `oklch(0.177 0.002 106.6)` | Page ground. Text on it: ink, ink-muted. |
| `raised` | `oklch(0.961 0.005 95.1)` | `oklch(0.226 0.004 106.7)` | Panels, cards, popovers and keycaps raised over page. Text on it: ink, ink-muted. |
| `ink` | `oklch(0.178 0.000 0.0)` | `oklch(0.939 0.007 97.4)` | Primary text and heavy rules on page and raised; also the logo ground. |
| `ink-muted` | `oklch(0.435 0.008 88.7)` | `oklch(0.666 0.007 97.4)` | Secondary text on page and raised (6.2:1 light, 6.2:1 dark on page). Generated with Leonardo against page. |
| `line` | `oklch(0.805 0.003 84.6)` | `oklch(0.313 0.005 91.6)` | Thin hairlines and separators. Decorative; never carries text. Generated with Leonardo against page. |
| `rule` | `oklch(0.178 0.000 0.0)` | `oklch(0.587 0.006 84.6)` | Heavy 2px rules and control borders. Light: same as ink (the printed look); dark: generated with Leonardo against page at 4.4:1, so it clears APCA Lc 30 for control borders on `page` and `raised`. |
| `orange` | `oklch(0.632 0.186 39.2)` | `oklch(0.703 0.195 40.2)` | brand anchor (matches the logo). Fills only: primary buttons, focused title bars, the logo cell. Never small text or a focus ring on light (use orange-ink); 2.96:1 on the light page. |
| `orange-deep` | `oklch(0.541 0.159 38.5)` | `oklch(0.577 0.160 40.8)` | Hatch partner of orange on a focused title bar; chart accent. |
| `orange-ink` | `oklch(0.500 0.181 33.9)` | `{orange}` | Orange as text, link hover and focus ring: 5.2:1 on page, 5.9:1 on raised in light; the brand orange itself in dark (6:1+). Light value generated with Leonardo. |
| `on-orange` | `oklch(0.178 0.000 0.0)` | `oklch(0.178 0.000 0.0)` | Text on an orange fill, both themes. |
| `red` | `oklch(0.491 0.166 27.2)` | `oklch(0.661 0.112 26.7)` | Errors and destructive actions; 4.5:1 or more on page and raised. Generated with Leonardo against page. |
| `code` | `oklch(0.226 0.002 106.6)` | `oklch(0.200 0.002 106.6)` | Code block ground, dark in both themes; text on it: code-ink. |
| `code-ink` | `oklch(0.961 0.008 91.5)` | `oklch(0.939 0.007 97.4)` | Text on code blocks. |
| `code-muted` | `oklch(0.624 0.012 89.8)` | `oklch(0.624 0.012 89.8)` | syntax colour, same in both themes. Comments and punctuation on code (4.75:1). |
| `code-soft` | `oklch(0.776 0.013 91.5)` | `oklch(0.776 0.013 91.5)` | syntax colour, same in both themes. Attributes and operators on code (8.4:1). |
| `code-orange` | `oklch(0.703 0.195 40.2)` | `oklch(0.703 0.195 40.2)` | syntax colour, same in both themes. Keywords on code (6:1). |
| `code-orange-soft` | `oklch(0.762 0.150 43.6)` | `oklch(0.762 0.150 43.6)` | syntax colour, same in both themes. Numbers and constants on code (7.6:1). |
| `code-yellow` | `oklch(0.846 0.169 84.9)` | `oklch(0.846 0.169 84.9)` | syntax colour, same in both themes. Strings on code (10.5:1). |

## Spacing allowlist

Padding, margin and gap use Tailwind steps 0, 0.5 (hairlines only), 1 to 8, 12 and 16, plus `px` and `auto`.
In our CSS the px values are 0, 1, 2, 4, 8, 12, 16, 20, 24, 28, 32, 48 and 64. Arbitrary values are not
allowed. Sizing (`h-*`, `size-*`, `w-*`, `max-w-*`) is not restricted. `pnpm run check:spacing` enforces
this and `components/ui` is exempt. Add `spacing-ok: reason` to a line for a documented exception.

## Checks

`pnpm run checks` runs three design-system checks: `check:colors` (contrast of every text pair, and the
derived colours against the generator), `check:spacing`, and `check:ui-log` (every shadcn component is
listed in `src/components/ui/EDITS.md`). This file and `global.css` are canonical; any token file kept in
a design tool is generated from them. ADR 0050 explains the choices and `design-system-tests.md` has the
evidence.

## Brand colour as text

A bright brand colour is a fill. Text, link hovers and focus rings use its text-safe variant
(`orange-ink`). A new brand colour needs both.

## Changing a colour

1. Edit the base value in `global.css`; never give a shadcn slot its own value.
2. The derived colours (`ink-muted`, `line`, `red`, `orange-ink`, dark `rule`) come from Adobe's Leonardo
   (`@adobe/leonardo-contrast-colors`, WCAG 2) solved against `page`. Change the ratio in
   `scripts/colors.mjs`, run `node scripts/colors.mjs --print`, and paste the values; do not hand-edit
   lightness.
3. Check every text pair on `page` and `raised` in both themes: `ink` 7:1, `ink-muted`, `red` and
   `orange-ink` 4.5:1, `rule` 3:1.

## Narrow screens

- Below 768px the header keeps the logo, a search icon button, a GitHub icon button and the menu button (right end), each a 40px square. From 768px it has the site links (the current one with an orange edge under it, never a fill), the search field (centred from 1024px), "Star on GitHub" and the theme button; once content scrolls beneath it a faint 4px checker hatch hangs under its rule.
- The menu opens a drawer from the right: a "contents" title bar with a close button, "Star on GitHub" beside a two-way sun and moon switch, then the site links and the contents. The header stays visible beside it.
- "On this page" is a heading bar under the page's lead: a left-aligned keycap button until it sticks, then a full-width strip with a progress ring and the heading being read. It sticks at `top: 57px`, because the header drops from a 2px to a 1px bottom rule while the bar is stuck (58px to 57px). The bar keeps one height in both states so sticking moves no content.
- Below 640px the article is frameless with 16px side padding. From 640px it is a tile with 32px padding; from 768px (with the sidebar docked) 24px, and 32px again from 1024px.
- Below 768px the wallpaper is only a band about 270px tall behind the title, faded from the top right, and the title and lead carry a page-coloured halo. Both exist only for legibility over the wallpaper.
- From 768px the contents and "on this page" are two sticky tiles in one column (204 to 248px wide, `clamp(204px, 24vw, 248px)`): the contents take the height that is left, "on this page" at most half. From 1024px "on this page" is a right-hand tile of up to 300px. A contents title too long for its row ends in an ellipsis.
- The "on this page" list, docked or dropdown, is a 2px `line` rail with link rows (32px docked, 40px dropdown); sub-headings are stepped in and muted; the heading being read is bold and its rail orange.
- The search dialog is 600px wide and 80px from the top from 768px, and 12px from the edges on a phone. A result is a title, its path and one line of text.
