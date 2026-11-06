/**
 * The light the room is lit by and the backdrop its walls and floor start
 * from, per site theme: the page's defaults, with its "Tauler light" and
 * "Tauler dark" backdrops (room.ts's module doc, "Changed to follow the
 * site's theme"), and which theme is on (siteTheme()). room.ts sets them
 * as uniforms.
 */

// ── Colour helpers, from the page ──────────────────────────────────────
type RGB = [number, number, number]
function linToLab(c: RGB): RGB {
  const l = Math.cbrt(
    0.4122214708 * c[0] + 0.5363325363 * c[1] + 0.0514459929 * c[2],
  )
  const m = Math.cbrt(
    0.2119034982 * c[0] + 0.6806995451 * c[1] + 0.1073969566 * c[2],
  )
  const s = Math.cbrt(
    0.0883024619 * c[0] + 0.2817188376 * c[1] + 0.6299787005 * c[2],
  )
  return [
    0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
    1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
    0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s,
  ]
}

function oklchToLin(L: number, C: number, hDeg: number): RGB {
  const h = (hDeg * Math.PI) / 180,
    a = C * Math.cos(h),
    b = C * Math.sin(h)
  let l = L + 0.3963377774 * a + 0.2158037573 * b,
    m = L - 0.1055613458 * a - 0.0638541728 * b,
    s = L - 0.0894841775 * a - 1.291485548 * b
  l = l * l * l
  m = m * m * m
  s = s * s * s
  return [
    4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
  ]
}

// ── The page's defaults ─────────────────────────────────────────────────
/** Lights: the "Photo match" preset. */
export const P0 = {
  exposure: 1.9, // the light theme's; see BACKDROP
  sky: 1.0,
  window: 1.0,
  uplight: 1.0,
  candles: 1.0,
  warmth: 0.5,
}

/**
 * The most luminance one reflected or indirect contribution brings to a
 * sample (see clampFireflies() in room-shader.ts), after exposure: the
 * shader's limit is this over the theme's exposure, so it bites at the same
 * displayed brightness in both themes. 2 is about 92% of white after the filmic curve. At 3 the
 * dark theme's floor still sparkled under the candle lamp; at 1 the glossy
 * floor's reflections and the walls lost a few percent of their light.
 */
export const FIREFLY_MAX = 2

/**
 * Backdrop: the page's "Tauler light" and "Tauler dark" presets, one per site
 * theme, in place of its "Full colour" default. Their colour (the page's
 * `bg`), ring lightness and ring chroma are not here: they are the
 * wallpaper's, so the room's walls start from the page colour of whichever
 * theme is on.
 *
 * Exposure is per theme too: the light is the same in both (see SKY), but
 * dark walls and floor bounce far less of it, so at the light theme's
 * exposure the dark room is near black. 3.5 keeps it dark but legible.
 */
export const BACKDROP = {
  light: {
    cellI: 0.35,
    cellHues: 3,
    cellSize: 300,
    wash: 1,
    seed: 18,
    exposure: P0.exposure,
  },
  dark: {
    cellI: 0.5,
    cellHues: 3,
    cellSize: 300,
    wash: 1,
    seed: 18,
    exposure: 3.5,
  },
}

/**
 * The sky: the colour rays that escape the room pick up, the light that
 * fills most of it. The page takes it from the backdrop; here it stays the
 * page's "Full colour" backdrop in both themes (see skyApart() in
 * room-shader.ts), so a dark theme darkens the walls and floor, not the
 * light falling on them.
 */
export const SKY = linToLab(oklchToLin(0.971, 0.01, 87.5))

/** The site's theme: next-themes puts `.dark` on <html>. */
export function siteTheme(): keyof typeof BACKDROP {
  return document.documentElement.classList.contains('dark') ? 'dark' : 'light'
}
