/* tauler generative wallpaper. Six deterministic fields, one per workspace.
   Seeded: the same workspace always draws the same art, so a panel can
   redraw an offset slice of it and match the wallpaper pixel for pixel.
   Nothing animates. */

export const FIELDS = ['flow'] as const

export type FieldKind = (typeof FIELDS)[number]

export function drawField(
  canvas: HTMLCanvasElement,
  kind: FieldKind = 'flow',
  hex = '#F0855A',
  seedIndex = 0,
  dpr = 2,
): void {
  const ctx = canvas.getContext('2d')
  if (!ctx) return
  const w = canvas.width / dpr,
    h = canvas.height / dpr
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
  ctx.clearRect(0, 0, w, h)
  ctx.lineCap = 'round'
  let s = 9001 + seedIndex * 7331
  const R = () => (s = (s * 1664525 + 1013904223) % 4294967296) / 4294967296
  const n = parseInt(hex.slice(1), 16),
    cr = (n >> 16) & 255,
    cg = (n >> 8) & 255,
    cb = n & 255
  const A = (a: number) => 'rgba(' + cr + ',' + cg + ',' + cb + ',' + a + ')'
  const W = (a: number) => 'rgba(238,236,240,' + a + ')'

  if (kind === 'flow') {
    const ang = (x: number, y: number) =>
      Math.sin(x * 0.0041) * 1.7 +
      Math.cos(y * 0.0053) * 1.4 +
      Math.sin((x + y) * 0.0016) * 0.9
    const strokes = Math.round(1500 * Math.max(1, (w * h) / (1920 * 950)))
    for (let p = 0; p < strokes; p++) {
      let x = R() * w,
        y = R() * h
      ctx.strokeStyle = R() < 0.22 ? W(0.05 + R() * 0.05) : A(0.05 + R() * 0.08)
      ctx.lineWidth = R() < 0.15 ? 1.6 : 0.9
      ctx.beginPath()
      ctx.moveTo(x, y)
      for (let k = 0; k < 58; k++) {
        const t = ang(x, y)
        x += Math.cos(t) * 3.4
        y += Math.sin(t) * 3.4
        if (x < -20 || x > w + 20 || y < -20 || y > h + 20) break
        ctx.lineTo(x, y)
      }
      ctx.stroke()
    }
  }

  /* Film grain. A 128px noise tile, source-over at very low alpha, laid
     down BEFORE the erase so it fades with the field instead of stopping
     at the stage edge. Kills the banding in the falloff and makes the
     accent read as light rather than as CSS. */
  const tile = document.createElement('canvas')
  tile.width = tile.height = 128
  const tctx = tile.getContext('2d')
  if (!tctx) return
  const img = tctx.createImageData(128, 128)
  for (let q = 0; q < img.data.length; q += 4) {
    const v = 150 + Math.floor(R() * 105)
    img.data[q] = v
    img.data[q + 1] = v
    img.data[q + 2] = v
    img.data[q + 3] = Math.floor(R() * 16)
  }
  tctx.putImageData(img, 0, 0)
  const pat = ctx.createPattern(tile, 'repeat')
  if (pat) {
    ctx.fillStyle = pat
    ctx.fillRect(0, 0, w, h)
  }

  /* Every field is erased outward so it dissolves into the surface
     instead of ending at an edge. This is not optional. */
  ctx.globalCompositeOperation = 'destination-out'
  const grd = ctx.createRadialGradient(
    w * 0.44,
    h * 0.5,
    Math.min(w, h) * 0.2,
    w * 0.44,
    h * 0.5,
    Math.max(w, h) * 0.72,
  )
  grd.addColorStop(0, 'rgba(0,0,0,0)')
  grd.addColorStop(0.55, 'rgba(0,0,0,0.28)')
  grd.addColorStop(1, 'rgba(0,0,0,0.94)')
  ctx.fillStyle = grd
  ctx.fillRect(0, 0, w, h)
  ctx.globalCompositeOperation = 'source-over'
}
