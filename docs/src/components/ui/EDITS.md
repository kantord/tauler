# Edits to shadcn components

Every file in this folder that came from `shadcn add` and was then edited is
logged here: what differs from upstream, why, and when. Brand through tokens
first (`global.css`); edit a component only for structure a token cannot
express. `scripts/check-ui-log.mjs` fails CI if a component file is missing
from this log, so a new or edited component has to be written down.

Upstream is shadcn's `base-lyra` style on Base UI. To see the divergence, run
`shadcn add <name> --overwrite` into a scratch copy and diff it.

`icon.tsx` is the site's own component, not shadcn's, and is not logged.

## button.tsx

| Date | Edit | Why |
| --- | --- | --- |
| 2026-11-07 | Variants cut to `default`, `outline`, `outline-primary` and `nav`; sizes to `default`, `lg`, `inline` and `icon`. | The site has four kinds of control and no `secondary`, `ghost`, `destructive` or `link`. |
| 2026-11-07 | Square, uppercase, mono, wide tracking, 1.5px rule border, solid orange focus outline. | Structure of the design's controls; none of it is a token. The width is `border-heavy`. |
| 2026-11-07 | `nav` variant: the current page (`aria-current="page"`) is lit. | Header links and workspace keys. |
| 2026-11-11 | Text and hover colour use `orange-ink`, not `primary`; the `icon` size is mono, not Oswald. | `primary` (orange) is 3.36:1 as text on light; Oswald left the system. |

## alert.tsx

| Date | Edit | Why |
| --- | --- | --- |
| 2026-11-07 | One `default` variant; `AlertAction` and the grid layout removed. The title is a hatched title bar (`.sbar`) and the frame is orange. | Docs callouts are sub-windows, not shadcn's inline alert. The hatch is a CSS background that no token expresses. |
| 2026-11-11 | Title text uses `orange-ink`; the frame is `border-thin`. | Contrast on light; nested panels use the thin rule. |
