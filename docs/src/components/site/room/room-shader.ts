/**
 * The room's shaders: the page's three (public/room/shaders/), fetched as
 * soon as this module loads, and every load-time edit the site makes to them
 * and to the renderer's GLSL chunks (room.ts's module doc lists the changes
 * and why). Each edit goes in at an anchor that must appear exactly once
 * (editShader(), replaceSpan()), so an upstream change fails loudly. The
 * edits of the other room modules (monstera-shader.ts, materials.ts,
 * denoise.ts) are applied here too, in their order.
 *
 * The interface is roomShaders(): the finished sources, ready to compile.
 */
import * as THREE from 'three'
import { installPathTracingChunks } from './vendor/PathTracingCommon.js'
import { MONSTERA_EDITS, ONE_TRACE } from './monstera-shader'
import { gBufferMain, withGBuffer } from './denoise'
import { withMaterials } from './materials'
import { testHooks } from '../test-hooks'

/** Where the room's assets are served (public/room/). */
export const ASSETS = '/room'

let chunksInstalled = false

async function loadShader(name: string) {
  const response = await fetch(`${ASSETS}/shaders/${name}.glsl`)
  if (!response.ok) throw new Error(`${name}: HTTP ${response.status}`)
  return response.text()
}

/** The page's shaders, fetched as soon as this module loads (Wallpaper.tsx loads room.ts early). */
const shaders = Promise.all(
  (['vs', 'fs-out', 'fs-room'] as const).map(loadShader),
)
shaders.catch(() => {}) // reported by createRoom()

/** Applies each edit to its single anchor; throws if the anchor is missing or repeated. */
function editShader(shader: string, edits: [string, string][]) {
  for (const [from, to] of edits) {
    if (shader.split(from).length !== 2)
      throw new Error(`shader edit: no single "${from}"`)
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
      throw new Error(`shader edit: no single "${anchor}"`)
  return (
    shader.slice(0, shader.indexOf(start)) +
    text +
    shader.slice(shader.indexOf(end))
  )
}

/**
 * The cove where the walls meet the floor and each other (KIND.COVE, from
 * room-scene.js): the inside of a cylinder, between its axis
 * and the corner, which meets both planes tangentially. Its branch goes in
 * before the Monstera's, which takes every kind from K_LEAF up. The walls'
 * paint takes its colour from the wall's coordinates on the cove as well, so
 * the backdrop's cells do not change colour half way round it.
 */
const COVE_EDITS: [string, string][] = [
  [
    '/* ---------------- scene ---------------- */',
    `// COVE: a concave fillet where two planes meet: the inside of the cylinder of radius r around
// the axis a -> b, only between the axis and the corner (the planes lie along e1 and e2 from
// the axis). The far root is the inside; the normal faces the axis.
#define K_COVE 8
float CoveIntersect(vec3 a, vec3 b, float r, vec3 e1, vec3 e2, vec3 ro, vec3 rd, out vec3 n)
{
	vec3 w = b - a; float h = length(w); w /= h;
	vec3 o = ro - a;
	float od = dot(o, w), dw = dot(rd, w);
	vec3 op = o - w * od, dp = rd - w * dw;
	float A = dot(dp, dp), B = dot(op, dp), disc = B * B - A * (dot(op, op) - r * r);
	if (disc < 0.0 || A < 1e-9) return INFINITY;
	float t = (-B + sqrt(disc)) / A, y = od + t * dw;
	vec3 q = op + t * dp;
	if (t <= 0.004 || y < 0.0 || y > h || dot(q, e1) < 0.0 || dot(q, e2) < 0.0) return INFINITY;
	n = -q;
	return t;
}
` + '\n/* ---------------- scene ---------------- */',
  ],
  [
    '\t\t\telse { d = SphereIntersect(a1.w, a1.xyz, lro, lrd); n = (lro + lrd * d) - a1.xyz; }',
    `			else if (k == K_COVE) {   // COVE
				vec3 a3 = texelFetch(tPrims, ivec2(3, i), 0).xyz;
				vec3 a4 = texelFetch(tPrims, ivec2(4, i), 0).xyz;
				d = CoveIntersect(a1.xyz, a2.xyz, a1.w, a3, a4, lro, lrd, n);
			}
` +
      '\t\t\telse { d = SphereIntersect(a1.w, a1.xyz, lro, lrd); n = (lro + lrd * d) - a1.xyz; }',
  ],
  [
    '\tif (id == M_WALL) { albedo = backdropAlbedo(backdropCoord(p, n))',
    '\tif (id == M_WALL) { albedo = backdropAlbedo(backdropCoord(p, abs(n.y) > 0.999 ? n : vec3(n.x, 0.0, n.z) / length(n.xz)))',
  ],
]

/**
 * The room shader with the cove, and the Monstera's primitives and materials,
 * traced from one intersection call site (see monstera-shader.ts).
 */
function withPlant(fsRoom: string) {
  return editShader(
    editShader(replaceSpan(fsRoom, ONE_TRACE), COVE_EDITS),
    MONSTERA_EDITS,
  )
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
 * So each of those contributions is clamped to FIREFLY_MAX (room-light.ts)
 * in luminance before it joins the sample: light reaching the camera by any reflection
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
 * rows below it repeat the floor seen there (see frame() in room.ts).
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

/** The room's shaders, every load-time edit applied. */
export interface RoomShaders {
  /** The page's vertex shader, which every pass shares. */
  vs: string
  /** The path-tracing pass: the page's fs-room, edited. */
  fsRoom: string
  /** The tone-mapping pass: the page's fs-out with the filmic curve. */
  outFrag: string
}

/**
 * The room's shaders, ready to compile. The first call installs the
 * renderer's GLSL chunks, edited, into THREE.ShaderChunk, before it waits
 * for anything. Rejects if a shader fails to load or an edit misses its
 * anchor.
 */
export async function roomShaders(): Promise<RoomShaders> {
  if (!chunksInstalled) {
    installPathTracingChunks(THREE)
    const chunks = THREE.ShaderChunk as Record<string, string>
    chunks.pathtracing_main = gBufferMain(
      freshStart(offCentre(chunks.pathtracing_main)),
      editShader,
    )
    chunksInstalled = true
  }

  const [vs, fsOut, fsLoaded] = await shaders
  const fsRoom = testHooks().roomBrokenShader
    ? `${fsLoaded}\n#error broken for a test`
    : fsLoaded

  // filmic curve in place of the demo's plain Reinhard; the output shader
  // applies sqrt() for display gamma, and the wallpaper reads the result as
  // display sRGB
  const outFrag = fsOut.replace(
    'void main()',
    'uniform float uExposure;\nvec3 ReinhardToneMapping(vec3 c) { c *= uExposure; vec3 x = max(vec3(0.0), c);\n' +
      '  vec3 m = (x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14); return clamp(m * m, 0.0, 1.0); }\n\nvoid main()',
  )

  return {
    vs,
    fsRoom: withGBuffer(
      clampFireflies(
        withMaterials(withPlant(skyApart(fsRoom)), editShader, replaceSpan),
      ),
      editShader,
    ),
    outFrag,
  }
}
