/**
 * The wallpaper's options: the one source the two things that draw it
 * read. Those are the shader (wallpaper-gl.ts, which also turns the home
 * page's room into pixel art on the same grid) and its JS fallback
 * (computeWallpaper() in tauler-wallpaper.js, also the reference the shader
 * is tested against).
 *
 * Colours are not here: they are the CSS variables --wallpaper-bg,
 * --wallpaper-ring-l and --wallpaper-ring-c (global.css), per theme.
 */
import {
  DEFAULTS,
  readCssColors,
} from '../../../lib/wallpaper/tauler-wallpaper.js'

/** The site's overrides of the designer's defaults. */
export const WALLPAPER_OPTIONS = { intensity: 0.1, seed: 18 } as const

/** Every option, as the site draws the wallpaper. */
export const WALLPAPER = { ...DEFAULTS, ...WALLPAPER_OPTIONS } as const

export type RGB = [number, number, number]

/** The wallpaper's colours: its background (sRGB 0-255) and hue ring. */
export interface WallpaperColours {
  background: RGB
  ringL: number
  ringC: number
}

/**
 * The colours where `element` sits, read from the CSS variables as the
 * wallpaper module reads them; a missing one is the module's light fallback.
 */
export function wallpaperColours(element: Element): WallpaperColours {
  const {
    background = [248, 245, 238],
    ringL = 0.66,
    ringC = 0.12,
  } = readCssColors(element)
  return { background, ringL, ringC }
}

/**
 * One dither pixel, in whole device px, at this device pixel ratio: the
 * wallpaper module's rule (computeWallpaper()).
 */
export function ditherPixel(dpr: number) {
  const size = Math.max(1, Math.round(WALLPAPER.pixelSize))
  return WALLPAPER.pixelUnit === 'css'
    ? Math.max(1, Math.round(size * dpr))
    : size
}
