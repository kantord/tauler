# Site styling is layered and machine-checked

The docs site's landing page is built from three component layers — text
atoms (`src/components/text/`, typography and text color), ui atoms
(`src/components/ui/`, surfaces, hairlines, spacing, and their own text), and
organisms (`src/components/organisms/`, compositions owning internal layout
plus section chrome) — while pages may only compose with layout utilities.
A `templates/` layer for page-level skeletons is reserved but deliberately
not created until something needs it. `scripts/check-class-layers.ts`
enforces the boundaries in CI with per-layer class allowlists, and bans
`rounded-*` and blur/backdrop utilities everywhere: radius 0 and "never blur
what you cover" are design invariants, not preferences.

Enforcement is a bespoke script, not a linter plugin, because nothing
off-the-shelf can do this today: Biome's `.astro` template support is
experimental and its `useSortedClasses` rule sorts by a fixed heuristic that
cannot see our custom-token utilities, and an ESLint + astro-parser stack
would bring a second lint ecosystem for one rule. The same reasoning picked
Prettier (with the Astro and Tailwind plugins) for formatting: it is the only
formatter that handles `.astro` templates maturely, and its class sorter
reads the real `@theme`. Revisit Biome when it formats Astro templates
stably and `useSortedClasses` understands theme-defined utilities.
