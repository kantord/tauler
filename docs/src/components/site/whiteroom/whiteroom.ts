/**
 * "White Room, Pixel Traced": a white room path-traced live by Erich
 * Loftis's THREE.js PathTracing Renderer (CC0,
 * https://github.com/erichlof/THREE.js-PathTracing-Renderer), as a layer of
 * the wallpaper: the wallpaper's shader (wallpaper-gl.ts) composites the
 * room's continuous colour with its own and turns the two into pixel art in
 * one quantize, on one grid, with one palette.
 *
 * What renders runs unchanged (scripts/vendor-whiteroom.sh): the renderer's
 * GLSL chunks and blue noise, the page's path-tracing, copy and output
 * shaders, and its scene description, on the three.js revision it ships
 * (r185).
 *
 * The page's main script is ported here by hand, with its defaults: the
 * "Photo match" lights and 48 samples. Kept: the uniforms, the three passes
 * up to continuous colour (path trace into a float target, copy it into the
 * accumulation target, filmic tone-map into an 8-bit target) and the photo's
 * framing. Left out: the page's own pixel pass (the wallpaper's shader does
 * that work, see above), the tweak panel, drag-to-orbit and scroll-to-zoom.
 *
 * Changed to be a layer: it renders with three.js into the wallpaper's own
 * WebGL context, at one texel per wallpaper cell, over the page rows the
 * room covers (layout()); the wallpaper reads the tone-mapped target as its
 * room image (cells). three.js's view of the GL state is reset before each
 * use, as the wallpaper's draws change it behind its back.
 *
 * Changed for a background: once the image has its samples the loop stops
 * instead of idling on requestAnimationFrame, and shaders compile
 * asynchronously before the layer joins the wallpaper. The room fades in
 * from the wallpaper as its samples gather (presence(), ROOM_EASE_IN), each
 * new image starts from nothing rather than from half the old one
 * (freshStart()), and reflected and indirect light is clamped against
 * fireflies (clampFireflies()).
 *
 * Changed to follow the site's theme: the walls and floor start from the
 * page colour of the theme that is on, with the page's "Tauler light" or
 * "Tauler dark" backdrop preset, while the light stays the same (see
 * BACKDROP, SKY); a theme switch starts the image over.
 *
 * Changed to frame any box: see frame().
 *
 * Added: a Monstera in a pot at the left of the room, the scene's 8th group
 * (monstera-scene.js, put in by scripts/whiteroom-monstera.patch), which wide
 * frames show. The room shader learns its primitives and materials by
 * load-time edits (monstera-shader.ts, withPlant()), and its leaves' maps are
 * baked once, before the first sample (bakeLeaves()).
 */
import * as THREE from 'three'
import { installPathTracingChunks } from './vendor/PathTracingCommon.js'
import { buildRoomScene } from './vendor/room-scene.js'
import { rgbToOklab } from '@/lib/wallpaper/tauler-wallpaper.js'
import { ditherPixel, type WallpaperColours } from '../wallpaper/options'
import {
  isSoftware,
  type Fade,
  type RoomCells,
} from '../wallpaper/wallpaper-gl'
import {
  ROOM_DETAIL,
  ROOM_EASE_IN,
  ROOM_FADE,
  ROOM_MATCH,
  ROOM_WAVE,
} from './room-size'
import {
  LEAF_MAPS,
  LEAF_RES,
  MONSTERA_EDITS,
  ONE_TRACE,
} from './monstera-shader'

declare global {
  interface Window {
    /** Replaces ROOM_FADE, ROOM_WAVE's depth, ROOM_MATCH and ROOM_DETAIL before the room starts; for tests. */
    taulerRoomFade?: Partial<
      Fade & { depth: number; match: number; detail: number }
    >
    /** Stops the room at this many samples (0: none, the room not drawn); for tests. */
    taulerRoomSamples?: number
  }
}

const ASSETS = '/whiteroom'

let chunksInstalled = false

/**
 * True when the room can render in `gl`: hardware WebGL 2 with float render
 * targets. Software WebGL (SwiftShader, llvmpipe) is refused: a path tracer
 * there takes seconds per frame and freezes the page.
 */
export function roomSupported(gl: WebGL2RenderingContext): boolean {
  return !isSoftware(gl) && !!gl.getExtension('EXT_color_buffer_float')
}

async function loadShader(name: string) {
  const response = await fetch(`${ASSETS}/shaders/${name}.glsl`)
  if (!response.ok) throw new Error(`${name}: HTTP ${response.status}`)
  return response.text()
}

/** The page's shaders, fetched as soon as this module loads (Wallpaper.tsx loads it early). */
const shaders = Promise.all(
  (['vs', 'fs-copy', 'fs-out', 'fs-room'] as const).map(loadShader),
)
shaders.catch(() => {}) // reported by createRoom()

/** Applies each edit to its single anchor; throws if the vendored shader changed. */
function editShader(shader: string, edits: [string, string][]) {
  for (const [from, to] of edits) {
    if (shader.split(from).length !== 2)
      throw new Error(`vendored shader changed: no single "${from}"`)
    shader = shader.replace(from, to)
  }
  return shader
}

/** Replaces the span from `start` up to (not including) `end`; throws unless each appears once. */
function replaceSpan(
  shader: string,
  { start, end, text }: { start: string; end: string; text: string },
) {
  for (const anchor of [start, end])
    if (shader.split(anchor).length !== 2)
      throw new Error(`vendored shader changed: no single "${anchor}"`)
  return (
    shader.slice(0, shader.indexOf(start)) +
    text +
    shader.slice(shader.indexOf(end))
  )
}

/**
 * The room shader with the Monstera's primitives and materials, traced from
 * one intersection call site (see monstera-shader.ts).
 */
function withPlant(fsRoom: string) {
  return editShader(replaceSpan(fsRoom, ONE_TRACE), MONSTERA_EDITS)
}

/**
 * Fireflies: lone cells far brighter than their neighbours, in clusters
 * that 48 samples never average away (around the candle lamp, on the wall
 * behind it and in the glossy floor under it, along the chrome chairs). They
 * are the rare paths that reach a small, bright light (a candle bulb, the
 * uplight, the window) by way of a rough reflection or after bouncing off a
 * surface: one such sample, at up to 24 times the scene's light, outweighs
 * the other 47 of its pixel.
 *
 * So each of those contributions is clamped to FIREFLY_MAX in luminance
 * before it joins the sample: light reaching the camera by any reflection
 * (paths with a bounce before the light) and all light after the first
 * diffuse hit. Two kinds keep their full value, the real highlights: lights
 * the camera sees directly (the uplight's head, the candle bulbs) and the
 * direct light at the first surface hit (next-event estimation, already
 * capped at 14 by the renderer). Clamping the whole sample instead turned the
 * candle bulbs grey, and clamping the indirect light alone left the floor's
 * reflections sparkling.
 */
function clampFireflies(fsRoom: string) {
  return editShader(fsRoom, [
    [
      'vec3 CalculateRadiance(out vec3 objectNormal',
      `uniform float uFireflyMax;
// [tauler] fireflies: scales c down to uFireflyMax in luminance
vec3 fireflyClamp(vec3 c) {
	float l = dot(c, vec3(0.2126, 0.7152, 0.0722));
	return l > uFireflyMax ? c * (uFireflyMax / l) : c;
}
vec3 CalculateRadiance(out vec3 objectNormal`,
    ],
    [
      '\tint bounces = 0;\n',
      '\tint bounces = 0;\n\tbool indirect = false, shadowIndirect = false;   // [tauler] past the first diffuse hit\n',
    ],
    [
      'if (hitMat == shadowTarget && t < INFINITY) acc += shadowW;',
      'if (hitMat == shadowTarget && t < INFINITY) acc += shadowIndirect ? fireflyClamp(shadowW) : shadowW;',
    ],
    [
      'acc += mask * sky(rayDirection);',
      'acc += b > 0 ? fireflyClamp(mask * sky(rayDirection)) : mask * sky(rayDirection);',
    ],
    [
      'if (lastSpec) acc += mask * emission;',
      'if (lastSpec) acc += b > 0 ? fireflyClamp(mask * emission) : mask * emission;',
    ],
    [
      'shadowW = mask * min(w, vec3(14.0)); }',
      'shadowW = mask * min(w, vec3(14.0)); shadowIndirect = indirect; }',
    ],
    ['\t\trayOrigin = xo;\n', '\t\trayOrigin = xo;\n\t\tindirect = true;\n'],
  ])
}

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
const P0 = {
  exposure: 1.9, // the light theme's; see BACKDROP
  sky: 1.0,
  window: 1.0,
  uplight: 1.0,
  candles: 1.0,
  warmth: 0.5,
}
/** The page's sample count. */
const MAX_SAMPLES = 48

/**
 * The most luminance one reflected or indirect contribution brings to a
 * sample (see clampFireflies()), after exposure: the shader's limit is this
 * over the theme's exposure, so it bites at the same displayed brightness in
 * both themes. 2 is about 92% of white after the filmic curve. At 3 the
 * dark theme's floor still sparkled under the candle lamp; at 1 the glossy
 * floor's reflections and the walls lost a few percent of their light.
 */
const FIREFLY_MAX = 2

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
const BACKDROP = {
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
 * page's "Full colour" backdrop in both themes (see skyApart()), so a dark
 * theme darkens the walls and floor, not the light falling on them.
 */
const SKY = linToLab(oklchToLin(0.971, 0.01, 87.5))

/**
 * The page's room shader lights the room with its backdrop colour: escaping
 * rays return it (sky()). Give the sky a colour of its own, uSkyLab, so the
 * backdrop is the walls' and floor's base colour only. And declare uVShift
 * and uVMin, for offCentre().
 */
function skyApart(fsRoom: string) {
  return editShader(fsRoom, [
    [
      'uniform vec3 uBgLab;',
      'uniform vec3 uBgLab;\nuniform vec3 uSkyLab;\nuniform float uVShift, uVMin;',
    ],
    [
      'vec3 sky(vec3 d) { return clamp(okToLin(uBgLab), 0.0, 1.0) * uSky; }',
      'vec3 sky(vec3 d) { return clamp(okToLin(uSkyLab), 0.0, 1.0) * uSky; }',
    ],
  ])
}

/**
 * The renderer's camera looks through the middle of its image plane. Shift
 * the plane vertically by uVShift (in the plane's units, tangents of the
 * vertical angle), so the image can run on below the view the framing asks
 * for and its top can stay below the window; and look no lower than uVMin:
 * rows below it repeat the floor seen there (see frame()).
 */
function offCentre(main: string) {
  return editShader(main, [
    [
      '(camUp * pixelPos.y * uVLen) + camForward',
      '(camUp * max(pixelPos.y * uVLen + uVShift, uVMin)) + camForward',
    ],
  ])
}

/**
 * The renderer starts an image over (its first frame, uFrameCounter 1) by
 * keeping half of the old one as a motion trail and halving the new sample,
 * for a camera being dragged. Here nothing moves: a new image follows a
 * resize or a theme switch, where the old one is the wrong size or colour.
 * So start from nothing, with the whole first sample; otherwise the first
 * frame is half as bright, and every later one darker by that half sample.
 */
function freshStart(main: string) {
  return editShader(main, [
    [
      'if (uFrameCounter == 1.0) // camera just moved after being still',
      'if (uFrameCounter == 1.0) previousPixel = vec4(0); // [tauler] a fresh start\n\telse if (false)',
    ],
  ])
}

/**
 * How much of the room shows after `samples` samples, per OKLCH channel: each
 * rises along a smootherstep, from 0 with no samples to 1 at ROOM_EASE_IN
 * samples for the slowest channel, the others sooner in the ratio of their
 * shares of the fade (lightness last, the channel the eye follows).
 */
function presence(samples: number, fade: Fade): Fade {
  const slowest = Math.max(fade.L, fade.C, fade.h)
  const ease = (share: number) => {
    const t = Math.min(1, (samples * slowest) / (ROOM_EASE_IN * share))
    return t * t * t * (t * (t * 6 - 15) + 10)
  }
  return { L: ease(fade.L), C: ease(fade.C), h: ease(fade.h) }
}

/** The site's theme: next-themes puts `.dark` on <html>. */
function siteTheme(): keyof typeof BACKDROP {
  return document.documentElement.classList.contains('dark') ? 'dark' : 'light'
}

/** The room's visible box on the page, in device px from the page's top-left. */
export interface RoomRegion {
  top: number
  height: number
  width: number
  dpr: number
}

export interface RoomLayer {
  /**
   * Lays the room out over `region` in these colours; starts the image over
   * (and renders its first sample) if either changed since the last call.
   */
  layout(region: RoomRegion, colours: WallpaperColours): void
  /** The room image for the wallpaper's cells pass; null before layout(). */
  readonly cells: RoomCells | null
  destroy(): void
}

/**
 * Starts the room in `gl`, the wallpaper's context, which must be hardware
 * (roomSupported()). Rejects if the assets fail to load or a shader fails to
 * compile. `onFrame` runs after every new sample, for the wallpaper to draw
 * it; `element.dataset.converged` is the sample count once the image has
 * all its samples, and absent while it renders.
 */
export async function createRoom(
  gl: WebGL2RenderingContext,
  {
    signal,
    element,
    onFrame,
  }: { signal: AbortSignal; element: HTMLElement; onFrame: () => void },
): Promise<RoomLayer> {
  if (!chunksInstalled) {
    installPathTracingChunks(THREE)
    const chunks = THREE.ShaderChunk as Record<string, string>
    chunks.pathtracing_main = freshStart(offCentre(chunks.pathtracing_main))
    chunksInstalled = true
  }

  const [[vs, fsCopy, fsOut, fsRoom], blue] = await Promise.all([
    shaders,
    new THREE.TextureLoader().loadAsync(`${ASSETS}/BlueNoise_R_128.png`),
  ])
  signal.throwIfAborted()
  blue.wrapS = blue.wrapT = THREE.RepeatWrapping
  blue.flipY = false
  blue.minFilter = blue.magFilter = THREE.NearestFilter
  blue.generateMipmaps = false

  const overrides = window.taulerRoomFade ?? {}
  const fade: Fade = {
    L: overrides.L ?? ROOM_FADE.L,
    C: overrides.C ?? ROOM_FADE.C,
    h: overrides.h ?? ROOM_FADE.h,
  }
  const wave = { ...ROOM_WAVE, depth: overrides.depth ?? ROOM_WAVE.depth }

  // three.js renders only into its own targets: it never draws to, or
  // resizes, the canvas, which is the wallpaper's
  const renderer = new THREE.WebGLRenderer({
    canvas: gl.canvas as HTMLCanvasElement,
    context: gl,
  })
  renderer.autoClear = false
  renderer.toneMapping = THREE.NoToneMapping
  let shaderError: string | null = null
  renderer.debug.onShaderError = (g, program, _vs, fs) => {
    shaderError =
      g.getShaderInfoLog(fs) || g.getProgramInfoLog(program) || 'unknown'
  }

  // ── Scene data ─────────────────────────────────────────────────────────
  const S = buildRoomScene()
  const packed = S.pack()
  const primTex = new THREE.DataTexture(
    packed.data,
    packed.width,
    packed.count,
    THREE.RGBAFormat,
    THREE.FloatType,
  )
  primTex.minFilter = primTex.magFilter = THREE.NearestFilter
  primTex.generateMipmaps = false
  primTex.needsUpdate = true
  const G = packed.groups
  const gInv: THREE.Matrix4[] = [],
    gRot: THREE.Matrix3[] = [],
    gMin: THREE.Vector3[] = [],
    gMax: THREE.Vector3[] = [],
    gRange = new Int32Array(16)
  for (let i = 0; i < 8; i++) {
    const g = G[i]
    const m = new THREE.Matrix4()
    if (g) m.makeRotationY(g.rotY).setPosition(g.pos[0], g.pos[1], g.pos[2])
    const rot = new THREE.Matrix3().setFromMatrix4(m)
    gRot.push(rot)
    gInv.push(m.clone().invert())
    gMin.push(new THREE.Vector3().fromArray(g ? g.min : [0, 0, 0]))
    gMax.push(new THREE.Vector3().fromArray(g ? g.max : [0, 0, 0]))
    gRange[i * 2] = g ? g.start : 0
    gRange[i * 2 + 1] = g ? g.end : 0
  }
  // candle bulb world positions
  const cg = G.filter((g) => g.name === 'candle')[0]
  const cm = new THREE.Matrix4()
    .makeRotationY(cg.rotY)
    .setPosition(cg.pos[0], cg.pos[1], cg.pos[2])
  const bulbs = S.bulbs.map((b) =>
    new THREE.Vector3().fromArray(b).applyMatrix4(cm),
  )

  /**
   * The leaves' maps (outline, splits, holes, droop, veins), one LEAF_RES
   * square per leaf, filtered: baked once by LEAF_MAPS (bakeLeaves()).
   */
  const leafRT = new THREE.WebGLRenderTarget(
    LEAF_RES,
    LEAF_RES * S.leafMats.length,
    {
      minFilter: THREE.LinearFilter,
      magFilter: THREE.LinearFilter,
      format: THREE.RGBAFormat,
      type: THREE.HalfFloatType,
      depthBuffer: false,
      stencilBuffer: false,
    },
  )

  let W = 2,
    H = 2
  const rtOpts: THREE.RenderTargetOptions = {
    minFilter: THREE.NearestFilter,
    magFilter: THREE.NearestFilter,
    format: THREE.RGBAFormat,
    type: THREE.FloatType,
    depthBuffer: false,
    stencilBuffer: false,
  }
  const ptRT = new THREE.WebGLRenderTarget(W, H, rtOpts),
    copyRT = new THREE.WebGLRenderTarget(W, H, rtOpts)

  const camera = new THREE.PerspectiveCamera(40, 1.5, 1, 5000)
  const target = new THREE.Vector3(5, 118, 0)
  const orbit = { yaw: 0, pitch: Math.atan2(2, 630), dist: Math.hypot(630, 2) }

  const vlen = Math.tan(THREE.MathUtils.degToRad(20))
  const u = {
    tPreviousTexture: { value: copyRT.texture },
    tBlueNoiseTexture: { value: blue },
    uCameraMatrix: { value: new THREE.Matrix4() },
    uResolution: { value: new THREE.Vector2(W, H) },
    uRandomVec2: { value: new THREE.Vector2() },
    uEPS_intersect: { value: 0.01 },
    uTime: { value: 0 },
    uSampleCounter: { value: 0 },
    uPreviousSampleCount: { value: 1 },
    uFrameCounter: { value: 1 },
    uULen: { value: vlen * 1.5 },
    uVLen: { value: vlen },
    uVShift: { value: 0 },
    uVMin: { value: -1e3 },
    uApertureSize: { value: 0 },
    uFocusDistance: { value: 500 },
    uCameraIsMoving: { value: false },
    uUseOrthographicCamera: { value: false },
    uSceneIsDynamic: { value: false },
    tPrims: { value: primTex },
    tLeaf: { value: leafRT.texture },
    uNumGroups: { value: G.length },
    uGInv: { value: gInv },
    uGRot: { value: gRot },
    uGMin: { value: gMin },
    uGMax: { value: gMax },
    uGRange: { value: gRange },
    uWinC0: { value: new THREE.Vector3().fromArray(S.window.c0) },
    uWinE1: { value: new THREE.Vector3().fromArray(S.window.e1) },
    uWinE2: { value: new THREE.Vector3().fromArray(S.window.e2) },
    uLampC0: { value: new THREE.Vector3().fromArray(S.lamp.c0) },
    uLampE1: { value: new THREE.Vector3().fromArray(S.lamp.e1) },
    uLampE2: { value: new THREE.Vector3().fromArray(S.lamp.e2) },
    uBulbs: { value: bulbs },
    uBgLab: { value: new THREE.Vector3(0.971, 0.0004, 0.01) },
    uSkyLab: { value: new THREE.Vector3(...SKY) },
    uRingL: { value: 0.72 },
    uRingC: { value: 0.14 },
    uCellI: { value: 0.8 },
    uCellSize: { value: 220 },
    uCellHues: { value: 6 },
    uSeed: { value: 18 },
    uWash: { value: 0.5 },
    uWashSize: { value: 160 },
    uSky: { value: P0.sky },
    uWindow: { value: P0.window },
    uUplight: { value: P0.uplight },
    uCandles: { value: P0.candles },
    uWarmth: { value: P0.warmth },
    uFireflyMax: { value: FIREFLY_MAX / P0.exposure },
  }
  const tri = new THREE.BufferGeometry()
  tri.setAttribute(
    'position',
    new THREE.Float32BufferAttribute([-1, -1, 0, 3, -1, 0, -1, 3, 0], 3),
  )
  const materials: THREE.ShaderMaterial[] = []
  function quadMesh(mat: THREE.ShaderMaterial) {
    materials.push(mat)
    const me = new THREE.Mesh(tri, mat)
    me.frustumCulled = false
    return me
  }
  const ptScene = new THREE.Scene(),
    copyScene = new THREE.Scene(),
    outScene = new THREE.Scene()
  const ortho = new THREE.OrthographicCamera(-1, 1, 1, -1, 0, 1)
  const leafScene = new THREE.Scene()
  leafScene.add(
    quadMesh(
      new THREE.ShaderMaterial({
        uniforms: {
          uLeafMat: {
            value: Array.from({ length: 16 }, (_, i) => S.leafMats[i] ?? 0),
          },
        },
        vertexShader: vs,
        fragmentShader: LEAF_MAPS,
        depthTest: false,
        depthWrite: false,
      }),
    ),
  )
  /** Draws the leaves' maps into leafRT, once, before the first sample. */
  function bakeLeaves() {
    renderer.resetState()
    renderer.setRenderTarget(leafRT)
    renderer.render(leafScene, ortho)
    renderer.setRenderTarget(null)
  }
  ptScene.add(
    quadMesh(
      new THREE.ShaderMaterial({
        uniforms: u,
        vertexShader: vs,
        fragmentShader: clampFireflies(withPlant(skyApart(fsRoom))),
        depthTest: false,
        depthWrite: false,
      }),
    ),
  )
  copyScene.add(
    quadMesh(
      new THREE.ShaderMaterial({
        uniforms: { tPathTracedImageTexture: { value: ptRT.texture } },
        vertexShader: vs,
        fragmentShader: fsCopy,
        depthTest: false,
        depthWrite: false,
      }),
    ),
  )
  const ou = {
    tPathTracedImageTexture: { value: ptRT.texture },
    uSampleCounter: { value: 0 },
    uOneOverSampleCounter: { value: 0 },
    uPixelEdgeSharpness: { value: 0.75 },
    uEdgeSharpenSpeed: { value: 0.05 },
    uCameraIsMoving: { value: false },
    uSceneIsDynamic: { value: false },
    uUseToneMapping: { value: true },
    uExposure: { value: 1 },
  }
  // filmic curve in place of the demo's plain Reinhard; the output shader
  // applies sqrt() for display gamma, and the wallpaper reads the result as
  // display sRGB
  const outFrag = fsOut.replace(
    'void main()',
    'uniform float uExposure;\nvec3 ReinhardToneMapping(vec3 c) { c *= uExposure; vec3 x = max(vec3(0.0), c);\n' +
      '  vec3 m = (x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14); return clamp(m * m, 0.0, 1.0); }\n\nvoid main()',
  )
  outScene.add(
    quadMesh(
      new THREE.ShaderMaterial({
        uniforms: ou,
        vertexShader: vs,
        fragmentShader: outFrag,
        depthTest: false,
        depthWrite: false,
        toneMapped: false,
      }),
    ),
  )

  /** The room's continuous colour, one texel per cell: what the wallpaper reads. */
  const outRT = new THREE.WebGLRenderTarget(W, H, {
    minFilter: THREE.NearestFilter,
    magFilter: THREE.NearestFilter,
    format: THREE.RGBAFormat,
    type: THREE.UnsignedByteType,
    depthBuffer: false,
    stencilBuffer: false,
  })

  /** The site theme's backdrop, on the wallpaper's colours. */
  function applyBackdrop(colours: WallpaperColours) {
    const backdrop = BACKDROP[siteTheme()]
    ou.uExposure.value = backdrop.exposure
    u.uFireflyMax.value = FIREFLY_MAX / backdrop.exposure
    u.uCellI.value = backdrop.cellI
    u.uCellHues.value = backdrop.cellHues
    u.uCellSize.value = backdrop.cellSize
    u.uWash.value = backdrop.wash
    u.uSeed.value = backdrop.seed
    u.uRingL.value = colours.ringL
    u.uRingC.value = colours.ringC
    const lab = rgbToOklab(colours.background)
    u.uBgLab.value.set(lab[0], lab[1], lab[2])
  }

  const cp = Math.cos(orbit.pitch)
  camera.position.set(
    target.x + Math.sin(orbit.yaw) * cp * orbit.dist,
    target.y + Math.sin(orbit.pitch) * orbit.dist,
    target.z + Math.cos(orbit.yaw) * cp * orbit.dist,
  )
  camera.lookAt(target)
  camera.updateMatrixWorld(true)
  u.uCameraMatrix.value.copy(camera.matrixWorld)

  /**
   * The highest the view may reach, as the tangent of the angle above the
   * camera's axis: a little below the window's sill, where it meets the
   * wall's face. The window is a light, near white in both themes; seen at
   * the top of the room, it was a hard bright band in the fade into the
   * dark wallpaper. Below it the top of the room is wall, the page colour.
   */
  const sill = new THREE.Vector3(
    S.window.c0[0],
    S.window.c0[1],
    0,
  ).applyMatrix4(camera.matrixWorldInverse)
  const topLimit = sill.y / -sill.z - 0.03
  /**
   * The lowest the view may look: a little above the window's reflection in
   * the glossy floor (the sill mirrored in it), a bright stripe across the
   * floor that tall boxes, and the image's run below the first screen,
   * would otherwise reach. Rows below look the same way, so they continue
   * the floor at that depth, grain and all, with nothing new in it.
   */
  const mirrored = new THREE.Vector3(
    S.window.c0[0],
    -S.window.c0[1],
    S.window.c0[2],
  ).applyMatrix4(camera.matrixWorldInverse)
  u.uVMin.value = mirrored.y / -mirrored.z + 0.08

  /**
   * Frames the camera for a visible box `aspect` wide to 1 high whose image
   * runs on beyond it (`total` of the visible height; layout() places it),
   * `width` the image's width as a share of the box's (whole cells).
   *
   * The page's rule sets the field of view: a 40° vertical view, widened
   * horizontally to fit, and on boxes narrower than 3:2 the 3:2 horizontal
   * view, widened vertically (to at most 37° each way). The view's top is
   * then held below the window (topLimit), the plane shifted down rather
   * than tilted, so walls stay upright: tall boxes see more floor rather
   * than the window band and the wall above it.
   */
  function frame(aspect: number, total: number, width: number) {
    const hHalf = Math.atan(1.5 * vlen)
    let v = aspect >= 1.5 ? vlen : Math.tan(hHalf) / aspect
    v = Math.min(v, Math.tan(THREE.MathUtils.degToRad(37)))
    const top = Math.min(v, topLimit)
    // the plane spans top .. top - 2v over the visible box, on at that rate
    u.uVLen.value = v * total
    u.uVShift.value = top - v * total
    u.uULen.value = v * aspect * width
  }

  // ── Layout: one texel per cell of the rows the room covers ──────────────
  let cells: RoomCells | null = null
  let laidOut = ''
  function layout(region: RoomRegion, colours: WallpaperColours) {
    const key = JSON.stringify([region, colours, siteTheme()])
    if (key === laidOut) return
    laidOut = key
    applyBackdrop(colours)
    const devP = ditherPixel(region.dpr)
    // the top fade starts along a wave around the box's top (a row's top),
    // the bottom one along a wave below where the box ends; the image runs
    // from the box's top until both fades have finished, and the wallpaper's
    // shader stretches its first row up to the wave's highest point
    const top = Math.ceil(region.top / devP) * devP
    const bottom = region.top + region.height
    const reach = Math.max(fade.L, fade.C, fade.h) + wave.depth
    const end = bottom + reach * region.height
    const rows: [number, number] = [
      Math.floor(region.top / devP),
      Math.ceil(end / devP) - 1,
    ]
    W = Math.max(2, Math.ceil(region.width / devP))
    H = Math.max(2, rows[1] - rows[0] + 1)
    ptRT.setSize(W, H)
    copyRT.setSize(W, H)
    outRT.setSize(W, H)
    u.uResolution.value.set(W, H)
    // the image's top edge is the first row's, perhaps above the box's top
    frame(
      region.width / region.height,
      (H * devP) / region.height,
      (W * devP) / region.width,
    )
    u.uVShift.value +=
      (2 * u.uVLen.value * (region.top - rows[0] * devP)) / (H * devP)
    renderer.resetState()
    renderer.initRenderTarget(outRT)
    cells = {
      // three.js's own texture object for the target: what it renders into
      texture: (
        renderer.properties.get(outRT.texture) as {
          __webglTexture: WebGLTexture
        }
      ).__webglTexture,
      rows,
      from: Math.floor((top - wave.depth * wave.rise * region.height) / devP),
      cols: W,
      top,
      bottom,
      height: region.height,
      fade,
      wave,
      match: overrides.match ?? ROOM_MATCH,
      detail: overrides.detail ?? ROOM_DETAIL,
      presence: { L: 0, C: 0, h: 0 },
    }
    samples = 0
    resetPending = true
    if (samples < maxSamples) step()
    start()
  }

  const maxSamples = Math.min(
    MAX_SAMPLES,
    window.taulerRoomSamples ?? MAX_SAMPLES,
  )
  let samples = 0,
    frameCount = 1,
    resetPending = true
  const t0 = performance.now()

  function step() {
    const isMoving = resetPending
    resetPending = false
    if (!isMoving) {
      samples += 1
      frameCount += 1
    } else {
      u.uPreviousSampleCount.value = samples || 1
      frameCount = 1
      samples = 1
    }
    u.uTime.value = ((performance.now() - t0) / 1000) % 1000
    u.uCameraIsMoving.value = isMoving
    u.uSampleCounter.value = samples
    u.uFrameCounter.value = frameCount
    u.uRandomVec2.value.set(Math.random(), Math.random())
    ou.uCameraIsMoving.value = isMoving
    ou.uSampleCounter.value = samples
    ou.uOneOverSampleCounter.value = 1 / samples
    renderer.resetState()
    renderer.setRenderTarget(ptRT)
    renderer.render(ptScene, camera)
    renderer.setRenderTarget(copyRT)
    renderer.render(copyScene, ortho)
    renderer.setRenderTarget(outRT)
    renderer.render(outScene, ortho)
    renderer.setRenderTarget(null)
    // the room fades in from the wallpaper as its samples gather
    if (cells) cells.presence = presence(samples, fade)
  }

  // ── Loop ───────────────────────────────────────────────────────────────
  let raf = 0
  function loop() {
    raf = 0
    if (samples < maxSamples) {
      step()
      onFrame()
    }
    // The page idles on requestAnimationFrame from here; a background stops.
    if (samples < maxSamples) start()
    else element.dataset.converged = String(samples)
  }
  function start() {
    delete element.dataset.converged
    if (!raf && compiled) raf = requestAnimationFrame(loop)
  }

  // ── Compile before the layer joins the wallpaper ───────────────────────
  // three.js picks a shader's variant from the render target current when
  // it compiles, so each pass compiles against the target it draws into.
  let compiled = false
  renderer.resetState()
  renderer.setRenderTarget(ptRT)
  const compiling = [renderer.compileAsync(ptScene, camera)]
  renderer.setRenderTarget(copyRT)
  compiling.push(renderer.compileAsync(copyScene, ortho))
  renderer.setRenderTarget(outRT)
  compiling.push(renderer.compileAsync(outScene, ortho))
  renderer.setRenderTarget(leafRT)
  compiling.push(renderer.compileAsync(leafScene, ortho))
  renderer.setRenderTarget(null)

  const destroy = () => {
    cancelAnimationFrame(raf)
    raf = 0
    compiled = false
    cells = null
    renderer.resetState()
    for (const disposable of [
      ptRT,
      copyRT,
      outRT,
      leafRT,
      tri,
      primTex,
      blue,
      ...materials,
      renderer,
    ])
      disposable.dispose()
  }

  try {
    await Promise.all(compiling)
    if (shaderError) throw new Error(`shader failed to compile: ${shaderError}`)
    signal.throwIfAborted()
  } catch (error) {
    destroy()
    throw error
  }
  bakeLeaves()
  compiled = true

  return {
    layout,
    get cells() {
      return cells
    },
    destroy,
  }
}
