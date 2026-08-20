// Chromatic aberration for non-text content (the flow-field canvas, the
// GitHub icon) — text-shadow has no effect there, so it needs its own
// treatment.
//
// History, because two prior attempts here both turned out wrong on
// measurement, not just taste:
//
//   v1 — filter: drop-shadow() x2 (plain source-over stacking).
//        Measured (ImageMagick mean luminance, same canvas region,
//        aberration off vs on): baseline 19.9 → 23.4 (+17%).
//
//   v2 — switched to feBlend mode="screen", on the theory that
//        screen's ceiling (1-(1-a)(1-b), capped at white) would
//        self-limit brightening on dense content. Measured: 23.7
//        (+19%) — statistically the same as v1, not better. The
//        theory was wrong for THIS content: the flow field is sparse
//        hairlines with lots of empty space, not dense overlapping
//        fills, so there's little actual stacking for a ceiling to
//        cap in the first place.
//
//   v3 — tightened the tint matrices' channel leaks AND switched the
//        final composite to `overlay` (gated by the backdrop's own
//        tone, so dark background stays dark). Measured: 21.6 (+12%,
//        a real improvement) — but visually turned everything reddish.
//        Root cause: overlay's "screen" branch engages per-channel
//        wherever the BACKDROP is already bright in that channel. The
//        flow field is orange — backdrop RED is high almost
//        everywhere there's a visible line — and the warm fringe copy
//        was ALSO built to be nearly pure red (matrix kept R at full
//        strength). Two independently-reasonable choices collided
//        specifically because of this content's own dominant hue,
//        which the numeric brightness measurement never would have
//        caught (it's a hue problem, not an intensity problem).
//
// v4 (current): back to `screen` for the final composite — the
// measured-safe baseline, not a new theory. Strength is controlled
// directly and predictably via feComponentTransfer scaling the tinted
// copies' ALPHA down before compositing, rather than via blend-mode
// tricks whose behavior depends on the specific content's own colors.
// This is duller science than v2/v3 but it's the one lever that
// doesn't have a content-dependent failure mode: less alpha in, less
// light out, regardless of what hue the backdrop happens to be.
//
// color-interpolation-filters is set to sRGB explicitly — SVG filters
// default to linearRGB, which would shift these colors from how the
// rest of the page's oklch-based palette actually looks.

let installed = false

function ensureFilter(): void {
  if (installed) return
  const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg')
  svg.id = 'crt-canvas-fringe-root'
  Object.assign(svg.style, {
    position: 'absolute',
    width: '0',
    height: '0',
    overflow: 'hidden',
  })
  svg.innerHTML = `
    <defs>
      <filter id="crt-canvas-fringe" color-interpolation-filters="sRGB" x="-15%" y="0%" width="130%" height="100%">
        <feOffset id="crt-fringe-warm-offset" in="SourceGraphic" dx="0" dy="0" result="warmOffset" />
        <feColorMatrix in="warmOffset" type="matrix" values="
          0.75 0 0 0 0
          0 0.2 0 0 0
          0 0 0.1 0 0
          0 0 0 1 0" result="warmTint" />
        <feComponentTransfer in="warmTint" result="warmTintDim">
          <feFuncA type="linear" slope="0.45" intercept="0" />
        </feComponentTransfer>
        <feOffset id="crt-fringe-cool-offset" in="SourceGraphic" dx="0" dy="0" result="coolOffset" />
        <feColorMatrix in="coolOffset" type="matrix" values="
          0.1 0 0 0 0
          0 0.2 0 0 0
          0 0 0.75 0 0
          0 0 0 1 0" result="coolTint" />
        <feComponentTransfer in="coolTint" result="coolTintDim">
          <feFuncA type="linear" slope="0.45" intercept="0" />
        </feComponentTransfer>
        <feBlend in="warmTintDim" in2="coolTintDim" mode="screen" result="fringe" />
        <feBlend in="SourceGraphic" in2="fringe" mode="screen" />
      </filter>
    </defs>
  `
  document.body.appendChild(svg)
  installed = true
}

export function setCanvasFringe(px: number): void {
  ensureFilter()
  document
    .getElementById('crt-fringe-warm-offset')
    ?.setAttribute('dx', String(px))
  document
    .getElementById('crt-fringe-cool-offset')
    ?.setAttribute('dx', String(-px))
}
