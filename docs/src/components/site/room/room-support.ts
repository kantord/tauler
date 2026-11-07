/**
 * Whether the room can run, apart from room.ts so that the check costs no
 * download: page-wallpaper.ts asks it first and fetches room.ts (three.js,
 * the renderer's GLSL) only on a yes.
 */
import { isSoftware } from '../wallpaper/wallpaper-gl'
import { testHooks } from '../test-hooks'

/**
 * True when the room can render in `gl`: hardware WebGL 2 with float render
 * targets. Software WebGL (SwiftShader, llvmpipe) is refused: a path tracer
 * there takes seconds per frame and freezes the page (unless a test asks,
 * testHooks().roomSoftwareGl).
 */
export function roomSupported(gl: WebGL2RenderingContext): boolean {
  return (
    (!!testHooks().roomSoftwareGl || !isSoftware(gl)) &&
    !!gl.getExtension('EXT_color_buffer_float')
  )
}
