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
- The derived colors, `ink-muted`, `line`, `red` and the dark `rule`, are generated with Adobe's Leonardo (`@adobe/leonardo-contrast-colors`, WCAG 2 contrast) as a scale from warm-gray and red key colors, solved against `page` in each theme. Targets: `ink-muted` 6:1, dark `rule` 3.4:1, `line` 1.4:1, `red` 5.2:1 light and 5.6:1 dark. To change one, change its ratio and regenerate; do not hand-edit the lightness.
- Checked on `page` and `raised` in both themes: `ink` 14:1 or more, `ink-muted` 5.6:1 or more, `red` 5.2:1 or more, `rule` 3.2:1 or more (light `rule` is `ink`), `on-orange` on `orange` 5:1 or more. `orange` itself is 2.96:1 on the light `page`, kept to match the logo, so it is only ever a fill; text and focus rings use `orange-ink`.
- Use WCAG 2 ratios as the gate; for text on orange or small body text, also eyeball it, since APCA would rate light-on-dark text more strictly.

## Using the colors

- Page and app ground is `bg-background`; cards and popovers `bg-card`; text `text-foreground`, secondary text `text-muted-foreground`.
- Light `rule` equals `ink`, so frames and control borders read as black ink; dark `rule` is a mid grey.
- `primary` (orange) marks whatever has focus or the one main action per view. Its text is always `primary-foreground` (near-black), never white. Orange as text or a link hover on light grounds is `orange-ink` (`text-orange-ink`), never `orange`. Fumadocs' search dialog sets its match highlight and result titles in `fd-primary` (orange); `global.css` overrides those to `orange-ink`, so `fd-primary` stays a fill. The override exists because Fumadocs uses one slot for both fill and text; recheck it when Fumadocs is upgraded. The orange link underline on article links is a decoration (the text is `ink`) and is also exempt; large display type (24px and up) may use `orange`.
- Controls take a heavy `border-input` border; panels and separators a thin `border-border`.
- Focus is the `ring` color as a solid ring, no glow. Hover and active states change border or ring, not hue.
- Errors: `destructive`, with a word or icon as well as color.

## Type

- Three families only; there is no fourth (workspace numerals are mono). The `--font-tu-*` variables in `:root` hold the stacks; `@theme` maps them to `font-display`, `font-sans` and `font-mono`. `display` (Archivo Variable, weight 900, width 125%, uppercase) for headlines at `display-xl` / `display-lg`. `sans` (DM Sans) for body: `text-base`, `text-sm`, `text-caption`. `mono` (IBM Plex Mono) for labels, nav, keycaps and code: `label` is uppercase and letter-spaced.
- Write the name lowercase, `tauler`; the capitals come from the display style.
- The fonts are loaded from npm (`@fontsource`), not vendored.

### Type scale

Defined in `@theme` in `global.css`; use these utilities, never `text-[Npx]`. `text-xs` is deliberately not redefined: it stays Tailwind's 12px, because Fumadocs' search dialog uses it (result breadcrumbs, group headers, filter chips) and 11px muted DM Sans is too small to read there. Our own 11px step has its own name, `text-caption`.

| Style | Size | Utility | Use |
| --- | --- | --- | --- |
| `caption` | 11px | `text-caption` | Group labels, table heads, fine print |
| `label` | 12px | `text-label` | Mono labels, buttons, header and footer |
| `label-strong` | 13px | `text-label-strong` | Title bars, table of contents |
| `text-sm` | 14px | `text-sm` | Navigation, code, tables, inline code, callouts |
| `text-base` | 16px | `text-base` | Body text |
| lead | 20px | `text-xl` | Page lead |
| `display-lg` | 24px | `text-display-lg` | Section headings |
| `display-xl` | 40 to 64px (`clamp`) | `text-display-xl` | Page title |

## Shape and spacing

- Page width is one token, `--width-page` (1320px), used as `max-w-page`. Spacing is Tailwind's 4px scale (`space-1` … `space-12`); use Tailwind's `p-*`, `gap-*` utilities. No half-steps (`gap-3.5`) and no arbitrary px; 2px is allowed for hairline offsets only (inline code padding). **Exception:** the 88px workspace numerals on `principles/page.draft.tsx` have no scale step, because they exist only on a draft page. Add a step when that page ships.
- `radius` is 0, so every shadcn `rounded-*` is square. The one exception is keycaps (`radius-key`, 4px).
- Borders: heavy (1.5px) for top-level tiles, the header and footer rules, and controls; thin (1px) for anything nested inside a tile: sub-windows, mini windows, callouts such as alerts, and separators. Both are utilities (`border-heavy`, `border-t-heavy`, `border-b-heavy`, `border-l-heavy`, `border-thin`) and the variables `--border-heavy` and `--border-thin`. No other widths, except the keycap's 3px foot.
- No shadows or gradients, and that includes third-party components: the search dialog's shadow is removed in `global.css`. Depth is rules and the 4px checker hatch (`line` on `page`, or `orange-deep` on `orange` when focused); the hatch is 4px everywhere.
- Tiles: a `card` panel with a heavy `rule` frame and a hatched title bar holding a `label-strong` mono title. The focused tile gets an orange frame and bar. **Exception:** the orange frame on the light page is 2.96:1, under the 3:1 border rule. The frame is not the only cue: the lit tile also has the orange title bar and its text, and the frame passes in dark, so it is exempt. Do not use `orange-ink` for it; that would turn a bright accent into a dull one.

## Logo

Use `assets/Logos/tauler-logo.svg` (light colors baked in). Do not redraw it, recolor the orange cell, or crop its grid rules.

## Iconography

No icon set. Use Lucide (shadcn's default) at 1.5px stroke when an icon is needed, in `foreground` or `muted-foreground`; otherwise mono text.


## Base colours

Light on `:root`, dark on `.dark`. `orange-ink` is `{orange}` in dark.

| Token | Light | Dark | Use |
| --- | --- | --- | --- |
| `page` | `oklch(0.919 0.008 91.5)` | `oklch(0.177 0.002 106.6)` | Page ground. Text on it: ink, ink-muted. |
| `raised` | `oklch(0.961 0.005 95.1)` | `oklch(0.226 0.004 106.7)` | Panels, cards, popovers and keycaps raised over page. Text on it: ink, ink-muted. |
| `ink` | `oklch(0.178 0.000 0.0)` | `oklch(0.939 0.007 97.4)` | Primary text and heavy rules on page and raised; also the logo ground. |
| `ink-muted` | `oklch(0.435 0.008 88.7)` | `oklch(0.666 0.007 97.4)` | Secondary text on page and raised (6.2:1 light, 6.2:1 dark on page). Generated with Leonardo against page. |
| `line` | `oklch(0.805 0.003 84.6)` | `oklch(0.313 0.005 91.6)` | Thin hairlines and separators. Decorative; never carries text. Generated with Leonardo against page. |
| `rule` | `oklch(0.178 0.000 0.0)` | `oklch(0.525 0.008 88.7)` | Heavy 1.5px rules and control borders. Light: same as ink (the printed look); dark: generated with Leonardo against page at 3.4:1. |
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

## Changing a colour

1. Edit the base value in `global.css`; never give a shadcn slot its own value.
2. The derived colours (`ink-muted`, `line`, `red`, `orange-ink`, dark `rule`) come from Adobe's Leonardo
   (`@adobe/leonardo-contrast-colors`, WCAG 2) solved against `page`: change the ratio, regenerate, do not
   hand-edit lightness.
3. Check every text pair on `page` and `raised` in both themes: `ink` 7:1, `ink-muted`, `red` and
   `orange-ink` 4.5:1, `rule` 3:1.
