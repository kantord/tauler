// Chromatic aberration for non-text content (the flow-field canvas, the
// GitHub icon) — text-shadow has no effect there, so it needs its own
// treatment. The first version used filter: drop-shadow() twice, which
// composites its colored copies with plain source-over (stacking). On
// SPARSE content (glyphs, lots of empty space) that reads as a clean
// per-edge fringe. On the flow field's thousands of overlapping
// semi-transparent lines it visibly brightened/washed out — source-over
// stacking has no ceiling, so opacity keeps accumulating wherever the
// warm and cool copies both land on already-dense art.
//
// The fix is mathematical, not just a lower opacity: screen blending
// (1 - (1-a)(1-b)) has a hard ceiling at full white no matter how many
// layers stack, so recombining the offset/tinted copies with `screen`
// instead of `over` self-limits on dense content instead of just
// tuning the symptom down. Built as a hand-assembled SVG filter
// (feOffset + feColorMatrix + feBlend) since CSS drop-shadow has no way
// to change its own internal compositing mode.
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
          1 0 0 0 0
          0 0.45 0 0 0
          0 0 0.15 0 0
          0 0 0 1 0" result="warmTint" />
        <feOffset id="crt-fringe-cool-offset" in="SourceGraphic" dx="0" dy="0" result="coolOffset" />
        <feColorMatrix in="coolOffset" type="matrix" values="
          0.15 0 0 0 0
          0 0.45 0 0 0
          0 0 1 0 0
          0 0 0 1 0" result="coolTint" />
        <feBlend in="warmTint" in2="coolTint" mode="screen" result="fringe" />
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
