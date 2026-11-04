/**
 * Where the room sits on the page and how it fades into the wallpaper. A
 * plain module rather than exports of whiteroom.ts: that is a client
 * module, and the home page, a server component, lays its logo out above the
 * room from ROOM_HEIGHT.
 *
 * The room is anchored to the page, like the wallpaper it is drawn with
 * (wallpaper-gl.ts): it fills the bottom ROOM_HEIGHT of the first screen,
 * the full width (these pages drop the scrollbar gutter, see
 * <Wallpaper edgeToEdge />), and scrolls away with the content. Its aspect
 * runs from about 0.9 on a phone held upright to 12 on a short, very wide
 * window; whiteroom.ts frames the camera to whatever box it gets.
 */

/** Half the first screen's height; the wallpaper always shows above it. */
export const ROOM_HEIGHT = '50svh'

/**
 * The room's top fades into the wallpaper, one OKLCH channel at a time: each
 * value is the share of ROOM_HEIGHT, from where the fade starts, over which
 * that channel eases (smootherstep) from the wallpaper's to the room's. Where
 * all three are still the wallpaper's, the cell is the wallpaper's own.
 * Below the first screen the room fades out the same way, so on a page that
 * scrolls its floor runs into the wallpaper rather than stopping at an edge
 * (the image runs on below the first screen for it, see whiteroom.ts).
 *
 * Lightness takes longest, the channel the eye follows; chroma and hue can
 * arrive sooner, as the wallpaper is near grey.
 */
export const ROOM_FADE = { L: 0.55, C: 0.4, h: 0.3 } as const

/**
 * Where each fade starts, column by column: a wave, so no straight line
 * marks where the room begins. The wave is a sum of `octaves` sines along
 * the page's x (CSS px, so it holds still under scroll and resize), the
 * first `wavelength` long, each next one `lacunarity` times shorter (not a
 * whole ratio, so the sum never visibly repeats) and `persistence` times
 * weaker. Each octave has a seeded phase, and two smooth seeded noises
 * along x warp it: one bends its phase by up to `phaseWarp` turns, the
 * other scales its amplitude by 1 ± `ampWarp`, so no stretch looks regular.
 * The sum is soft-limited to `depth` (share of ROOM_HEIGHT) peak to peak,
 * `rise` of it above the room's top and the rest below (above the box the
 * room is its top row's wide mean, wallpaper-gl.ts);
 * below the
 * first screen the bottom fade's start runs from the box's bottom down, on
 * a wave of its own. All randomness is the wallpaper's own seeded hash
 * (wallpaper-gl.ts).
 *
 * Mostly above the box: the lamp's head is about a fifth of the way down a
 * wide room, so the wave's lowest dips (depth × (1 - rise) below the top)
 * stay clear of it, the picture and the furniture at common window sizes.
 */
export const ROOM_WAVE = {
  octaves: 4,
  wavelength: 760,
  lacunarity: 2.13,
  persistence: 0.45,
  phaseWarp: 0.8,
  ampWarp: 0.7,
  depth: 0.3,
  rise: 0.9,
} as const

/**
 * How far into the room (share of ROOM_HEIGHT, from each fade's start) its
 * local mean lightness and chroma are pulled to the wallpaper's: fully at
 * the edge, easing to none. Without it, a room much lighter or darker than
 * the wallpaper where they meet shows as a band however gentle the fade (the
 * light theme's walls are a good deal darker than its page colour). The
 * room's own detail is kept; only its mean moves.
 */
export const ROOM_MATCH = 0.6

/**
 * How far below the room's top (share of ROOM_HEIGHT) its detail eases in
 * over its wide local mean. The wave above lifts the fade's start into rows
 * the image does not cover, which take the mean of its first row; detail
 * appearing all at once at the image's top would draw a line there, and the
 * dark theme's wall corners would show it as vertical streaks starting on it.
 */
export const ROOM_DETAIL = 0.15

/**
 * Over how many samples (of the image's 48) the room fades in from the
 * wallpaper. Before its first sample the room's cells are the wallpaper's
 * own; its weight in the fade (the same per-channel OKLCH mix, multiplied
 * in) then rises along a smootherstep, lightness reaching full at this count
 * and chroma and hue sooner, in the ratio of their ROOM_FADE shares.
 *
 * The first samples are mostly noise: many cells are near black where no
 * path has found light yet. Shown at full weight they flashed dark over the
 * wallpaper; over 12 samples dark specks still showed on the light walls
 * around the 8th sample. By 32 (about half a second at 60 Hz) the noise has
 * thinned out before the room has much weight.
 */
export const ROOM_EASE_IN = 32
