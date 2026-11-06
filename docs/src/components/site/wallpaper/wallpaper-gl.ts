/**
 * The wallpaper as a WebGL 2 shader: computeWallpaper() (tauler-wallpaper.js)
 * on the GPU, for one horizontal slice of the page at a time, and the one
 * place the page's background becomes pixel art: a room layer (the home
 * page's path-traced room, room.ts) is composited here in continuous
 * colour and quantized with the wallpaper, once. The JS module stays the
 * reference: for the same options, colours, device pixel ratio and page
 * position, and no room, this draws its pixels (tests/wallpaper.spec.ts
 * compares them).
 *
 * Two passes:
 *
 * 1. Cells: one fragment per dither pixel ("cell"), into an RGBA8 target the
 *    slice's size in cells. It runs computeWallpaper()'s inner loop up to the
 *    continuous colour: hue and fade from two lattices with eased 4-corner
 *    means and the per-cell noise hash, giving each OKLCH channel as a
 *    number of grid steps from the background. Where a room layer covers the
 *    cell, the room's colour takes over, channel by channel (see Room
 *    below). Then the one quantize: one Bayer threshold for every channel,
 *    level = floor(value + threshold), and the levels pick the colour from
 *    a palette texture. Room cells take hue on a finer grid than the
 *    wallpaper's (ROOM_HUE_SUB), with the same threshold.
 * 2. Present: every device pixel of the canvas copies its cell, by integer
 *    division, so a cell is exactly devP × devP device pixels on the page's
 *    grid however the slice's top falls.
 *
 * The CPU prepares what the shader would compute badly or slowly, per draw:
 * the palette, the hue and wash lattices, and each column's and row's
 * lattice corner and weight (wallpaper-grid.ts). The GLSL is
 * wallpaper-shaders.ts; this module owns the GL resources and the draw.
 *
 * The per-cell noise is the module's integer hash in `uint` arithmetic,
 * exact. What is left in single precision is the mixing of corners and the
 * final divisions, where the module works in doubles (rounding to floats at
 * fixed points). A cell could land on the other side of a level boundary
 * only if its value were within about 1e-7 of it: none did in testing (on
 * SwiftShader and an NVIDIA GPU, millions of cells, three device pixel
 * ratios, both themes, deep into long pages), so the tests ask for every
 * byte. If a GPU ever differs, it is by one level on a rare cell.
 *
 * The room: an image of continuous colour (display sRGB, one texel per
 * cell) over a band of the page's rows, the full width. Each OKLCH channel
 * is the wallpaper's at the band's top and bottom edges and eases to the
 * room's over its own share of the room's height (RoomCells.fade), along a
 * smootherstep; the hue takes the shorter way round, and a grey room cell
 * takes the wallpaper's. The fades start along a wave (RoomCells.wave,
 * wave() in CELLS): a sum of sines along the page's x, each with a seeded
 * phase and warped by smooth seeded noise, so no straight line marks where
 * the room begins. Where every
 * channel is the wallpaper's the cell is the wallpaper's own computation,
 * byte for byte, so the room's edges are the wallpaper by construction.
 * The room also fades in over time (RoomCells.presence, a weight multiplied
 * into every channel's): before its first sample it is not drawn at all.
 */
import { THRESHOLDS } from '@/lib/wallpaper/tauler-wallpaper.js'
import { WALLPAPER, ditherPixel, type WallpaperColours } from './options'
import { axis, wallpaperGrid, type Grid } from './wallpaper-grid'
import { CELLS, PRESENT, SAMPLERS, VERTEX } from './wallpaper-shaders'

/** A slice of the page: what one canvas shows. */
export interface Slice {
  /** The canvas's top edge, in device px from the page's top. */
  top: number
  /** The canvas's size in device px. */
  width: number
  height: number
  dpr: number
}

/** The share of the room's height over which each OKLCH channel eases. */
export interface Fade {
  L: number
  C: number
  h: number
}

/** A fade's starting line along the page's x: see ROOM_WAVE. */
export interface Wave {
  octaves: number
  /** CSS px */
  wavelength: number
  lacunarity: number
  persistence: number
  /** turns */
  phaseWarp: number
  ampWarp: number
  /** share of the room's height, peak to peak */
  depth: number
  /** share of the depth above the room's top (the bottom fade's wave starts at the box's bottom) */
  rise: number
}

/**
 * A room layer for pass 1: an image of continuous colour on the page's
 * cells, which the wallpaper fades into at its top and bottom.
 */
export interface RoomCells {
  /** One texel per cell, display sRGB; texel row 0 is the band's last row. */
  texture: WebGLTexture
  /** The page's cell rows the image covers, first and last. */
  rows: [number, number]
  /**
   * The first page cell row the room reaches: the top fade's wave rises
   * above the image. Rows above the image take its first row's wide mean
   * (see `detail`).
   */
  from: number
  /** Its width in cells, from the page's left edge. */
  cols: number
  /** Device px from the page's top: where the room's top fade starts (a row's top). */
  top: number
  /** Device px from the page's top: where its bottom fade starts. */
  bottom: number
  /** Device px: the height the fades' shares are of. */
  height: number
  fade: Fade
  /** The wave each fade starts along (ROOM_WAVE in room/room-size.ts). */
  wave: Wave
  /** How far in from each edge (share of height) the room's mean is pulled to the wallpaper's. */
  match: number
  /** How far below its top (share of height) the room's detail eases in over its wide mean. */
  detail: number
  /**
   * How much of the room shows, 0..1 per OKLCH channel, multiplied into that
   * channel's fade: it rises as the image gathers samples (ROOM_EASE_IN). At
   * 0 in every channel the room is not drawn at all, so every cell is the
   * wallpaper's own.
   */
  presence: Fade
}

export interface SliceRenderer {
  readonly kind: 'webgl2' | '2d'
  draw(slice: Slice, colours: WallpaperColours, room?: RoomCells | null): void
  destroy(): void
}

export interface GlSliceRenderer extends SliceRenderer {
  readonly kind: 'webgl2'
  /** The context, which a room layer renders its image in. */
  readonly gl: WebGL2RenderingContext
}

/** True when the WebGL 2 context is software (SwiftShader, llvmpipe, ...). */
export function isSoftware(gl: WebGL2RenderingContext) {
  const debug = gl.getExtension('WEBGL_debug_renderer_info')
  const name = String(
    gl.getParameter(debug ? debug.UNMASKED_RENDERER_WEBGL : gl.RENDERER),
  )
  return /swiftshader|llvmpipe|softpipe|lavapipe|software|basic render driver/i.test(
    name,
  )
}

/**
 * A renderer drawing into `canvas` with WebGL 2, or null where there is none,
 * where it is software and `software` is false, or where its shaders do not
 * compile. `canvas` cannot take a 2D context after this, even on null: give
 * the fallback a fresh one.
 *
 * The context may be shared with a room layer that renders with three.js
 * (room.ts): draw() sets every piece of GL state it relies on, and the
 * room resets three.js's view of the state before it renders.
 */
export function createGlRenderer(
  canvas: HTMLCanvasElement,
  {
    software = false,
    powerPreference = 'low-power',
  }: { software?: boolean; powerPreference?: WebGLPowerPreference } = {},
): GlSliceRenderer | null {
  const gl = canvas.getContext('webgl2', {
    alpha: false,
    antialias: false,
    depth: false,
    stencil: false,
    preserveDrawingBuffer: false,
    powerPreference,
  })
  if (!gl) return null
  if (!software && isSoftware(gl)) {
    gl.getExtension('WEBGL_lose_context')?.loseContext()
    return null
  }

  function program(fragment: string) {
    const p = gl!.createProgram()
    for (const [type, source] of [
      [gl!.VERTEX_SHADER, VERTEX],
      [gl!.FRAGMENT_SHADER, fragment],
    ] as const) {
      const s = gl!.createShader(type)!
      gl!.shaderSource(s, source)
      gl!.compileShader(s)
      gl!.attachShader(p, s)
      gl!.deleteShader(s)
    }
    gl!.linkProgram(p)
    if (!gl!.getProgramParameter(p, gl!.LINK_STATUS)) {
      console.warn('tauler: wallpaper shader:', gl!.getProgramInfoLog(p))
      return null
    }
    const uniforms = new Map<string, WebGLUniformLocation | null>()
    return {
      p,
      at: (name: string) => {
        if (!uniforms.has(name))
          uniforms.set(name, gl!.getUniformLocation(p, name))
        return uniforms.get(name)!
      },
    }
  }
  const cells = program(CELLS),
    present = program(PRESENT)
  if (!cells || !present) {
    gl.getExtension('WEBGL_lose_context')?.loseContext()
    return null
  }
  // no attributes (the triangle comes from gl_VertexID), but a VAO of our
  // own, so a room's three.js buffers are never drawn from
  const vertices = gl.createVertexArray()

  /** The state draw() and upload() rely on, whatever ran in the context before. */
  function resetState() {
    gl!.bindVertexArray(vertices)
    for (const cap of [
      gl!.BLEND,
      gl!.CULL_FACE,
      gl!.DEPTH_TEST,
      gl!.STENCIL_TEST,
      gl!.SCISSOR_TEST,
      gl!.DITHER,
      gl!.RASTERIZER_DISCARD,
      gl!.POLYGON_OFFSET_FILL,
      gl!.SAMPLE_ALPHA_TO_COVERAGE,
      gl!.SAMPLE_COVERAGE,
    ])
      gl!.disable(cap)
    gl!.colorMask(true, true, true, true)
    gl!.bindBuffer(gl!.PIXEL_UNPACK_BUFFER, null)
    gl!.pixelStorei(gl!.UNPACK_ALIGNMENT, 1)
    gl!.pixelStorei(gl!.UNPACK_FLIP_Y_WEBGL, false)
    gl!.pixelStorei(gl!.UNPACK_PREMULTIPLY_ALPHA_WEBGL, false)
    for (const p of [
      gl!.UNPACK_ROW_LENGTH,
      gl!.UNPACK_IMAGE_HEIGHT,
      gl!.UNPACK_SKIP_PIXELS,
      gl!.UNPACK_SKIP_ROWS,
      gl!.UNPACK_SKIP_IMAGES,
    ])
      gl!.pixelStorei(p, 0)
  }

  const textures = new Map<string, WebGLTexture>()
  /** (Re)fills the named texture; float formats for tables and lattices. */
  function upload(
    name: string,
    width: number,
    height: number,
    format: 'r32f' | 'rgba32f' | 'rgba8',
    data: ArrayBufferView | null,
  ) {
    let texture = textures.get(name)
    if (!texture) {
      texture = gl!.createTexture()!
      textures.set(name, texture)
    }
    gl!.activeTexture(gl!.TEXTURE0)
    gl!.bindTexture(gl!.TEXTURE_2D, texture)
    for (const parameter of [gl!.TEXTURE_MIN_FILTER, gl!.TEXTURE_MAG_FILTER])
      gl!.texParameteri(gl!.TEXTURE_2D, parameter, gl!.NEAREST)
    const [internal, layout, type] = {
      r32f: [gl!.R32F, gl!.RED, gl!.FLOAT],
      rgba32f: [gl!.RGBA32F, gl!.RGBA, gl!.FLOAT],
      rgba8: [gl!.RGBA8, gl!.RGBA, gl!.UNSIGNED_BYTE],
    }[format]
    gl!.texImage2D(
      gl!.TEXTURE_2D,
      0,
      internal,
      width,
      height,
      0,
      layout,
      type,
      data,
    )
    return texture
  }
  /** A per-cell table, wrapped 1024 entries to a row (see table() in CELLS). */
  function uploadTable(name: string, table: Float32Array) {
    const n = table.length / 4
    const width = Math.min(n, 1024),
      height = Math.ceil(n / 1024)
    const padded = new Float32Array(width * height * 4)
    padded.set(table)
    upload(name, width, height, 'rgba32f', padded)
  }
  function uploadLattice(
    name: string,
    lattice: (i: number, j: number) => number,
    [i0, i1]: number[],
    [j0, j1]: number[],
  ) {
    const width = i1 - i0 + 1,
      height = j1 - j0 + 1
    const values = new Float32Array(width * height)
    for (let j = 0; j < height; j++)
      for (let i = 0; i < width; i++)
        values[j * width + i] = lattice(i0 + i, j0 + j)
    upload(name, width, height, 'r32f', values)
  }

  resetState()
  const threshold = THRESHOLDS[WALLPAPER.matrix] ?? THRESHOLDS.bayer8
  upload('threshold', 8, 8, 'r32f', threshold)
  const framebuffer = gl.createFramebuffer()

  let grid: Grid | null = null,
    gridKey = ''
  let columns = { key: '', hue: [0, 0], wash: [0, 0] }
  let cellsSize = ''

  return {
    kind: 'webgl2',
    gl,
    draw(slice, colours, room) {
      resetState()
      const key = JSON.stringify(colours)
      if (key !== gridKey) {
        grid = wallpaperGrid(colours)
        gridKey = key
        upload(
          'palette',
          grid.palNH,
          grid.palNL * grid.palNC,
          'rgba8',
          grid.palette,
        )
      }
      const g = grid!
      const devP = ditherPixel(slice.dpr),
        pc = devP / slice.dpr
      const row0 = Math.floor(slice.top / devP)
      const shift = slice.top - row0 * devP
      const cols = Math.ceil(slice.width / devP),
        rows = Math.ceil((slice.height + shift) / devP)
      if (
        gl.drawingBufferWidth !== slice.width ||
        gl.drawingBufferHeight !== slice.height
      )
        throw new Error('the canvas is larger than WebGL can draw')

      const columnsKey = `${cols} ${slice.dpr}`
      if (columns.key !== columnsKey) {
        const x = axis(0, cols, pc)
        uploadTable('cols', x.table)
        columns = { key: columnsKey, hue: x.hue, wash: x.wash }
      }
      const y = axis(row0, rows, pc)
      uploadTable('rows', y.table)
      uploadLattice('hue', g.hue, columns.hue, y.hue)
      uploadLattice('wash', g.wash, columns.wash, y.wash)

      // pass 1: cells
      const target =
        cellsSize === `${cols} ${rows}`
          ? textures.get('cells')!
          : upload('cells', cols, rows, 'rgba8', null)
      cellsSize = `${cols} ${rows}`
      gl.bindFramebuffer(gl.FRAMEBUFFER, framebuffer)
      gl.framebufferTexture2D(
        gl.FRAMEBUFFER,
        gl.COLOR_ATTACHMENT0,
        gl.TEXTURE_2D,
        target,
        0,
      )
      gl.viewport(0, 0, cols, rows)
      gl.useProgram(cells.p)
      const u = cells.at
      SAMPLERS.forEach(([name, uniform], unit) => {
        gl.activeTexture(gl.TEXTURE0 + unit)
        gl.bindTexture(gl.TEXTURE_2D, textures.get(name)!)
        gl.uniform1i(u(uniform), unit)
      })
      gl.uniform2i(u('uOrigin'), 0, row0)
      gl.uniform1ui(u('uSeedHueNoise'), g.seedHueNoise)
      gl.uniform1ui(u('uSeedBias'), g.seedBias)
      gl.uniform1ui(u('uSeedEdge'), g.seedEdge)
      gl.uniform1f(u('uHueAmp'), g.hueAmp)
      gl.uniform1f(u('uBiasAmp'), g.biasAmp)
      gl.uniform1f(u('uIntensity'), g.intensity)
      gl.uniform1f(u('uDL'), g.dL)
      gl.uniform1f(u('uDC'), g.dC)
      gl.uniform1f(u('uSL'), g.sL)
      gl.uniform1f(u('uSC'), g.sC)
      gl.uniform1f(u('uSH'), g.sHq)
      gl.uniform1i(u('uNH'), g.nHq)
      gl.uniform1i(u('uPalL0'), g.palL0)
      gl.uniform1i(u('uPalNC'), g.palNC)
      gl.uniform2i(u('uWallL'), g.wallL[0], g.wallL[1])
      gl.uniform1i(u('uWallC'), g.wallC)
      // a room with nothing to show yet is not drawn: the wallpaper's own cells
      const showRoom =
        room && room.presence.L + room.presence.C + room.presence.h > 0
      gl.uniform1i(u('uRoom'), showRoom ? 1 : 0)
      if (showRoom) {
        const unit = SAMPLERS.length
        gl.activeTexture(gl.TEXTURE0 + unit)
        gl.bindTexture(gl.TEXTURE_2D, room.texture)
        gl.uniform1i(u('uRoomImage'), unit)
        gl.uniform3i(u('uRoomCells'), room.cols, room.rows[0], room.rows[1])
        gl.uniform1i(u('uRoomFrom'), room.from)
        gl.uniform3f(u('uRoomEdges'), room.top, room.bottom, room.height)
        gl.uniform3f(u('uFade'), room.fade.L, room.fade.C, room.fade.h)
        const w = room.wave
        gl.uniform4f(
          u('uWave'),
          w.depth,
          w.wavelength,
          w.lacunarity,
          w.persistence,
        )
        gl.uniform3f(u('uWarp'), w.phaseWarp, w.ampWarp, w.rise)
        gl.uniform1i(u('uOctaves'), w.octaves)
        gl.uniform1f(u('uPc'), pc)
        gl.uniform1f(u('uMatch'), room.match)
        gl.uniform1f(u('uDetail'), room.detail)
        gl.uniform3f(
          u('uPresence'),
          room.presence.L,
          room.presence.C,
          room.presence.h,
        )
        gl.uniform2i(u('uRoomL'), g.roomL[0], g.roomL[1])
        gl.uniform1i(u('uRoomC'), g.roomC)
        gl.uniform2f(u('uBg'), g.bgL, g.bgC)
        gl.uniform1i(u('uDevP'), devP)
      }
      gl.drawArrays(gl.TRIANGLES, 0, 3)

      // pass 2: present
      gl.bindFramebuffer(gl.FRAMEBUFFER, null)
      gl.viewport(0, 0, slice.width, slice.height)
      gl.useProgram(present.p)
      gl.activeTexture(gl.TEXTURE0)
      gl.bindTexture(gl.TEXTURE_2D, target)
      gl.uniform1i(present.at('uCells'), 0)
      gl.uniform1i(present.at('uDevP'), devP)
      gl.uniform1i(present.at('uHeight'), slice.height)
      gl.uniform1i(present.at('uShift'), shift)
      gl.drawArrays(gl.TRIANGLES, 0, 3)
    },
    destroy() {
      for (const texture of textures.values()) gl.deleteTexture(texture)
      gl.deleteFramebuffer(framebuffer)
      gl.deleteVertexArray(vertices)
      gl.deleteProgram(cells.p)
      gl.deleteProgram(present.p)
      gl.getExtension('WEBGL_lose_context')?.loseContext()
    },
  }
}
