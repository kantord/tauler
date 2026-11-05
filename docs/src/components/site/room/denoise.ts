/**
 * The room's denoise: an edge-aware spatial filter between the accumulated
 * image and the tone-mapping pass, so what the wallpaper quantizes is smooth.
 *
 * At 48 samples the room's grain is path-tracing variance, and the one
 * quantize makes it worse: lightness jitter crosses palette thresholds and
 * draws specks of the wrong hue. This is the spatial half of SVGF (Schied et
 * al. 2017), on the image the renderer already accumulates; the temporal
 * half, reprojection, has nothing to do with a camera that never moves.
 *
 * - The room shader also writes a G-buffer from each sample's first hit,
 *   accumulated like the colour (two more attachments of its target, so the
 *   copy pass, COPY_G, copies three): the normal and hit distance, and the
 *   albedo and the second moment of the sample's luminance over that albedo
 *   (gBufferMain(), withGBuffer()). Accumulated rather than taken from one
 *   sample, so a cell a chair leg half covers says so in every channel.
 * - DEMOD divides the mean colour by the mean albedo (albedo demodulation:
 *   irradiance, what the filter may smooth, texture taken out) and estimates
 *   each cell's variance of the mean from the moments, prefiltered 3 × 3.
 * - ATROUS runs four à-trous passes (5 × 5 B3 spline at steps 1, 2, 4, 8),
 *   each tap weighted by how alike the two cells' normals, hit distances
 *   (against the local depth gradient), albedos and irradiance luminances
 *   are (that last against the cell's noise, so a real shadow edge stops it
 *   and grain does not), then the last pass multiplies the albedo back in.
 *   Teak grain, the rug, the print, leaves and chair legs come back from the
 *   albedo; only the light on them is smoothed.
 *
 * It filters linear irradiance, not OKLab: demodulation is a product in
 * linear light (colour = albedo × irradiance), and a weighted mean of
 * nonlinear values is biased (darkens), most where the noise is worst. The
 * perceptual part, how big a difference is allowed, comes from the variance
 * term instead. The normal test's exponent is 32 rather than SVGF's 128, for
 * the 3 cm cove where the walls meet the floor (SIGMA).
 *
 * The result replaces the accumulated image as the tone-mapping pass's
 * input, with every cell marked an edge so that pass's own blur leaves it
 * alone. It runs after every sample, about 0.45 ms at 1440 × 900, until the
 * loop stops at 48, so the room fades in already smooth and there is no pop
 * when it converges; after that it costs nothing.
 */
import * as THREE from 'three'

/** Luminance weights, shared by every pass. */
const LUM = 'const vec3 LUM = vec3(0.2126, 0.7152, 0.0722);'

/**
 * The renderer's pathtracing_main chunk with the G-buffer's outputs and
 * accumulation, for the chunk room.ts installs (only the room shader
 * includes it). It reads `objectNormal`, `objectColor` and `gFirstT`, which
 * withGBuffer() gives the room shader. `edit` is room.ts's editShader
 * (one anchor each, throws if missing).
 */
export function gBufferMain(
  main: string,
  edit: (shader: string, edits: [string, string][]) => string,
) {
  return edit(main, [
    [
      'void main( void )',
      `uniform sampler2D tPreviousG0, tPreviousG1;
layout(location = 1) out highp vec4 gOut0;   // [tauler] denoise: first hit's normal, distance
layout(location = 2) out highp vec4 gOut1;   // [tauler] denoise: first hit's albedo, irradiance luminance squared
${LUM}
void main( void )`,
    ],
    [
      'pc_fragColor = vec4(previousPixel.rgb + currentPixel.rgb, currentPixel.a);',
      `pc_fragColor = vec4(previousPixel.rgb + currentPixel.rgb, currentPixel.a);
	// [tauler] denoise: the G-buffer accumulates like the colour, from nothing on a fresh start
	bool fresh = uFrameCounter == 1.0;
	vec4 g0 = fresh ? vec4(0) : texelFetch(tPreviousG0, ivec2(gl_FragCoord.xy), 0);
	vec4 g1 = fresh ? vec4(0) : texelFetch(tPreviousG1, ivec2(gl_FragCoord.xy), 0);
	vec3 alb = objectColor;
	float l = dot(currentPixel.rgb / (dot(alb, LUM) > 0.0 ? max(alb, vec3(1e-3)) : vec3(1)), LUM);
	gOut0 = g0 + vec4(objectNormal, gFirstT);
	gOut1 = g1 + vec4(alb, l * l);`,
    ],
  ])
}

/**
 * The room shader recording its first hit's distance, for gBufferMain():
 * after the cove and plant edits (withPlant()), whose path loop it edits.
 */
export function withGBuffer(
  fsRoom: string,
  edit: (shader: string, edits: [string, string][]) => string,
) {
  return edit(fsRoom, [
    [
      'vec3 hitNormal, hitLocal;',
      'vec3 hitNormal, hitLocal;\nfloat gFirstT = 0.0;   // [tauler] denoise',
    ],
    [
      'objectNormal += nl; objectColor += albedo; objectID = float(hitPrim);',
      'objectNormal += nl; objectColor += albedo; objectID = float(hitPrim); gFirstT = t;',
    ],
  ])
}

/** The copy pass, in place of the page's: all three attachments of the accumulated target. */
export const COPY_G = `precision highp float;
precision highp sampler2D;
uniform sampler2D tPathTracedImageTexture, tG0, tG1;
layout(location = 1) out highp vec4 gOut0;
layout(location = 2) out highp vec4 gOut1;
void main()
{
	ivec2 p = ivec2(gl_FragCoord.xy);
	pc_fragColor = texelFetch(tPathTracedImageTexture, p, 0);
	gOut0 = texelFetch(tG0, p, 0);
	gOut1 = texelFetch(tG1, p, 0);
}
`

/** What DEMOD and ATROUS share: reading the accumulated G-buffer. */
const COMMON = `precision highp float;
precision highp int;
precision highp sampler2D;
${LUM}
uniform sampler2D tG0, tG1;
uniform float uInvN;   // 1 / samples
// mean albedo; 1 where nothing was hit (no texture to take out)
vec3 albedoAt(ivec2 p) {
	vec3 a = texelFetch(tG1, p, 0).rgb * uInvN;
	return dot(a, LUM) > 0.0 ? max(a, vec3(1e-3)) : vec3(1);
}
`

/** Irradiance (rgb) and its variance of the mean (a), prefiltered 3 × 3. */
const DEMOD = `${COMMON}
uniform sampler2D tColor;
vec3 irradianceAt(ivec2 p) { return texelFetch(tColor, p, 0).rgb * uInvN / albedoAt(p); }
void main()
{
	ivec2 p = ivec2(gl_FragCoord.xy), size = textureSize(tColor, 0);
	float v = 0.0, wsum = 0.0;
	for (int y = -1; y <= 1; y++)
	for (int x = -1; x <= 1; x++) {
		ivec2 q = clamp(p + ivec2(x, y), ivec2(0), size - 1);
		float l = dot(irradianceAt(q), LUM);
		float m2 = texelFetch(tG1, q, 0).a * uInvN;
		float w = (x == 0 ? 2.0 : 1.0) * (y == 0 ? 2.0 : 1.0);
		v += w * max(0.0, m2 - l * l) * uInvN;
		wsum += w;
	}
	pc_fragColor = vec4(irradianceAt(p), v / wsum);
}
`

/** One à-trous pass; the last (uFinal) remodulates into an accumulated image. */
const ATROUS = `${COMMON}
uniform sampler2D tIrr;
uniform int uStep;
uniform bool uFinal;
uniform float uSigmaL, uSigmaN, uSigmaZ, uSigmaA;
vec3 normalAt(ivec2 p) { vec3 n = texelFetch(tG0, p, 0).xyz; return dot(n, n) > 1e-6 ? normalize(n) : vec3(0); }
float depthAt(ivec2 p) { return texelFetch(tG0, p, 0).w * uInvN; }
void main()
{
	ivec2 p = ivec2(gl_FragCoord.xy), size = textureSize(tIrr, 0), hi = size - 1;
	vec4 c = texelFetch(tIrr, p, 0);
	vec3 n = normalAt(p), a = albedoAt(p);
	float z = depthAt(p), l = dot(c.rgb, LUM);
	// the hit distance's gradient, one-sided where that is smaller, so a silhouette does not inflate it
	float zx0 = depthAt(clamp(p - ivec2(1, 0), ivec2(0), hi)), zx1 = depthAt(clamp(p + ivec2(1, 0), ivec2(0), hi));
	float zy0 = depthAt(clamp(p - ivec2(0, 1), ivec2(0), hi)), zy1 = depthAt(clamp(p + ivec2(0, 1), ivec2(0), hi));
	vec2 dz = vec2(min(abs(z - zx0), abs(zx1 - z)), min(abs(z - zy0), abs(zy1 - z)));
	// the noise the luminance test allows: the variance, blurred 3 x 3
	float v = 0.0;
	for (int y = -1; y <= 1; y++)
	for (int x = -1; x <= 1; x++)
		v += (x == 0 ? 0.5 : 0.25) * (y == 0 ? 0.5 : 0.25) * texelFetch(tIrr, clamp(p + ivec2(x, y), ivec2(0), hi), 0).a;
	float lDen = uSigmaL * sqrt(v) + 1e-5;
	const float K[3] = float[3](0.375, 0.25, 0.0625);
	vec3 sum = vec3(0); float wsum = 0.0, vsum = 0.0;
	for (int y = -2; y <= 2; y++)
	for (int x = -2; x <= 2; x++) {
		ivec2 o = ivec2(x, y) * uStep, q = p + o;
		if (any(lessThan(q, ivec2(0))) || any(greaterThan(q, hi))) continue;
		vec4 cq = texelFetch(tIrr, q, 0);
		float w = K[abs(x)] * K[abs(y)];
		if (x != 0 || y != 0) {
			w *= pow(max(0.0, dot(n, normalAt(q))), uSigmaN);
			w *= exp(-abs(z - depthAt(q)) / (uSigmaZ * dot(dz, abs(vec2(o))) + 1e-3 * z + 1e-3));
			w *= exp(-length(a - albedoAt(q)) / uSigmaA);
			w *= exp(-abs(l - dot(cq.rgb, LUM)) / lDen);
		}
		sum += w * cq.rgb;
		vsum += w * w * cq.a;
		wsum += w;
	}
	vec3 irr = sum / wsum;
	pc_fragColor = uFinal ? vec4(irr * a / uInvN, 1.0) : vec4(irr, vsum / (wsum * wsum));
}
`

/** The à-trous steps, in cells. */
const STEPS = [1, 2, 4, 8]

/**
 * Edge-stopping strengths. Luminance: differences up to about this many
 * standard deviations of the cell's noise blend (SVGF's 4). Normal: the
 * exponent on their dot product: 32, not SVGF's 128, which left the 3 cm
 * coves where walls meet the floor (a curve a few cells across) unfiltered,
 * a dotted bright line; chair legs and leaves held either way. Depth: hit-distance
 * differences this many times what the local gradient predicts. Albedo: a
 * distance in linear RGB.
 */
const SIGMA = { L: 4, N: 32, Z: 1, A: 0.1 }

export interface GBufferSource {
  /** The accumulated target's three attachments: colour, normal + distance, albedo + moment. */
  textures: THREE.Texture[]
}

export interface Denoiser {
  /** The compile jobs, each against the target it draws into. */
  compile(): Promise<THREE.Object3D>[]
  setSize(width: number, height: number): void
  /** Where run() writes: an accumulated image (colour × samples, alpha 1). */
  readonly output: THREE.Texture
  /** Filters the accumulated image of `samples` samples into `output`. */
  run(samples: number): void
  dispose(): void
}

export function createDenoiser(
  renderer: THREE.WebGLRenderer,
  source: GBufferSource,
  vs: string,
): Denoiser {
  const opts: THREE.RenderTargetOptions = {
    minFilter: THREE.NearestFilter,
    magFilter: THREE.NearestFilter,
    format: THREE.RGBAFormat,
    type: THREE.FloatType,
    depthBuffer: false,
    stencilBuffer: false,
  }
  const ping = [
    new THREE.WebGLRenderTarget(2, 2, opts),
    new THREE.WebGLRenderTarget(2, 2, opts),
  ]
  const [color, g0, g1] = source.textures
  const shared = { tG0: { value: g0 }, tG1: { value: g1 }, uInvN: { value: 1 } }
  const demodU = { ...shared, tColor: { value: color } }
  const atrousU = {
    ...shared,
    tIrr: { value: ping[0].texture },
    uStep: { value: 1 },
    uFinal: { value: false },
    uSigmaL: { value: SIGMA.L },
    uSigmaN: { value: SIGMA.N },
    uSigmaZ: { value: SIGMA.Z },
    uSigmaA: { value: SIGMA.A },
  }
  const tri = new THREE.BufferGeometry()
  tri.setAttribute(
    'position',
    new THREE.Float32BufferAttribute([-1, -1, 0, 3, -1, 0, -1, 3, 0], 3),
  )
  const scene = (
    fragmentShader: string,
    uniforms: Record<string, THREE.IUniform>,
  ) => {
    const material = new THREE.ShaderMaterial({
      uniforms,
      vertexShader: vs,
      fragmentShader,
      depthTest: false,
      depthWrite: false,
    })
    const mesh = new THREE.Mesh(tri, material)
    mesh.frustumCulled = false
    const s = new THREE.Scene()
    s.add(mesh)
    return { s, material }
  }
  const demod = scene(DEMOD, demodU)
  const atrous = scene(ATROUS, atrousU)
  const ortho = new THREE.OrthographicCamera(-1, 1, 1, -1, 0, 1)

  // the passes alternate targets, so the last writes into this one
  const last = ping[STEPS.length % 2]

  return {
    output: last.texture,
    compile() {
      renderer.setRenderTarget(ping[0])
      const jobs = [
        renderer.compileAsync(demod.s, ortho),
        renderer.compileAsync(atrous.s, ortho),
      ]
      renderer.setRenderTarget(null)
      return jobs
    },
    setSize(width, height) {
      for (const rt of ping) rt.setSize(width, height)
    },
    run(samples) {
      shared.uInvN.value = 1 / samples
      renderer.setRenderTarget(ping[0])
      renderer.render(demod.s, ortho)
      let from = 0
      STEPS.forEach((step, i) => {
        atrousU.tIrr.value = ping[from].texture
        atrousU.uStep.value = step
        atrousU.uFinal.value = i === STEPS.length - 1
        renderer.setRenderTarget(ping[1 - from])
        renderer.render(atrous.s, ortho)
        from = 1 - from
      })
      renderer.setRenderTarget(null)
    },
    dispose() {
      for (const d of [...ping, tri, demod.material, atrous.material])
        d.dispose()
    },
  }
}
