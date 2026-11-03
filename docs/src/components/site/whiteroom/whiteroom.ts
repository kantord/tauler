/**
 * "White Room, Pixel Traced": a white room path-traced live by Erich
 * Loftis's THREE.js PathTracing Renderer (CC0,
 * https://github.com/erichlof/THREE.js-PathTracing-Renderer), then reduced to
 * dithered pixel art with the tauler wallpaper's palette rules.
 *
 * What renders runs unchanged (scripts/vendor-whiteroom.sh): the renderer's
 * GLSL chunks and blue noise, the page's five shaders, and its scene
 * description, on the three.js revision it ships (r185).
 *
 * The page's main script is ported here by hand, with its defaults: the
 * "Photo match" lights, the "Soft" pixel palette and the "Full colour"
 * backdrop, 48 samples. Kept line for line: the uniforms,
 * the four passes (path trace into a float target, copy it into the
 * accumulation target, filmic tone-map into an 8-bit target, quantize to
 * pixel art on screen), the resize rule (whole device pixels per dither
 * pixel, the photo's framing), and the wall measurement that anchors the
 * palette. Left out: the tweak panel, drag-to-orbit and scroll-to-zoom.
 *
 * Changed for a background: once the image has its samples the loop stops
 * instead of idling on requestAnimationFrame, and shaders compile
 * asynchronously before the canvas joins the page.
 *
 * Changed to sit on the wallpaper: the dither pixels are the wallpaper's
 * pixels, on its grid. The page's are 4 CSS px, counted from the canvas's
 * bottom-left, with the Bayer pattern phased by the canvas height. Here they
 * take the wallpaper's size (3 device px at any zoom, asked of the wallpaper
 * module itself), and the pixel pass counts them from the top-left of the
 * window, as the wallpaper does: the container is the window, and the canvas
 * fills its bottom ROOM_HEIGHT. So every render pixel covers exactly one
 * wallpaper pixel, and the dither pattern runs on from the wallpaper's.
 */
import * as THREE from 'three'
import { installPathTracingChunks } from './vendor/PathTracingCommon.js'
import { buildRoomScene } from './vendor/room-scene.js'
import {
  computeWallpaper,
  readCssColors,
} from '@/lib/wallpaper/tauler-wallpaper.js'
import { WALLPAPER_OPTIONS } from '../Wallpaper'
import { ROOM_FADE, ROOM_HEIGHT } from './room-size'

const ASSETS = '/whiteroom'

/** See roomSupported(). */
const CONTEXT: WebGLContextAttributes = {
  antialias: false,
  alpha: false,
  preserveDrawingBuffer: true,
  failIfMajorPerformanceCaveat: true,
}

const SOFTWARE_RENDERER =
  /swiftshader|llvmpipe|softpipe|lavapipe|software|basic render driver/i

let chunksInstalled = false

export interface Room {
  destroy(): void
}

/**
 * True when the browser can run the renderer: hardware WebGL 2 with float
 * render targets. Software WebGL (SwiftShader, llvmpipe) is refused: a path
 * tracer there takes seconds per frame and freezes the page. Headless Chrome
 * hands out SwiftShader despite `failIfMajorPerformanceCaveat`, so the
 * renderer's name is checked as well.
 */
export function roomSupported(): boolean {
  const gl = document.createElement('canvas').getContext('webgl2', CONTEXT)
  if (!gl) return false
  const debug = gl.getExtension('WEBGL_debug_renderer_info')
  const name = String(
    gl.getParameter(debug ? debug.UNMASKED_RENDERER_WEBGL : gl.RENDERER),
  )
  const ok =
    !SOFTWARE_RENDERER.test(name) && !!gl.getExtension('EXT_color_buffer_float')
  gl.getExtension('WEBGL_lose_context')?.loseContext()
  return ok
}

async function loadShader(name: string, signal: AbortSignal) {
  const response = await fetch(`${ASSETS}/shaders/${name}.glsl`, { signal })
  if (!response.ok) throw new Error(`${name}: HTTP ${response.status}`)
  return response.text()
}

// ── Colour helpers, from the page ──────────────────────────────────────
type RGB = [number, number, number]
// hexToLin, on the sRGB bytes the wallpaper reads its CSS colour into
function srgbToLin(rgb: RGB): RGB {
  return rgb.map((v) => {
    v /= 255
    return v <= 0.04045 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4)
  }) as RGB
}
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
  exposure: 1.9,
  sky: 1.0,
  window: 1.0,
  uplight: 1.0,
  candles: 1.0,
  warmth: 0.5,
}
/** Pixel art: the "Soft" palette. */
const PX0 = {
  // pixel: the wallpaper's (see wallpaperPixel()), not the page's 4 CSS px
  samples: 48,
  stepL: 0.07,
  stepC: 0.04,
  stepH: 30,
  cut: 0.02,
  dither: 1,
  hue: 0,
  matrix: 0,
  shared: true,
  anchor: true,
}
/**
 * Backdrop: the page's "Tauler light" and "Tauler dark" presets, one per site
 * theme, in place of its "Full colour" default. Their colour (the page's
 * `bg`), ring lightness and ring chroma are not here: they are the
 * wallpaper's, read from its CSS variables (see backdropColours()), so the
 * room's walls start from the page colour of whichever theme is on.
 */
const BACKDROP = {
  light: {
    cellI: 0.35,
    cellHues: 3,
    cellSize: 300,
    wash: 1,
    seed: 18,
    exposure: 1.9,
  },
  dark: {
    cellI: 0.5,
    cellHues: 3,
    cellSize: 300,
    wash: 1,
    seed: 18,
    exposure: 1.9,
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
 * backdrop is the walls' and floor's base colour only.
 */
function skyApart(fsRoom: string) {
  return editShader(fsRoom, [
    ['uniform vec3 uBgLab;', 'uniform vec3 uBgLab;\nuniform vec3 uSkyLab;'],
    [
      'vec3 sky(vec3 d) { return clamp(okToLin(uBgLab), 0.0, 1.0) * uSky; }',
      'vec3 sky(vec3 d) { return clamp(okToLin(uSkyLab), 0.0, 1.0) * uSky; }',
    ],
  ])
}

/** The site's theme: next-themes puts `.dark` on <html>. */
function siteTheme(): keyof typeof BACKDROP {
  return document.documentElement.classList.contains('dark') ? 'dark' : 'light'
}

/**
 * The wallpaper's colours where `element` sits: its background (the page
 * colour, sRGB 0-255) and its hue ring, read as the wallpaper reads them.
 */
function backdropColours(element: HTMLElement) {
  const {
    background = [255, 255, 255],
    ringL = 0.7,
    ringC = 0.12,
  } = readCssColors(element)
  return { background, ringL, ringC }
}

/** The wallpaper's dither pixel at this device pixel ratio, in device px. */
function wallpaperPixel(dpr: number) {
  return computeWallpaper({ ...WALLPAPER_OPTIONS, width: 1, height: 1, dpr })
    .devP
}

/**
 * The page's pixel pass counts dither pixels from the bottom-left of the
 * canvas. Count them from the top of the window instead: uGridBottom is the
 * canvas's bottom edge, in device px from the window's top.
 */
function onWindowGrid(fsPix: string) {
  const edits: [string, string][] = [
    ['void main()', 'uniform int uGridBottom;\n\nvoid main()'],
    [
      'ivec2 dp = ivec2(gl_FragCoord.xy) / uDevP;',
      'int row = (uGridBottom - 1 - int(gl_FragCoord.y)) / uDevP;\n' +
        '\tivec2 dp = ivec2(int(gl_FragCoord.x) / uDevP, (uGridBottom - 1) / uDevP - row);',
    ],
    ['ivec2 g = ivec2(dp.x, -dp.y);', 'ivec2 g = ivec2(dp.x, row);'],
  ]
  return editShader(fsPix, edits)
}

/** Applies each edit to its single anchor; throws if the vendored shader changed. */
function editShader(shader: string, edits: [string, string][]) {
  for (const [from, to] of edits) {
    if (shader.split(from).length !== 2)
      throw new Error(`vendored shader changed: no single "${from}"`)
    shader = shader.replace(from, to)
  }
  return shader
}

/**
 * The fade into the wallpaper (ROOM_FADE), in the pixel pass; runs after
 * onWindowGrid(), whose `row` it uses. tWall holds the wallpaper's own pixel
 * for every cell of the fade, from uWallRow0 (a window-grid row) down.
 *
 * Two steps, one on each side of the quantize:
 *
 * 1. Before it, the render's continuous colour eases toward the wallpaper's,
 *    each OKLCH channel by its own weight: 1 at the render's top edge, 0 at
 *    ROOM_FADE's share of its height (smoothstep). Hue takes the shorter arc,
 *    and the hue of a grey (either side) counts as the other's.
 * 2. After it, each channel is the wallpaper's own value where the cell's
 *    ordered-dither threshold falls below that channel's weight. The
 *    quantize's grid (its lightness steps are anchored on the room's wall)
 *    is not the wallpaper's, so step 1 alone would land near the top on
 *    render levels around the wallpaper's colour, a coarser dither than the
 *    wallpaper's, and then jump to it. Step 2 hands over cell by cell instead,
 *    and where all three channels go to the wallpaper the cell is its exact
 *    sRGB bytes. Every channel of every output cell is on one of the two
 *    palettes' grids, so the fade stays crisp pixel art.
 */
function fadeIntoWallpaper(fsPix: string) {
  return editShader(fsPix, [
    [
      'void main()',
      'uniform sampler2D tWall;\nuniform int uWallRow0;\n' +
        'uniform float uRenderTop, uRenderHeight;  // device px, from the window top\n' +
        'uniform vec3 uFade;                       // L, C, h: ROOM_FADE\n' +
        '\n' +
        'void main()',
    ],
    [
      'float L = lab.x, C = length(lab.yz), h = degrees(atan(lab.z, lab.y));',
      `float L = lab.x, C = length(lab.yz), h = degrees(atan(lab.z, lab.y));
	float depth = ((float(row) + 0.5) * float(uDevP) - uRenderTop) / uRenderHeight;
	bool fading = depth < max(uFade.x, max(uFade.y, uFade.z));
	vec3 fadeW = vec3(0.0), wallRgb = vec3(0.0);
	float Lw = 0.0, Cw = 0.0, hw = 0.0;
	const float GREY = 0.005;
	if (fading) {
		fadeW = 1.0 - smoothstep(vec3(0.0), uFade, vec3(depth));
		wallRgb = texelFetch(tWall, ivec2(dp.x, row - uWallRow0), 0).rgb;
		vec3 wl = lin2oklab(wallRgb * wallRgb);   // in the pass's own space: it encodes with sqrt()
		Lw = wl.x; Cw = length(wl.yz); hw = degrees(atan(wl.z, wl.y));
		float dh = mod(hw - h + 540.0, 360.0) - 180.0;
		if (C < GREY) h = hw;
		else if (Cw >= GREY) h += dh * fadeW.z;
		L = mix(L, Lw, fadeW.x);
		C = mix(C, Cw, fadeW.y);
	}`,
    ],
    [
      'pc_fragColor = vec4(sqrt(lchToLin(Lq, Cq, radians(hq))), 1.0);',
      `if (fading) {
		float t = threshold(g + ivec2(4, 2));
		bvec3 wall = lessThan(vec3(t), fadeW);
		if (all(wall)) { pc_fragColor = vec4(wallRgb, 1.0); return; }
		if (wall.x) Lq = Lw;
		if (wall.y) Cq = Cw;
		if (wall.z && Cw >= GREY) hq = hw;
	}
	pc_fragColor = vec4(sqrt(lchToLin(Lq, Cq, radians(hq))), 1.0);`,
    ],
  ])
}

/**
 * Starts rendering into a canvas across the bottom ROOM_HEIGHT of
 * `container`, which must cover the window exactly, as the wallpaper does:
 * its top-left is the origin of the pixel grid.
 * Rejects if the assets fail to load or a shader fails to compile; `onLost`
 * runs if the GPU drops the context later.
 */
export async function mountRoom(
  container: HTMLElement,
  { signal, onLost }: { signal: AbortSignal; onLost?: () => void },
): Promise<Room> {
  if (!chunksInstalled) {
    installPathTracingChunks(THREE)
    chunksInstalled = true
  }

  const [vs, fsCopy, fsOut, fsPix, fsRoom, blue] = await Promise.all([
    loadShader('vs', signal),
    loadShader('fs-copy', signal),
    loadShader('fs-out', signal),
    loadShader('fs-pix', signal),
    loadShader('fs-room', signal),
    new THREE.TextureLoader().loadAsync(`${ASSETS}/BlueNoise_R_128.png`),
  ])
  signal.throwIfAborted()
  blue.wrapS = blue.wrapT = THREE.RepeatWrapping
  blue.flipY = false
  blue.minFilter = blue.magFilter = THREE.NearestFilter
  blue.generateMipmaps = false

  const canvas = document.createElement('canvas')
  // opaque: its top fades into the wallpaper in the pixel pass (ROOM_FADE)
  canvas.style.cssText = `display:block;position:absolute;left:0;bottom:0;width:100%;height:${ROOM_HEIGHT}`
  const gl = canvas.getContext('webgl2', CONTEXT)
  if (!gl) throw new Error('hardware WebGL 2 is not available')
  gl.getExtension('EXT_color_buffer_float')

  const renderer = new THREE.WebGLRenderer({ canvas, context: gl })
  renderer.autoClear = false
  renderer.toneMapping = THREE.NoToneMapping
  renderer.setPixelRatio(1)
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
    uApertureSize: { value: 0 },
    uFocusDistance: { value: 500 },
    uCameraIsMoving: { value: false },
    uUseOrthographicCamera: { value: false },
    uSceneIsDynamic: { value: false },
    tPrims: { value: primTex },
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
    uSky: { value: 1 },
    uWindow: { value: 1 },
    uUplight: { value: 1 },
    uCandles: { value: 1 },
    uWarmth: { value: 0.5 },
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
  ptScene.add(
    quadMesh(
      new THREE.ShaderMaterial({
        uniforms: u,
        vertexShader: vs,
        fragmentShader: skyApart(fsRoom),
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
  // filmic curve in place of the demo's plain Reinhard; the output shader applies sqrt() for display gamma
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

  const outRT = new THREE.WebGLRenderTarget(W, H, {
    minFilter: THREE.NearestFilter,
    magFilter: THREE.NearestFilter,
    format: THREE.RGBAFormat,
    type: THREE.UnsignedByteType,
    depthBuffer: false,
    stencilBuffer: false,
  })
  const pu = {
    tImg: { value: outRT.texture },
    tBlue: { value: blue },
    uPixel: { value: true },
    uDevP: { value: 1 },
    uGridBottom: { value: 1 },
    uMatrix: { value: 0 },
    uShared: { value: true },
    uStepL: { value: 0.07 },
    uStepC: { value: 0.04 },
    uStepH: { value: 30 },
    uAnchorL: { value: 0.93 },
    uCut: { value: 0.02 },
    uDither: { value: 1 },
    uHueOffset: { value: 0 },
    tWall: { value: null as THREE.DataTexture | null },
    uWallRow0: { value: 0 },
    uRenderTop: { value: 0 },
    uRenderHeight: { value: 1 },
    uFade: { value: new THREE.Vector3(ROOM_FADE.L, ROOM_FADE.C, ROOM_FADE.h) },
  }
  const pixScene = new THREE.Scene()
  pixScene.add(
    quadMesh(
      new THREE.ShaderMaterial({
        uniforms: pu,
        vertexShader: vs,
        fragmentShader: fadeIntoWallpaper(onWindowGrid(fsPix)),
        depthTest: false,
        depthWrite: false,
        toneMapped: false,
      }),
    ),
  )
  let dirty = true,
    anchorPending = true

  // ── Settings: applyAll(P0), applyPx(PX0), applyBg(BACKDROP) ──────────
  // exposure: per theme, see BACKDROP
  u.uSky.value = P0.sky
  u.uWindow.value = P0.window
  u.uUplight.value = P0.uplight
  u.uCandles.value = P0.candles
  u.uWarmth.value = P0.warmth
  const MAX_SAMPLES = PX0.samples
  pu.uStepL.value = PX0.stepL
  pu.uStepC.value = PX0.stepC
  pu.uStepH.value = PX0.stepH
  pu.uCut.value = PX0.cut
  pu.uHueOffset.value = PX0.hue
  pu.uDither.value = PX0.dither
  pu.uMatrix.value = PX0.matrix
  pu.uShared.value = PX0.shared
  pu.uPixel.value = true
  /** The site theme's backdrop, on the wallpaper's colours. */
  function applyBackdrop() {
    const backdrop = BACKDROP[siteTheme()]
    ou.uExposure.value = backdrop.exposure
    const colours = backdropColours(container)
    u.uCellI.value = backdrop.cellI
    u.uCellHues.value = backdrop.cellHues
    u.uCellSize.value = backdrop.cellSize
    u.uWash.value = backdrop.wash
    u.uSeed.value = backdrop.seed
    u.uRingL.value = colours.ringL
    u.uRingC.value = colours.ringC
    const lab = linToLab(srgbToLin(colours.background))
    u.uBgLab.value.set(lab[0], lab[1], lab[2])
    return colours
  }
  let colours = applyBackdrop()

  /**
   * The wallpaper's own pixels under the fade (see fadeIntoWallpaper()):
   * computed by the wallpaper module with the mounted wallpaper's options and
   * colours. Its pixels are counted from the window's top-left whatever area
   * it is asked for, so it is asked for the window from the top down to the
   * fade's last row, and the rows the render covers go up.
   */
  function uploadWallpaper(
    dpr: number,
    width: number,
    renderTop: number,
    renderHeight: number,
  ) {
    const devP = pu.uDevP.value
    const fadeEnd =
      renderTop + renderHeight * Math.max(ROOM_FADE.L, ROOM_FADE.C, ROOM_FADE.h)
    const row0 = Math.floor(renderTop / devP)
    const rows = Math.max(1, Math.ceil(fadeEnd / devP) - row0 + 1)
    const wallpaper = computeWallpaper({
      ...WALLPAPER_OPTIONS,
      ...colours,
      width: width / dpr,
      height: ((row0 + rows) * devP) / dpr,
      dpr,
      deviceWidth: width,
      deviceHeight: (row0 + rows) * devP,
    })
    const cells = wallpaper.cols * 4
    const texture = new THREE.DataTexture(
      new Uint8Array(wallpaper.data.buffer, row0 * cells, rows * cells),
      wallpaper.cols,
      rows,
      THREE.RGBAFormat,
      THREE.UnsignedByteType,
    )
    texture.minFilter = texture.magFilter = THREE.NearestFilter
    texture.generateMipmaps = false
    texture.needsUpdate = true
    pu.tWall.value?.dispose()
    pu.tWall.value = texture
    pu.uWallRow0.value = row0
    pu.uRenderTop.value = renderTop
    pu.uRenderHeight.value = renderHeight
  }

  function placeCamera() {
    const cp = Math.cos(orbit.pitch)
    camera.position.set(
      target.x + Math.sin(orbit.yaw) * cp * orbit.dist,
      target.y + Math.sin(orbit.pitch) * orbit.dist,
      target.z + Math.cos(orbit.yaw) * cp * orbit.dist,
    )
    camera.lookAt(target)
    camera.updateMatrixWorld(true)
    u.uCameraMatrix.value.copy(camera.matrixWorld)
  }

  // ── Resize: every dither pixel is a whole number of device pixels ──────
  // Exact device-pixel sizes of the canvas and of the window-sized container,
  // where the browser reports them.
  const devSizes = new Map<Element, [number, number]>()
  function deviceSize(element: HTMLElement, dpr: number) {
    return (
      devSizes.get(element) ?? [
        Math.round(element.clientWidth * dpr),
        Math.round(element.clientHeight * dpr),
      ]
    )
  }
  function resize() {
    const dpr = devicePixelRatio || 1
    const bottom = deviceSize(container, dpr)[1]
    const devP = wallpaperPixel(dpr)
    const [dw, dh] = deviceSize(canvas, dpr)
    pu.uDevP.value = devP
    pu.uGridBottom.value = bottom
    // the window-grid rows the canvas touches, the first and last perhaps in part
    W = Math.max(2, Math.ceil(dw / devP))
    H = Math.max(
      2,
      Math.floor((bottom - 1) / devP) - Math.floor((bottom - dh) / devP) + 1,
    )
    uploadWallpaper(dpr, dw, bottom - dh, dh)
    renderer.setSize(dw, dh, false)
    ptRT.setSize(W, H)
    copyRT.setSize(W, H)
    outRT.setSize(W, H)
    anchorPending = true
    u.uResolution.value.set(W, H)
    const aspect = W / H
    // keep the photo's framing: 40 degrees vertical at 3:2, wider on narrow screens
    // (The site's box runs from about 0.9:1 to 12:1: wide boxes see further
    // along the back wall, tall ones more floor and wall above the window;
    // the scene patch extends both, see scripts/whiteroom-scene.patch.)
    const hHalf = Math.atan(1.5 * vlen)
    let v = aspect >= 1.5 ? vlen : Math.tan(hHalf) / aspect
    v = Math.min(v, Math.tan(THREE.MathUtils.degToRad(37)))
    u.uVLen.value = v
    u.uULen.value = v * aspect
    camera.fov = THREE.MathUtils.radToDeg(2 * Math.atan(v))
    camera.aspect = aspect
    camera.updateProjectionMatrix()
    reset()
  }

  let samples = 0,
    frame = 1,
    resetPending = true
  const t0 = performance.now()
  function reset() {
    resetPending = true
    anchorPending = true
    start()
  }

  function step() {
    const isMoving = resetPending
    resetPending = false
    if (!isMoving) {
      samples += 1
      frame += 1
    } else {
      u.uPreviousSampleCount.value = samples || 1
      frame = 1
      samples = 1
    }
    u.uTime.value = ((performance.now() - t0) / 1000) % 1000
    u.uCameraIsMoving.value = isMoving
    u.uSampleCounter.value = samples
    u.uFrameCounter.value = frame
    u.uRandomVec2.value.set(Math.random(), Math.random())
    ou.uCameraIsMoving.value = isMoving
    ou.uSampleCounter.value = samples
    ou.uOneOverSampleCounter.value = 1 / samples
    renderer.setRenderTarget(ptRT)
    renderer.render(ptScene, camera)
    renderer.setRenderTarget(copyRT)
    renderer.render(copyScene, ortho)
    dirty = true
  }
  let wallBuf: Uint8Array | null = null
  function measureWall() {
    // average a patch of back wall (upper left of the frame) and use its OKLab lightness as the grid anchor
    // The page's frame is always 3:2; this box can be any shape, its view
    // that 3:2 view grown on one axis (see resize()). Measure the same patch
    // of wall: the page's, at the place the 3:2 view takes in this frame.
    // Off to the side in a wide frame the wall is darker, and so would be the
    // whole palette.
    const sx = (1.5 * vlen) / u.uULen.value,
      sy = vlen / u.uVLen.value,
      x0 = Math.max(0, Math.floor(W * (0.5 - 0.45 * sx))),
      y0 = Math.floor(H * (0.5 + 0.08 * sy)),
      w = Math.max(2, Math.floor(W * 0.08 * sx)),
      h = Math.max(2, Math.floor(H * 0.08 * sy))
    if (!wallBuf || wallBuf.length !== w * h * 4)
      wallBuf = new Uint8Array(w * h * 4)
    renderer.readRenderTargetPixels(outRT, x0, y0, w, h, wallBuf)
    let r = 0,
      g = 0,
      b = 0
    const n = w * h
    for (let i = 0; i < n; i++) {
      r += wallBuf[i * 4]
      g += wallBuf[i * 4 + 1]
      b += wallBuf[i * 4 + 2]
    }
    const lin = [r, g, b].map((v) => {
      v = v / n / 255
      return v * v
    }) as RGB
    return linToLab(lin)[0]
  }
  function present() {
    renderer.setRenderTarget(outRT)
    renderer.render(outScene, ortho)
    if (anchorPending && PX0.anchor && samples >= Math.min(MAX_SAMPLES, 16)) {
      pu.uAnchorL.value = Math.min(1, Math.max(0.05, measureWall()))
      anchorPending = false
    }
    renderer.setRenderTarget(null)
    renderer.render(pixScene, ortho)
    dirty = false
  }

  // ── Loop ───────────────────────────────────────────────────────────────
  let raf = 0
  function loop() {
    raf = 0
    if (samples < MAX_SAMPLES || resetPending) step()
    if (dirty) present()
    // The page idles on requestAnimationFrame from here; a background stops.
    if (samples < MAX_SAMPLES || resetPending || anchorPending) start()
    else container.dataset.converged = String(samples)
  }
  function start() {
    delete container.dataset.converged
    if (!raf && compiled) raf = requestAnimationFrame(loop)
  }

  // ── Compile before the canvas joins the page ───────────────────────────
  // three.js picks a shader's variant from the render target current when
  // it compiles, so each pass compiles against the target it draws into.
  let compiled = false
  placeCamera()
  renderer.setRenderTarget(ptRT)
  const compiling = [renderer.compileAsync(ptScene, camera)]
  renderer.setRenderTarget(copyRT)
  compiling.push(renderer.compileAsync(copyScene, ortho))
  renderer.setRenderTarget(outRT)
  compiling.push(renderer.compileAsync(outScene, ortho))
  renderer.setRenderTarget(null)
  compiling.push(renderer.compileAsync(pixScene, ortho))

  const destroy = () => {
    cancelAnimationFrame(raf)
    observer?.disconnect()
    themeObserver?.disconnect()
    pu.tWall.value?.dispose()
    canvas.removeEventListener('webglcontextlost', lost)
    for (const disposable of [
      ptRT,
      copyRT,
      outRT,
      tri,
      primTex,
      blue,
      ...materials,
      renderer,
    ])
      disposable.dispose()
    canvas.remove()
  }
  const lost = (event: Event) => {
    event.preventDefault()
    cancelAnimationFrame(raf)
    onLost?.()
  }
  canvas.addEventListener('webglcontextlost', lost)
  let observer: ResizeObserver | undefined,
    themeObserver: MutationObserver | undefined

  try {
    await Promise.all(compiling)
    if (shaderError) throw new Error(`shader failed to compile: ${shaderError}`)
    signal.throwIfAborted()
  } catch (error) {
    destroy()
    throw error
  }
  compiled = true
  container.append(canvas)

  // exact device-pixel sizes, so every dither pixel is a whole number of device pixels on the window's grid
  observer = new ResizeObserver((entries) => {
    for (const entry of entries) {
      const box = entry.devicePixelContentBoxSize?.[0]
      if (box) devSizes.set(entry.target, [box.inlineSize, box.blockSize])
      else devSizes.delete(entry.target)
    }
    resize()
  })
  for (const element of [canvas, container]) {
    try {
      observer.observe(element, { box: 'device-pixel-content-box' })
    } catch {
      // Safari: fall back to CSS size x devicePixelRatio
      observer.observe(element)
    }
  }
  resize()

  // A theme switch (next-themes sets the class on <html>) changes the page
  // colour: the room's backdrop and the wallpaper it fades into. resize()
  // uploads the wallpaper again and starts the image over, wall anchor too.
  let theme = `${siteTheme()} ${JSON.stringify(colours)}`
  themeObserver = new MutationObserver(() => {
    colours = applyBackdrop()
    const next = `${siteTheme()} ${JSON.stringify(colours)}`
    if (next === theme) return
    theme = next
    resize()
  })
  themeObserver.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ['class', 'style', 'data-theme'],
  })

  return { destroy }
}
