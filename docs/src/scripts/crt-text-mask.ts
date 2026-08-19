// Attenuates the scanline mesh specifically over text — so the mesh's
// own darkening doesn't compound with the aberration/flicker already
// sitting on the same glyphs. Off (0) by default: this is an addition
// on top of the shipped scanline mask, not a replacement for it.
//
// Mechanism: builds a canvas-rasterised alpha mask — full alpha (255,
// full scanline strength) everywhere, reduced alpha over the bounding
// box of every text-bearing element. That's combined with the existing
// top/bottom split mask via a second mask-image layer + mask-composite:
// intersect, so both masks' alpha multiply together. Both layers are
// pure alpha (no color/luminance ambiguity) for the same reason the
// original split mask is: this is the one mask semantic Chromium
// resolves unambiguously without needing an explicit mask-type.
//
// Chromium-only: mask-composite here uses the unprefixed spec keyword
// (intersect), not the older -webkit-mask-composite Porter-Duff
// keywords Safari still needs. Not verified outside Chromium — this
// project's own test suite only runs Chromium too (playwright.config.ts).

const MAP_LONG_EDGE = 512
const PADDING_PX = 3

function collectTextRects(width: number, height: number): DOMRect[] {
  const rects: DOMRect[] = []
  const shouldSkip = (el: Element): boolean =>
    !!el.closest('[aria-label="CRT tuning panel"]') ||
    (typeof el.className === 'string' && el.className.startsWith('crt-'))

  const walker = document.createTreeWalker(
    document.body,
    NodeFilter.SHOW_ELEMENT,
    {
      acceptNode(node) {
        const el = node as Element
        if (shouldSkip(el)) return NodeFilter.FILTER_REJECT
        for (const child of Array.from(el.childNodes)) {
          if (
            child.nodeType === Node.TEXT_NODE &&
            child.textContent &&
            child.textContent.trim()
          ) {
            return NodeFilter.FILTER_ACCEPT
          }
        }
        return NodeFilter.FILTER_SKIP
      },
    },
  )

  let n: Node | null
  while ((n = walker.nextNode())) {
    const r = (n as Element).getBoundingClientRect()
    if (
      r.width > 0 &&
      r.height > 0 &&
      r.bottom > 0 &&
      r.top < height &&
      r.right > 0 &&
      r.left < width
    ) {
      rects.push(r)
    }
  }
  return rects
}

function buildMaskDataUrl(
  attenuation: number,
  width: number,
  height: number,
): string {
  const scale = MAP_LONG_EDGE / Math.max(width, height)
  const canvas = document.createElement('canvas')
  canvas.width = Math.max(1, Math.round(width * scale))
  canvas.height = Math.max(1, Math.round(height * scale))
  const ctx = canvas.getContext('2d')
  if (!ctx) return ''

  ctx.fillStyle = 'rgba(255,255,255,1)'
  ctx.fillRect(0, 0, canvas.width, canvas.height)

  const alpha = Math.max(0, Math.min(1, 1 - attenuation))
  ctx.fillStyle = `rgba(255,255,255,${alpha})`
  for (const r of collectTextRects(width, height)) {
    ctx.fillRect(
      (r.left - PADDING_PX) * scale,
      (r.top - PADDING_PX) * scale,
      (r.width + PADDING_PX * 2) * scale,
      (r.height + PADDING_PX * 2) * scale,
    )
  }
  return canvas.toDataURL('image/png')
}

const SPLIT: Record<'top' | 'bottom', string> = {
  top: 'linear-gradient(to bottom, black 50%, transparent 50%)',
  bottom: 'linear-gradient(to bottom, transparent 50%, black 50%)',
}

let currentAttenuation = 0
let resizeBound = false

function render(): void {
  const width = window.innerWidth
  const height = window.innerHeight

  for (const suffix of ['top', 'bottom'] as const) {
    const el = document.querySelector(`.crt-scanlines-${suffix}`)
    if (!(el instanceof HTMLElement)) continue

    if (currentAttenuation <= 0) {
      // Back to the plain stylesheet mask — no need to keep a canvas
      // data URL alive when the effect is off.
      el.style.maskImage = ''
      el.style.webkitMaskImage = ''
      el.style.maskComposite = ''
      el.style.maskSize = ''
      el.style.webkitMaskSize = ''
      el.style.maskRepeat = ''
      el.style.webkitMaskRepeat = ''
      continue
    }

    const dataUrl = buildMaskDataUrl(currentAttenuation, width, height)
    el.style.maskImage = `${SPLIT[suffix]}, url(${dataUrl})`
    el.style.webkitMaskImage = `${SPLIT[suffix]}, url(${dataUrl})`
    el.style.maskComposite = 'intersect'
    // The PNG's intrinsic size is MAP_LONG_EDGE (512px on its long
    // edge) scaled down from the real viewport — much smaller than the
    // element it's masking. Without an explicit size, mask-size
    // defaults to auto (native pixel size) and mask-repeat defaults to
    // repeat, so the mask TILES across the viewport at the wrong scale
    // instead of stretching to cover it — text-shaped holes end up
    // scattered at repeat-period offsets instead of pinned under the
    // actual text. The gradient layer has no intrinsic size and
    // already fills correctly either way, but mask-size is a single
    // comma-separated value applying to both layers positionally, so
    // both need stating explicitly.
    el.style.maskSize = '100% 100%, 100% 100%'
    el.style.webkitMaskSize = '100% 100%, 100% 100%'
    el.style.maskRepeat = 'no-repeat, no-repeat'
    el.style.webkitMaskRepeat = 'no-repeat, no-repeat'
  }
}

export function setTextAttenuation(attenuation: number): void {
  currentAttenuation = attenuation
  render()
  if (!resizeBound) {
    resizeBound = true
    let t: ReturnType<typeof setTimeout>
    addEventListener('resize', () => {
      clearTimeout(t)
      t = setTimeout(render, 200)
    })
    // Rects measured before web fonts swap in are wrong — fallback-font
    // metrics differ from the real ones, so text can be narrower/wider
    // than what got baked into the mask a moment earlier.
    document.fonts?.ready.then(() => {
      if (currentAttenuation > 0) render()
    })
  }
}
