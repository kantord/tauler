# Edits to shadcn components

Every file in this folder that came from `shadcn add` and was then edited is
logged here: what differs from upstream, why, and when. Brand through tokens
first (`global.css`); edit a component only for structure a token cannot
express. `scripts/check-ui-log.mjs` fails CI if a component file is missing
from this log, so a new or edited component has to be written down.

Upstream is shadcn's `base-lyra` style on Base UI. To see the divergence, run
`shadcn add <name> --overwrite` into a scratch copy and diff it. Checked against shadcn 4.23.0 on
2026-11-14: the log below covered the main edits of `button.tsx` and `alert.tsx`; the rows dated that day
add what a real diff showed was missing. Upstream imports `cn` from the `cn` package. Both files import it
from `@/lib/utils`, and the files are formatted with oxfmt (single quotes, trailing commas), so a raw diff is noisy.

`icon.tsx` is the site's own component, not shadcn's, and is not logged.

## button.tsx

| Date | Edit | Why |
| --- | --- | --- |
| 2026-11-07 | Variants cut to `default`, `outline`, `outline-primary` and `nav`; sizes to `default`, `lg`, `inline` and `icon`. | The site has four kinds of control and no `secondary`, `ghost`, `destructive` or `link`. |
| 2026-11-07 | Uppercase, mono, wide tracking, 2px rule border, solid orange focus outline. (Upstream is already square.) | Structure of the design's controls; none of it is a token. The width is the theme default (`--default-border-width`), so the class is plain `border`. |
| 2026-11-07 | `nav` variant: the current page (`aria-current="page"`) is lit. | Header links and workspace keys. |
| 2026-11-11 | Text and hover colour use `orange-ink`, not `primary`; the `icon` size is mono, not Oswald. | `primary` (orange) is 3.36:1 as text on light; Oswald left the system. |
| 2026-11-14 | Removed the `default` variant's hover fill, the `aria-invalid` styles, the dark-mode variants, `active` press, `transition-all` and `bg-clip-padding`; added `cursor-pointer`, `gap-2` and `no-underline` to the base. | Controls are flat and have no invalid state; the dark theme comes from the tokens. |
| 2026-11-14 | The `lg` size is 52px high, sentence case; `inline` is a new borderless size. Sizes `xs`, `sm`, `icon-xs`, `icon-sm` and `icon-lg` removed. | A call to action and a bare word in a line of text. |
| 2026-11-14 | `buttonVariants` is now a function that merges classes with `cn`; the variants object is private (`variants`). `className` is a string only, not Base UI's function form. | A link styled as a button takes `buttonVariants()`, and `size` and `className` must win over the base `border`. |
| 2026-11-25 | The label type is the `text-label` utility alone; `font-mono`, `tracking-[.08em]` and the tracking token are gone from the base classes. | Letter-spacing is part of a text style (see DESIGN.md). |
| 2026-11-25 | New `keycap` variant (4px corners via `rounded-key`, heavy border, card fill). | The "on this page" heading bar below 768px is a keycap; `--radius-key` is the design system's one rounded corner. |


## alert.tsx

| Date | Edit | Why |
| --- | --- | --- |
| 2026-11-07 | One `default` variant; `AlertAction` and the grid layout removed. The title is a hatched title bar (`.sbar`) and the frame is orange. | Docs callouts are sub-windows, not shadcn's inline alert. The hatch is a CSS background that no token expresses. |
| 2026-11-11 | Title text uses `orange-ink`; the frame is `border-thin`. | Contrast on light; nested panels use the thin rule. |
| 2026-11-14 | `AlertDescription` is `px-4 py-4 text-sm leading-[1.6]` in the inherited text colour; muted colour, `text-balance` and link underline styles removed. The `destructive` variant, icon handling (`*:[svg]` rules, `has-[>svg]` columns) and `rounded-none`, `px-2.5 py-2` and `text-xs` on the frame are gone. | The callout body reads as prose, and the docs use no icons or destructive callouts. |
