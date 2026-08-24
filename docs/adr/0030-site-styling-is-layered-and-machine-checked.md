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

## Amendment: `[data-physical-object]` may round its own corners

Radius-0 is still the default for everything — panels, buttons, code
blocks, all UI chrome. The one exception is an element carrying
`data-physical-object`: a hardware mockup (currently just
`DeviceMockup.astro`'s laptop frame) standing in for a real object on a
desk, not another piece of interface. Every other panel on the landing page
(`CodePanel`, `ScreenMock`) is also sharp-cornered, which is exactly the
problem — a sharp-cornered device mockup reads as *more panel chrome*, not
as a distinct physical thing sitting in front of the screen it wraps.
Rounding is the signal that tells them apart; it's semantic, not decorative.

The mechanism is two rules in the same `design` layer, declared after
`* { border-radius: 0; }`, keyed by an explicit `data-physical-object` value
rather than its bare presence — a real laptop doesn't round symmetrically,
so neither does this: the lid rounds only its top corners (away from the
hinge), the base rounds only its bottom corners (the front edge resting on
the desk), and the hinge line where the two meet stays flush on both sides:

```css
[data-physical-object='lid'] {
  border-radius: var(--radius-device) var(--radius-device) 0 0;
}
[data-physical-object='base'] {
  border-radius: 0 0 var(--radius-device) var(--radius-device);
}
```

Cascade layers only order-break across layers; two rules in the same layer
still resolve by ordinary specificity, and an attribute selector
(0,0,1,0) beats the universal selector (0,0,0,0) regardless of which comes
second. No `!important` needed, so nothing here weakens the guarantee that
Starlight's own CSS still loses everywhere it isn't explicitly named.

`data-physical-object` is a plain attribute, not a Tailwind class, for the
same reason `[data-reveal]` already is: it sidesteps
`check-class-layers.ts`'s `rounded-*` ban on purpose, because it isn't a
utility class in the first place — it's this repo's existing convention for
one-off, non-utility CSS. That's also what keeps this narrow: getting
radius requires an explicit, named opt-in on a specific element, not a
class anyone can reach for. Reaching for `data-physical-object` on
anything that isn't standing in for a real object is the wrong tool, the
same way reaching for `rounded-*` would be.
