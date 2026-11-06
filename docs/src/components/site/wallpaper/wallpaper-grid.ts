/**
 * The wallpaper's quantization grid, palette and lattices, prepared on the
 * CPU for the shader (wallpaper-gl.ts), in double precision as
 * computeWallpaper() (tauler-wallpaper.js) works them out. Pure: no WebGL
 * here, so tests/wallpaper.spec.ts checks the palette on the CPU.
 *
 * What the CPU prepares instead of the shader, per draw (all small):
 *
 * - The palette: every grid point the slice can reach, the wallpaper's and
 *   the room's (any lightness, chroma to 0.4, ROOM_HUE_SUB hue columns per
 *   wallpaper hue level), converted to sRGB by the
 *   module's own oklchToRgb() (gamut mapping included), so a colour is the
 *   module's to the byte; the shader only computes levels.
 * - The hue lattice: a corner never equals its left, upper or upper-left
 *   neighbour, a dynamic-programming table filled from the page's top-left
 *   (the module's distinctLattice()). It is filled on the CPU for the corners
 *   the slice touches and uploaded; the wash lattice goes up with it.
 * - Each column's and row's lattice corner and eased weight, computed in
 *   double precision as the module does and uploaded as floats. The shader
 *   would otherwise divide page coordinates (tens of thousands of px deep in
 *   a long page) by the lattice spacing in single precision.
 *
 */
import {
  EASE,
  distinctLattice,
  hash3,
  oklchToRgb,
  rgbToOklab,
  U32,
} from '@/lib/wallpaper/tauler-wallpaper.js'
import { WALLPAPER, type WallpaperColours } from './options'

const TAU = 2 * Math.PI

/** The highest chroma a room colour keeps a level for (no sRGB colour goes past 0.37). */
const ROOM_MAX_CHROMA = 0.4

/**
 * How many times finer than the wallpaper's the room's hue grid is: 4, so
 * 15° steps where the wallpaper's are 60° (WALLPAPER.stepH).
 *
 * On the wallpaper's own grid the dither spreads a room colour over the two
 * hue levels around it, up to 60° away: a green leaf between 120° and
 * 180° drew cyan specks, and the tan rug and floor pink and yellow ones. The
 * wallpaper's colours are near grey, where hue barely shows, so its grid
 * was never chosen with saturated colours in mind. On the finer grid a room
 * cell's hue is within one 15° step of its colour's, and the dither still
 * blends the two levels around it, so no gradient bands.
 *
 * The palette holds both grids at once: column h is hue h × sHq /
 * ROOM_HUE_SUB, so the wallpaper's level q is column q × ROOM_HUE_SUB, the
 * very colour it was before. Wallpaper cells are unchanged to the byte, and
 * a room cell fading into the wallpaper picks from a superset of its
 * colours. (ADR 0048, "Why one pipeline".)
 */
export const ROOM_HUE_SUB = 4

/** The quantization grid and lattices for one set of colours. */
export function wallpaperGrid(colours: WallpaperColours) {
  const o = WALLPAPER
  const lab = rgbToOklab(colours.background)
  const bgL = lab[0],
    bgC = Math.hypot(lab[1], lab[2])
  const sL = Math.max(0.002, o.stepL),
    sC = Math.max(0.001, o.stepC)
  const nHq = Math.max(
    1,
    Math.round(TAU / ((Math.max(0.5, o.stepH) * Math.PI) / 180)),
  )
  const sHq = TAU / nHq
  const intensity = Math.max(0, Math.min(1, o.intensity))
  const dL = colours.ringL - bgL,
    dC = colours.ringC - bgC
  const seed = o.seed | 0
  const nCell = Math.max(2, o.cellHues | 0)
  const washMax = Math.max(0, Math.min(1, o.wash))
  // The levels the module keeps colours for (its colour cache's range).
  const qL0 = Math.floor(Math.min(0, dL * intensity) / sL) - 2
  const nqL = Math.ceil((Math.abs(dL) * intensity) / sL) + 5
  const nqC = Math.ceil((Math.max(0, dC) * intensity) / sC) + 4
  // The levels a room colour can take: lightness inside 0..1, chroma to
  // ROOM_MAX_CHROMA (tests/docs.spec.ts builds the same grid).
  const roomL = [Math.ceil(-bgL / sL), Math.floor((1 - bgL) / sL)]
  const roomC = Math.floor(ROOM_MAX_CHROMA / sC + 1e-9)
  // Palette: x = the room's hue level (the wallpaper's level q is column
  // q * ROOM_HUE_SUB), y = (lightness level - palL0) * palNC + chroma level.
  const palL0 = Math.min(qL0, roomL[0])
  const palNL = Math.max(qL0 + nqL - 1, roomL[1]) - palL0 + 1
  const palNC = Math.max(nqC, roomC + 1)
  const palNH = nHq * ROOM_HUE_SUB
  const palette = new Uint8Array(palNH * palNL * palNC * 4)
  for (let l = 0; l < palNL; l++)
    for (let c = 0; c < palNC; c++)
      for (let h = 0; h < palNH; h++) {
        const qL = palL0 + l
        // the background level is the background colour itself (see the module)
        const rgb =
          qL === 0 && c === 0
            ? colours.background
            : oklchToRgb(
                bgL + qL * sL,
                Math.max(0, bgC + c * sC),
                (h * sHq) / ROOM_HUE_SUB,
              )
        palette.set([...rgb, 255], ((l * palNC + c) * palNH + h) * 4)
      }
  return {
    bgL,
    bgC,
    sL,
    sC,
    sHq,
    nHq,
    palNH,
    dL,
    dC,
    intensity,
    wallL: [qL0, qL0 + nqL - 1],
    wallC: nqC - 1,
    roomL,
    roomC,
    palL0,
    palNL,
    palNC,
    palette,
    hueAmp: Math.max(0, o.noise) * 2 * sHq,
    biasAmp:
      Math.abs(dL) > 1e-6 ? (Math.max(0, o.bias) * 2 * sL) / Math.abs(dL) : 0,
    seedHueNoise: seed + 1,
    seedBias: seed + 3,
    seedEdge: seed + 4,
    hue: distinctLattice(
      seed,
      Array.from({ length: nCell }, (_, k) => (k / nCell) * TAU),
    ),
    wash: (i: number, j: number) => {
      const v = hash3(seed + 2, i, j) * U32
      return washMax * v * v
    },
  }
}
export type Grid = ReturnType<typeof wallpaperGrid>

/**
 * Each column's (or row's) lattice corner and eased weight, for both
 * lattices, as computeWallpaper() works them out: [hue corner - first, hue
 * weight, wash corner - first, wash weight] per cell, and the first and last
 * corner of each lattice.
 */
export function axis(first: number, count: number, pc: number) {
  const ease = EASE[WALLPAPER.ease] ?? EASE.smooth
  const cellH = Math.max(1, WALLPAPER.cell),
    cellW = Math.max(1, WALLPAPER.washSize)
  const out = new Float32Array(count * 4)
  const hI = new Int32Array(count),
    wI = new Int32Array(count)
  for (let k = 0; k < count; k++) {
    const x = (first + k + 0.5) * pc
    let u = x / cellH,
      i = Math.floor(u)
    hI[k] = i
    out[k * 4 + 1] = ease(u - i)
    u = x / cellW
    i = Math.floor(u)
    wI[k] = i
    out[k * 4 + 3] = ease(u - i)
  }
  for (let k = 0; k < count; k++) {
    out[k * 4] = hI[k] - hI[0]
    out[k * 4 + 2] = wI[k] - wI[0]
  }
  return {
    table: out,
    hue: [hI[0], hI[count - 1] + 1],
    wash: [wI[0], wI[count - 1] + 1],
  }
}
