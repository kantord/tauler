// Experimental: bends the REAL hero content (not just the scanline
// overlay), via an SVG feDisplacementMap filter on <main>. Off by
// default, opt-in only through the tuning panel — see crt-panel.ts.
//
// Why this is safe to try where a naive full-page warp wouldn't be:
// filter is a pure paint-time effect, it never moves a click target's
// actual hit-test geometry. Displacing DOCS/GITHUB or the command line
// even a few px would make them visually not where they're clickable.
//
// Falloff model: distance from the nearest of the TWO RIGHT corners of
// <main> (top-right, bottom-right) — not a single left/right split.
// tauler's hero content clusters near BOTH the top-left (the eyebrow
// label) and the bottom-left (the meta line) of the hero, so a plain
// vertical safe-zone boundary (the first version) protected the middle
// of the left edge but not its actual top/bottom corners specifically.
// Anchoring from the two corners that are genuinely always empty
// background does. Two radii per corner, the way CSS border-radius
// needs two lengths per corner for an ellipse:
//   - STRONG_RADIUS_PX: full displacement strength inside this distance
//   - WEAK_RADIUS_PX: fades from full strength down to WEAK_FLOOR
//     between STRONG_RADIUS_PX and here, then to exactly 0 beyond it
// WEAK_FLOOR is intentionally non-zero — a wide, faint halo that CAN
// graze the edge of a clickable element is an explicit, deliberate
// choice here (not a bug to eliminate), as long as it stays subtle.
//
// The displacement itself reuses the scanlines' geometry: distance from
// an off-screen point above the viewport, signed by which side of the
// vertical middle a pixel falls on, so the content that IS warped bends
// the same direction the scanline mesh already does.

const STRONG_RADIUS_PX = 500
const WEAK_RADIUS_PX = 950
const WEAK_FLOOR = 0.22
const MIN_WIDTH_PX = 900
// 512, not the 128 this started at — a low-res displacement map shows up
// as visible blockiness in the falloff specifically (each map pixel
// there is several real screen pixels wide). A hand-crafted reference
// (codepen.io/cauners/pen/ExMaqOW) uses a 600×600 pre-baked PNG for the
// same feDisplacementMap technique; 512 is the same order of magnitude
// without a meaningfully heavier per-pixel build loop.
const MAP_SIZE = 512

let currentScale = 40

function buildDisplacementMap(width: number, height: number): string {
  const canvas = document.createElement('canvas')
  canvas.width = MAP_SIZE
  canvas.height = MAP_SIZE
  const ctx = canvas.getContext('2d')
  if (!ctx) return ''

  const img = ctx.createImageData(MAP_SIZE, MAP_SIZE)
  const midY = MAP_SIZE / 2
  const mapScaleX = MAP_SIZE / width
  const mapScaleY = MAP_SIZE / height
  const strongMap = STRONG_RADIUS_PX * ((mapScaleX + mapScaleY) / 2)
  const weakMap = WEAK_RADIUS_PX * ((mapScaleX + mapScaleY) / 2)
  // The two corners this anchors from, in map space.
  const corners = [
    { x: MAP_SIZE, y: 0 },
    { x: MAP_SIZE, y: MAP_SIZE },
  ]

  for (let y = 0; y < MAP_SIZE; y++) {
    for (let x = 0; x < MAP_SIZE; x++) {
      let dist = Infinity
      for (const c of corners) {
        const dx = x - c.x
        const dyC = y - c.y
        dist = Math.min(dist, Math.sqrt(dx * dx + dyC * dyC))
      }
      let strength: number
      if (dist <= strongMap) {
        strength = 1
      } else if (dist >= weakMap) {
        strength = 0
      } else {
        const t = (dist - strongMap) / (weakMap - strongMap)
        strength = 1 - t * (1 - WEAK_FLOOR)
      }

      const dy = y - midY
      // Signed, capped displacement: grows away from the vertical
      // middle, scaled to fit a single byte around a mid-gray zero point.
      const raw = Math.max(-127, Math.min(127, dy * 0.35))
      const value = 128 + raw * strength
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

function ensureFilter(width: number, height: number): void {
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
      <filter id="crt-page-warp" x="0%" y="0%" width="100%" height="100%">
        <feImage href="${buildDisplacementMap(width, height)}" result="warpmap" preserveAspectRatio="none" x="0%" y="0%" width="100%" height="100%" />
        <feDisplacementMap id="crt-page-warp-disp" in="SourceGraphic" in2="warpmap" scale="${currentScale}" xChannelSelector="G" yChannelSelector="R" />
      </filter>
    </defs>
  `
  document.body.appendChild(svg)
  installed = true
}

// Real bug this had: every call to setPageWarp(true) registered a NEW
// resize listener that unconditionally called apply(true), and none
// were ever removed — toggling on/off/on stacked duplicate listeners,
// and toggling OFF didn't stop them. Resize the window after unchecking
// the box and a stale listener would call apply(true) anyway, silently
// re-enabling the warp against what the (now-unchecked) checkbox showed.
// Fixed by tracking the desired state in one place and registering the
// resize listener exactly once, checking that state rather than a
// value baked into the closure at registration time.
let wantOn = false
let resizeBound = false

function apply(on: boolean): void {
  const main = document.querySelector('main')
  if (!(main instanceof HTMLElement)) return
  if (!on || window.innerWidth < MIN_WIDTH_PX) {
    main.style.filter = ''
    return
  }
  if (!installed) ensureFilter(main.clientWidth, main.clientHeight)
  main.style.filter = 'url(#crt-page-warp)'
}

export function setPageWarp(on: boolean): void {
  wantOn = on
  apply(on)
  if (!resizeBound) {
    resizeBound = true
    addEventListener('resize', () => {
      installed = false
      apply(wantOn)
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
