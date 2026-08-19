// Experimental: bends the REAL hero content (not just the scanline
// overlay), via an SVG feDisplacementMap filter on <main>. Off by
// default, opt-in only through the tuning panel — see crt-panel.ts.
//
// Why this is safe to try where a naive full-page warp wouldn't be:
// filter is a pure paint-time effect, it never moves a click target's
// actual hit-test geometry. Displacing DOCS/GITHUB or the command line
// even a few px would make them visually not where they're clickable.
// So the displacement map here is ~0 in a "safe zone" covering the
// hero's real content column (SAFE_ZONE_PX, with a soft ramp past it)
// and only grows in the empty background area to the right, where nothing
// is interactive. Below MIN_WIDTH_PX there's no meaningful empty
// background to warp into, so the filter isn't applied at all.
//
// The displacement itself reuses the scanlines' geometry: distance from
// an off-screen point above the viewport, signed by which side of the
// vertical middle a pixel falls on, so the content that IS warped bends
// the same direction the scanline mesh already does.

const SAFE_ZONE_PX = 650
const RAMP_PX = 250
const MIN_WIDTH_PX = 900
// 512, not the 128 this started at — a low-res displacement map shows up
// as visible blockiness in the safe-zone ramp specifically (each map
// pixel there is several real screen pixels wide). A hand-crafted
// reference (codepen.io/cauners/pen/ExMaqOW) uses a 600×600 pre-baked
// PNG for the same feDisplacementMap technique; 512 is the same order
// of magnitude without a meaningfully heavier per-pixel build loop.
const MAP_SIZE = 512

let currentScale = 40

function buildDisplacementMap(width: number): string {
  const canvas = document.createElement('canvas')
  canvas.width = MAP_SIZE
  canvas.height = MAP_SIZE
  const ctx = canvas.getContext('2d')
  if (!ctx) return ''

  const img = ctx.createImageData(MAP_SIZE, MAP_SIZE)
  const midY = MAP_SIZE / 2
  const safeZoneMap = (SAFE_ZONE_PX / width) * MAP_SIZE
  const rampMap = (RAMP_PX / width) * MAP_SIZE

  for (let y = 0; y < MAP_SIZE; y++) {
    for (let x = 0; x < MAP_SIZE; x++) {
      const dy = y - midY
      const safety = Math.min(
        1,
        Math.max(0, (x - safeZoneMap) / rampMap),
      )
      // Signed, capped displacement: grows away from the vertical
      // middle, scaled to fit a single byte around a mid-gray zero point.
      const raw = Math.max(-127, Math.min(127, dy * 0.35))
      const value = 128 + raw * safety
      const i = (y * MAP_SIZE + x) * 4
      // R carries the real (Y) displacement data. G is held at a flat
      // 128 — neutral, zero displacement — and is what xChannelSelector
      // reads. Using R for both, as the first version did, drove X and Y
      // displacement from the same value: every pixel sheared diagonally
      // instead of bending vertically, which read as a shifted duplicate
      // of the page rather than a curve.
      img.data[i] = value
      img.data[i + 1] = 128
      img.data[i + 2] = 128
      img.data[i + 3] = 255
    }
  }
  ctx.putImageData(img, 0, 0)
  return canvas.toDataURL('image/png')
}

let installed = false

function ensureFilter(width: number): void {
  const existing = document.getElementById('crt-warp-filter-root')
  if (existing) existing.remove()

  const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg')
  svg.id = 'crt-warp-filter-root'
  Object.assign(svg.style, {
    position: 'absolute',
    width: '0',
    height: '0',
    overflow: 'hidden',
  })
  svg.innerHTML = `
    <defs>
      <filter id="crt-page-warp" x="-10%" y="-10%" width="120%" height="120%">
        <feImage href="${buildDisplacementMap(width)}" result="warpmap" preserveAspectRatio="none" x="0%" y="0%" width="100%" height="100%" />
        <feDisplacementMap id="crt-page-warp-disp" in="SourceGraphic" in2="warpmap" scale="${currentScale}" xChannelSelector="G" yChannelSelector="R" />
      </filter>
    </defs>
  `
  document.body.appendChild(svg)
  installed = true
}

function apply(on: boolean): void {
  const main = document.querySelector('main')
  if (!(main instanceof HTMLElement)) return
  if (!on || window.innerWidth < MIN_WIDTH_PX) {
    main.style.filter = ''
    return
  }
  if (!installed) ensureFilter(window.innerWidth)
  main.style.filter = 'url(#crt-page-warp)'
}

export function setPageWarp(on: boolean): void {
  apply(on)
  if (on) {
    addEventListener('resize', () => {
      installed = false
      apply(true)
    })
  }
}

// Live-tunable: scale is a plain SVG attribute on the existing
// <feDisplacementMap>, not a CSS custom property, so this updates it
// directly rather than rebuilding the filter or the displacement map —
// cheap enough to wire straight to a slider's input event.
export function setPageWarpStrength(scale: number): void {
  currentScale = scale
  const disp = document.getElementById('crt-page-warp-disp')
  disp?.setAttribute('scale', String(scale))
}
