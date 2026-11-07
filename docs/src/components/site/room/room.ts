/**
 * The home background: a room inspired by a photo by Urban Vintage on Unsplash,
 * path-traced live by Erich Loftis's THREE.js PathTracing Renderer (CC0,
 * https://github.com/erichlof/THREE.js-PathTracing-Renderer), as a layer of
 * the wallpaper: the wallpaper's shader (wallpaper-gl.ts) composites the
 * room's continuous colour with its own and turns the two into pixel art in
 * one quantize, on one grid, with one palette.
 *
 * It began as a standalone page ("the page" below), whose path-tracing and
 * output shaders (public/room/shaders/) and scene description
 * (room-scene.js) run here on the renderer's GLSL chunks and blue noise
 * (vendor/, the only third-party code) and three.js r185.
 *
 * The page's main script is ported here, with its defaults: the
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
 * from the wallpaper as its samples gather (presence() and ROOM_EASE_IN, in
 * room-size.ts), each new image starts from nothing rather than from half
 * the old one (freshStart()), and reflected and indirect light is clamped
 * against fireflies (clampFireflies(); both in room-shader.ts).
 *
 * Changed to follow the site's theme: the walls and floor start from the
 * page colour of the theme that is on, with the page's "Tauler light" or
 * "Tauler dark" backdrop preset, while the light stays the same (see
 * BACKDROP, SKY in room-light.ts); a theme switch starts the image over.
 *
 * Changed to frame any box: see frame().
 *
 * Added: a Monstera in an ochre bowl at the left of the room, the scene's
 * 8th group (monstera-scene.js, added in room-scene.js), which wide frames
 * show, and a small cove where the walls meet the floor (room-scene.js).
 * The room shader learns their primitives
 * and materials by load-time edits (COVE_EDITS and withPlant() in
 * room-shader.ts, monstera-shader.ts), and the leaves' maps are baked once, before the first sample
 * (bakeLeaves()).
 *
 * Changed: the materials (materials.ts). Every material is physically based
 * and procedural: GGX microfacets sampled from their visible normals for
 * metals, dielectric coats and the teak's lacquer, Fresnel's odds between the
 * specular and diffuse lobes, measured albedos, and non-periodic hash noise in
 * object space, seeded per object, for colour, roughness and shading normals.
 * Applied after the plant's edits, before the firefly clamp and the G-buffer.
 *
 * Added: a denoise between accumulation and tone-mapping (denoise.ts), so
 * the 48 samples' grain does not quantize into specks of the wrong hue. It
 * is the spatial half of SVGF: the room shader also writes a G-buffer from
 * its first hits (normal, hit distance, albedo, luminance moment),
 * accumulated like the colour in two more attachments of its target (the
 * copy pass is the denoise's, COPY_G, which copies all three; the page's is
 * not used); the colour is divided by the albedo, and the linear irradiance
 * left is smoothed by four edge-aware à-trous passes (steps 1, 2, 4, 8)
 * before the albedo goes back in. Linear, not OKLab, because a mean of
 * nonlinear values is biased; the normal test's exponent is 32 so the 3 cm
 * cove is filtered too. It runs after every sample until the loop stops at
 * 48, and the tone-mapping pass reads its result (denoiser.output) in place
 * of the accumulated image.
 *
 * Where the code is: this module is the layer (createRoom(): the scene's
 * data, render targets, passes, framing, layout and the sampling loop) and
 * the check for whether it can run (roomSupported()). The shaders and every
 * load-time edit to them are room-shader.ts (roomShaders()); the lights and
 * per-theme backdrop are room-light.ts; where the room sits and how it fades
 * in, room-size.ts. Tests change its behaviour only through test-hooks.ts.
 */
import * as THREE from 'three'
import { buildRoomScene } from './room-scene.js'
import { rgbToOklab } from '@/lib/wallpaper/tauler-wallpaper.js'
import { ditherPixel, type WallpaperColours } from '../wallpaper/options'
import {
  isSoftware,
  type Fade,
  type RoomCells,
} from '../wallpaper/wallpaper-gl'
import {
  ROOM_DETAIL,
  ROOM_FADE,
  ROOM_MATCH,
  ROOM_WAVE,
  presence,
} from './room-size'
import { LEAF_MAPS, LEAF_RES } from './monstera-shader'
import { COPY_G, createDenoiser } from './denoise'
import { ASSETS, roomShaders } from './room-shader'
import { BACKDROP, FIREFLY_MAX, P0, SKY, siteTheme } from './room-light'
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

/** The page's sample count. */
const MAX_SAMPLES = 48

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
  const [{ vs, fsRoom, outFrag }, blue] = await Promise.all([
    roomShaders(),
    new THREE.TextureLoader().loadAsync(`${ASSETS}/BlueNoise_R_128.png`),
  ])
  signal.throwIfAborted()
  blue.wrapS = blue.wrapT = THREE.RepeatWrapping
  blue.flipY = false
  blue.minFilter = blue.magFilter = THREE.NearestFilter
  blue.generateMipmaps = false

  const overrides = testHooks().roomFade ?? {}
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
  // two more attachments carry the denoise's G-buffer (denoise.ts)
  const accOpts = { ...rtOpts, count: 3 }
  const ptRT = new THREE.WebGLRenderTarget(W, H, accOpts),
    copyRT = new THREE.WebGLRenderTarget(W, H, accOpts)

  const camera = new THREE.PerspectiveCamera(40, 1.5, 1, 5000)
  const target = new THREE.Vector3(5, 118, 0)
  const orbit = { yaw: 0, pitch: Math.atan2(2, 630), dist: Math.hypot(630, 2) }

  const vlen = Math.tan(THREE.MathUtils.degToRad(20))
  const u = {
    tPreviousTexture: { value: copyRT.texture },
    tPreviousG0: { value: copyRT.textures[1] },
    tPreviousG1: { value: copyRT.textures[2] },
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
        fragmentShader: fsRoom,
        depthTest: false,
        depthWrite: false,
      }),
    ),
  )
  copyScene.add(
    quadMesh(
      new THREE.ShaderMaterial({
        uniforms: {
          tPathTracedImageTexture: { value: ptRT.texture },
          tG0: { value: ptRT.textures[1] },
          tG1: { value: ptRT.textures[2] },
        },
        vertexShader: vs,
        fragmentShader: COPY_G,
        depthTest: false,
        depthWrite: false,
      }),
    ),
  )
  // the denoise's result stands in for the accumulated image
  const denoiser = createDenoiser(renderer, ptRT, vs)
  const ou = {
    tPathTracedImageTexture: { value: denoiser.output },
    uSampleCounter: { value: 0 },
    uOneOverSampleCounter: { value: 0 },
    uPixelEdgeSharpness: { value: 0.75 },
    uEdgeSharpenSpeed: { value: 0.05 },
    uCameraIsMoving: { value: false },
    uSceneIsDynamic: { value: false },
    uUseToneMapping: { value: true },
    uExposure: { value: 1 },
  }
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
    denoiser.setSize(W, H)
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
    testHooks().roomSamples ?? MAX_SAMPLES,
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
    denoiser.run(samples)
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
  compiling.push(...denoiser.compile())

  const destroy = () => {
    cancelAnimationFrame(raf)
    raf = 0
    compiled = false
    cells = null
    renderer.resetState()
    denoiser.dispose()
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
    // compileAsync() only waits for the driver; three.js checks a program's
    // link status on its first use, normally the first render. Use each
    // program now (no draw, just its uniform lookup) so that a shader that
    // failed to compile rejects here, and the wallpaper draws alone.
    for (const program of renderer.info.programs ?? []) program.getUniforms()
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
