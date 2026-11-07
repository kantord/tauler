/**
 * The one test seam of the home background: `window.__taulerTest`, an object
 * a test sets before the page loads (Playwright's addInitScript) to change
 * how the wallpaper and the room behave. Production users never set it;
 * nothing on the page writes it, and no URL parameter, storage key or other
 * user input reaches it. Absent, testHooks() returns one frozen empty object:
 * every hook is off, and reading one costs a property lookup.
 *
 * The hooks ship in the production bundle on purpose: CI's Playwright tests
 * run against the production static build (DOCS_PREBUILT), so stripping them
 * would mean testing a different artifact (ADR 0048, "Testing without a
 * GPU").
 *
 * Read only here, by the wallpaper (page-wallpaper.ts) and the room (room.ts,
 * room-shader.ts); each reads a hook when it starts, so a hook set after the
 * page loaded takes effect at the next start.
 */
import type { Fade } from './wallpaper/wallpaper-gl'

export interface TaulerTestHooks {
  /**
   * Lets the wallpaper's shader run on software WebGL, which it refuses as
   * slower than the CPU path (page-wallpaper.ts); for tests that check the
   * shader where there is no GPU.
   */
  wallpaperSoftwareGl?: boolean
  /**
   * Lets the room run on software WebGL, which roomSupported() refuses; for
   * the smoke test, which runs it in a tiny window. The wallpaper's context
   * must be allowed there too (wallpaperSoftwareGl).
   */
  roomSoftwareGl?: boolean
  /** Appends a syntax error to the room's fragment shader; for the smoke test. */
  roomBrokenShader?: boolean
  /** Stops the room at this many samples (0: none, the room not drawn). */
  roomSamples?: number
  /** Replaces ROOM_FADE, ROOM_WAVE's depth, ROOM_MATCH and ROOM_DETAIL before the room starts. */
  roomFade?: Partial<Fade & { depth: number; match: number; detail: number }>
}

declare global {
  interface Window {
    /** Set only by tests: see test-hooks.ts. */
    __taulerTest?: TaulerTestHooks
  }
}

const NONE: Readonly<TaulerTestHooks> = Object.freeze({})

/** The hooks a test set, or none. */
export function testHooks(): Readonly<TaulerTestHooks> {
  return window.__taulerTest ?? NONE
}
