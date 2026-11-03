/**
 * The height of the render whiteroom.ts pins to the bottom of the window, as
 * a CSS length, and how its top fades into the wallpaper. A plain module
 * rather than exports of whiteroom.ts: that is a client module, and the home
 * page, a server component, lays its logo out above the render from
 * ROOM_HEIGHT.
 *
 * The render spans the window's full width (these pages drop the scrollbar
 * gutter, see <Wallpaper edgeToEdge />), so its aspect runs from about 0.9 on
 * a phone held upright to 12 on a short, very wide window. whiteroom.ts frames
 * the camera to whatever box it gets, and the scene's walls reach far enough
 * for the widest of them (see scripts/whiteroom-scene.patch).
 */

/** Half the window height; the wallpaper always shows above it. */
export const ROOM_HEIGHT = '50svh'

/**
 * The top of the render fades into the wallpaper, one OKLCH channel at a
 * time: each value is the share of the render's height, from its top edge,
 * over which that channel eases from the wallpaper's to the render's. At the
 * top edge all three are the wallpaper's, so the render's top row is the
 * wallpaper, pixel for pixel. Lightness goes first, so the room's shape
 * shows early; its hues arrive last.
 */
export const ROOM_FADE = { L: 0.15, C: 0.25, h: 0.4 } as const
