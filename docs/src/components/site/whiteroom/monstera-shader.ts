/**
 * The room shader's additions for the Monstera (monstera-scene.js), and a path
 * loop with one intersection call site, as load-time edits of the vendored
 * fs-room shader, in the style of whiteroom.ts's own: each goes in at one
 * anchor that must appear exactly once, or the room fails to start
 * (editShader, replaceSpan).
 *
 * - ONE_TRACE replaces the page's light sampling and path loop with the same
 *   estimator traced from one SceneIntersect call site. The scene's
 *   intersection code is inlined at every call site; the page's three (the
 *   loop and two in its light sampling) made the shader about three times the
 *   size, a cost on every pixel, plant on screen or not.
 * - MONSTERA_EDITS add the plant's primitive kinds (leaf blade, open frustum,
 *   cluster box), materials and thin-leaf transmission.
 * - LEAF_MAPS is a shader of its own, run once: it bakes each leaf's
 *   outline, splits, holes, veins and drooping straps into a texture (tLeaf)
 *   that the room shader samples, a few lookups per ray instead of all that
 *   code in the intersection loop.
 *
 * The GLSL is kept byte for byte with the plant's standalone prototype (the
 * zip page plus an apply script), where it was tuned; edit both together.
 */

/** Light sampling and the path loop with one SceneIntersect call site: replaces the span from `start` up to `end`. */
export const ONE_TRACE = {
  start: '/* next-event estimation toward one randomly chosen light */',
  end: 'void SetupScene(void) {}',
  text: `/* next-event estimation toward one randomly chosen light: returns what the light adds if the
   shadow ray (from x along dir) reaches it, hitting material target; zero when there is nothing
   to trace. The caller traces the shadow ray. */
vec3 sampleLight(vec3 x, vec3 nl, out vec3 dir, out int target)
{
	float r = rng();
	dir = vec3(0, 1, 0); target = -1;
	if (r < 0.75) {
		vec3 P, ln, Le; float A, p;
		if (r < 0.5) {
			p = 0.5; target = M_WINDOW;
			P = uWinC0 + rng() * uWinE1 + rng() * uWinE2; ln = vec3(0, 0, 1);
			A = length(cross(uWinE1, uWinE2)); Le = windowEmission(P);
		} else {
			p = 0.25; target = M_UPLIGHT;
			P = uLampC0 + rng() * uLampE1 + rng() * uLampE2; ln = vec3(0, 1, 0);
			A = length(cross(uLampE1, uLampE2)); Le = warmColor() * uUplight * 110.0;
		}
		vec3 L = P - x; float d2 = dot(L, L); dir = L * inversesqrt(d2);
		float cx = dot(nl, dir), cl = dot(-dir, ln);
		if (cx <= 0.0 || cl <= 0.0) return vec3(0);
		return Le * cx * cl * A / (PI * d2) / p;
	}
	// one of the 12 candle bulbs, sampled by solid angle
	target = M_BULB;
	int bi = int(min(rng() * 12.0, 11.0));
	vec3 c = uBulbs[bi]; float rad = 1.5;
	vec3 L = c - x; float d2 = dot(L, L);
	float cosMax = sqrt(max(0.0, 1.0 - rad * rad / d2));
	float ct = mix(cosMax, 1.0, rng()), st = sqrt(max(0.0, 1.0 - ct * ct)), ph = rng() * TWO_PI;
	vec3 wz = L / sqrt(d2);
	vec3 U = normalize(cross(abs(wz.y) < 0.9 ? vec3(0, 1, 0) : vec3(1, 0, 0), wz)), Vv = cross(wz, U);
	dir = normalize(U * cos(ph) * st + Vv * sin(ph) * st + wz * ct);
	float cx = dot(nl, dir);
	if (cx <= 0.0) return vec3(0);
	return warmColor() * uCandles * 60.0 * cx * 2.0 * (1.0 - cosMax) * 12.0 / 0.25;
}

/* The page's path loop, with one SceneIntersect call site: each pass traces either the path's
   next ray or the shadow ray of the light sample taken at the last diffuse hit. SceneIntersect
   is inlined at every call site; the page's three sites (one here, two in sampleLights) made the
   shader about three times the size, which costs time on every pixel, plant on screen or not.
   Same estimator as the page's: at most 7 bounces, the same light sampling and roulette. */
vec3 CalculateRadiance(out vec3 objectNormal, out vec3 objectColor, out float objectID, out float pixelSharpness)
{
	vec3 acc = vec3(0), mask = vec3(1);
	bool lastSpec = true;
	int type; vec3 albedo, emission; float rough, specK;
	bool shadow = false, alive = true;
	vec3 shadowOrigin = vec3(0), shadowDir = vec3(0), shadowW = vec3(0); int shadowTarget = -1;
	int bounces = 0;

	for (int pass = 0; pass < 14; pass++)
	{
		if (!shadow && (!alive || bounces >= 7)) break;
		float t = SceneIntersect(shadow ? shadowOrigin : rayOrigin, shadow ? shadowDir : rayDirection);
		if (shadow) {
			if (hitMat == shadowTarget && t < INFINITY) acc += shadowW;
			shadow = false;
			continue;
		}
		int b = bounces++;
		if (t >= INFINITY) {
			acc += mask * sky(rayDirection);
			if (b == 0) pixelSharpness = 1.0;
			break;
		}
		vec3 x = rayOrigin + rayDirection * t;
		vec3 n = normalize(hitNormal);
		vec3 nl = dot(n, rayDirection) < 0.0 ? n : -n;
		getMaterial(hitMat, hitLocal, n, type, albedo, emission, rough, specK);

		if (b == 0) {
			objectNormal += nl; objectColor += albedo; objectID = float(hitPrim);
		}
		if (type == LIGHT) {
			if (lastSpec) acc += mask * emission;
			if (b == 0) pixelSharpness = 1.0;
			break;
		}

		float cosi = max(0.0, dot(-rayDirection, nl));
		if (type == SPEC) {
			float f = pow(1.0 - cosi, 5.0);
			mask *= albedo + (1.0 - albedo) * f;
			rayDirection = randomDirectionInSpecularLobe(nl, reflect(rayDirection, nl), rough);
			rayOrigin = x + nl * 0.02;
			lastSpec = true;
			continue;
		}
		if (type == COAT) {
			float F = (0.04 + 0.96 * pow(1.0 - cosi, 5.0)) * specK;
			if (rng() < F) {
				rayDirection = randomDirectionInSpecularLobe(nl, reflect(rayDirection, nl), rough);
				rayOrigin = x + nl * 0.02;
				lastSpec = true;
				continue;
			}
		}
		// diffuse
		mask *= albedo;
		vec3 xo = x + nl * 0.02;
		vec3 w = sampleLight(xo, nl, shadowDir, shadowTarget);
		if (w.r + w.g + w.b > 0.0) { shadow = true; shadowOrigin = xo; shadowW = mask * min(w, vec3(14.0)); }
		rayOrigin = xo;
		rayDirection = randomCosWeightedDirectionInHemisphere(nl);
		lastSpec = false;

		if (b >= 2) {
			float q = max(max(mask.r, mask.g), mask.b);
			if (rng() > q) alive = false;   // after this hit's shadow ray
			else mask /= q;
		}
	}
	return min(acc, vec3(24.0));
}

`,
}

const DEFINES = `// MONSTERA: material ids (must match MAT in the scene script) and primitive kinds
#define M_LEAF 22
#define M_PETIOLE 23
#define M_STEM 24
#define M_AERIAL_ROOT 25
#define M_POT 26
#define M_SOIL 27
#define M_FURL 28
#define K_LEAF 5
#define K_CONE 6
#define K_CLUSTER 7
`

const FUNCTIONS = `/* ==================== MONSTERA: primitives and materials (begin) ==================== */
// Leaf maps (monstera-leaves.glsl, baked once): per leaf, over X in [-0.3, 1.05] along the midrib
// and Y in [-0.65, 0.65] across (leaf lengths), (distance to the blade's edge, > 0 on it; height
// below the sheet; vein strength; freshness + blemish), filtered.
uniform sampler2D tLeaf;
vec4 leafMap(vec2 P, float idx)
{
	vec2 size = vec2(textureSize(tLeaf, 0));
	vec2 uv = clamp(vec2((P.x + 0.3) / 1.35, (P.y + 0.65) / 1.3), 0.5 / size.x, 1.0 - 0.5 / size.x);
	return textureLod(tLeaf, vec2(uv.x, (idx + uv.y) * size.x / size.y), 0.0);
}

// Leaf blade: the sheet z = L (bend X^2 + twist X Y + cup Y^2) in the frame (T, S, N), less each
// strap's droop, cut out where the map's distance is negative. a = base, b = base + T L,
// c = S (1 + leaf index), d = (bend, cup, twist). The sheet is intersected analytically; a few
// regula falsi steps from each of its roots then find the drooping straps.
// Writes primAux = side * (2 + 16 * freshness (0..15) + blemish (0..15) + vein * 0.99): side +1
// upper face, -1 underside.
float LeafIntersect(vec3 a, vec3 b, vec3 c, vec3 dd, vec3 ro, vec3 rd, out vec3 n)
{
	vec3 T = b - a; float L = length(T); T /= L;
	float idx = length(c) - 1.0;
	vec3 S = c / (1.0 + idx), N = cross(T, S);
	idx = floor(idx + 0.5);
	vec3 o = ro - a;
	vec3 lo = vec3(dot(o, T), dot(o, S), dot(o, N)) / L;
	vec3 D = vec3(dot(rd, T), dot(rd, S), dot(rd, N)) / L;
	float Bd = dd.x, Cp = dd.y, Tw = dd.z;
	float A = Bd * D.x * D.x + Tw * D.x * D.y + Cp * D.y * D.y;
	float B = 2.0 * Bd * lo.x * D.x + Tw * (lo.x * D.y + lo.y * D.x) + 2.0 * Cp * lo.y * D.y - D.z;
	float C = Bd * lo.x * lo.x + Tw * lo.x * lo.y + Cp * lo.y * lo.y - lo.z;
	float t0, t1;
	int nr = 2;   // number of roots (a dynamic loop bound keeps the loop body from being inlined twice)
	if (abs(A) < 1e-9) { t0 = -C / B; t1 = INFINITY; nr = 1; }
	else {
		float disc = B * B - 4.0 * A * C;
		if (disc < 0.0) return INFINITY;
		disc = sqrt(disc);
		float q = -0.5 * (B + (B < 0.0 ? -disc : disc));
		t0 = q / A; t1 = C / q;
		if (t0 > t1) { float tmp = t0; t0 = t1; t1 = tmp; }
	}
	if (t1 <= 0.004) return INFINITY;
	if (t0 <= 0.004) { t0 = t1; nr = 1; }
	// Both roots: the camera is often below a blade's sheet, and a ray from below crosses it
	// twice, often first off the blade and then on it.
	for (int r = 0; r < nr; r++) {
		float t = r == 0 ? t0 : t1;
		vec2 P = lo.xy + t * D.xy;
		if (P.x < -0.3 || P.x > 1.05 || abs(P.y) > 0.65) continue;
		// The drooping straps: from the sheet's root, two Newton steps on F(t) = height above the
		// drooped surface, the droop read from the map at the root and once more. That is exact
		// enough because the droop is shallow (a few cm); what makes the straps read apart is
		// mostly their shading, from the droop's slope exaggerated in the normal below. A deeper
		// droop showed loose fragments of strap where grazing rays missed it, or needed a
		// bracketed search (a loop of map reads, inside the scene's primitive loop: about 0.7 ms
		// per sample on every pixel).
		vec4 m = leafMap(P, idx);
		float tb = t;
		if (m.g > 1e-5) {
			tb = t + clamp(m.g / (2.0 * A * t + B), -0.5 * L, 0.5 * L);
			P = lo.xy + tb * D.xy; m = leafMap(P, idx);
			float fb = lo.z + tb * D.z - (Bd * P.x * P.x + Tw * P.x * P.y + Cp * P.y * P.y - m.g);
			tb += clamp(fb / (2.0 * A * tb + B), -0.1 * L, 0.1 * L);
			P = lo.xy + tb * D.xy;
		}
		if (tb <= 0.004 || m.r < 0.0) continue;
		// surface gradient: the sheet's and the map's height (the straps, the fold and ripple, the
		// veins' relief), exaggerated 3x to read at pixel scale
		const float STEP = 0.006;
		vec2 dw = 3.0 * (vec2(leafMap(P + vec2(STEP, 0.0), idx).g, leafMap(P + vec2(0.0, STEP), idx).g) - m.g) / STEP;
		vec3 g = vec3(-(2.0 * Bd * P.x + Tw * P.y) + dw.x, -(Tw * P.x + 2.0 * Cp * P.y) + dw.y, 1.0);
		// the side from the true slope: the exaggerated one would flip it where a strap curls
		float side = dot(D, g - vec3(0.667 * dw, 0.0)) < 0.0 ? 1.0 : -1.0;
		n = T * g.x + S * g.y + N * g.z;
		primAux = side * (2.0 + 16.0 * floor(m.a) + floor(fract(m.a) * 17.7) + 0.99 * clamp(m.b, 0.0, 1.0));
		return tb;
	}
	return INFINITY;
}

// Open frustum a -> b (radii r0 -> r1) with optional annular caps (inner radius < 0 = no cap).
float ConeFrustumIntersect(vec3 a, vec3 b, float r0, float r1, float capBIn, float capAIn, vec3 ro, vec3 rd, out vec3 n)
{
	vec3 w = b - a; float h = length(w); w /= h;
	float k = (r1 - r0) / h;
	vec3 o = ro - a;
	float od = dot(o, w), dw = dot(rd, w);
	vec3 op = o - w * od, dp = rd - w * dw;
	float rr = r0 + k * od;
	float A = dot(dp, dp) - k * k * dw * dw;
	float B = 2.0 * (dot(op, dp) - k * dw * rr);
	float C = dot(op, op) - rr * rr;
	float best = INFINITY;
	float disc = B * B - 4.0 * A * C;
	if (disc >= 0.0 && abs(A) > 1e-9) {
		disc = sqrt(disc);
		float ta = (-B - disc) / (2.0 * A), tb = (-B + disc) / (2.0 * A);
		for (int r = 0; r < 2; r++) {
			float t = r == 0 ? min(ta, tb) : max(ta, tb);
			float y = od + t * dw;
			if (t > 0.004 && y >= 0.0 && y <= h) {
				vec3 rad = op + t * dp;
				n = normalize(rad) - w * k;
				best = t;
				break;
			}
		}
	}
	if (abs(dw) > 1e-6) {
		float tc = (h - od) / dw;
		if (capBIn >= 0.0 && tc > 0.004 && tc < best) {
			float rad = length(op + tc * dp);
			if (rad <= r1 && rad >= capBIn) { best = tc; n = w; }
		}
		tc = -od / dw;
		if (capAIn >= 0.0 && tc > 0.004 && tc < best) {
			float rad = length(op + tc * dp);
			if (rad <= r0 && rad >= capAIn) { best = tc; n = -w; }
		}
	}
	return best;
}

void monsteraMaterial(int id, vec3 p, vec3 n, inout int type, inout vec3 albedo, inout float rough, inout float specK)
{
	// One noise lookup, shared by all the plant's materials at a scale each: every lookup in the
	// shader is inlined, and its code and registers cost every pixel, plant on screen or not.
	float m = fbm(p * (id == M_PETIOLE ? vec3(0.3, 2.5, 0.3) : vec3(1.6)));   // about 0.2 .. 0.75
	if (id == M_LEAF) {
		// Matte with a waxy sheen: diffuse, lightened toward the sky's tone at grazing angles, most
		// on faces turned up to the sky. A real glossy coat (COAT) sparkled: at 48 samples its
		// rare reflections of the window are white dots. New leaves are a lighter, yellower green.
		float aux = abs(hitAux) - 2.0, fresh = floor(aux) / 255.0, vein = fract(aux) / 0.99;
		bool top = hitAux > 0.0;
		vec3 nn = normalize(n);
		float sheen = pow(1.0 - abs(dot(nn, rayDirection)), 2.0) * (0.4 + 0.6 * abs(nn.y));
		vec3 base = top ? mix(vec3(0.011, 0.032, 0.016), vec3(0.04, 0.1, 0.028), fresh)
		                : mix(vec3(0.05, 0.095, 0.045), vec3(0.08, 0.16, 0.05), fresh);
		albedo = mix(base * (0.85 + 0.3 * m), top ? vec3(0.08, 0.15, 0.06) : vec3(0.18, 0.26, 0.1), vein * (top ? 0.45 : 0.6));
		albedo = mix(albedo, vec3(0.3, 0.35, 0.34), (top ? 0.75 : 0.3) * sheen);
	}
	else if (id == M_PETIOLE || id == M_FURL) {
		// petioles a lighter green than the blades; a furled new leaf paler still
		albedo = (id == M_FURL ? vec3(0.09, 0.2, 0.04) : vec3(0.05, 0.12, 0.03)) * (0.8 + 0.4 * m);
		type = COAT; rough = 0.45; specK = 0.3;
	}
	else if (id == M_STEM) {
		// the stems and the petioles' sheathing bases: olive green, ringed with old leaf scars
		float ring = smoothstep(0.85, 1.0, abs(sin(p.y * 0.9 + m * 4.0)));
		albedo = mix(vec3(0.05, 0.08, 0.02), vec3(0.11, 0.09, 0.04), ring) * (0.75 + 0.5 * m);
		type = COAT; rough = 0.45; specK = 0.3;
	}
	else if (id == M_AERIAL_ROOT) {
		albedo = vec3(0.17, 0.105, 0.05) * (0.6 + 0.8 * m);
	}
	else if (id == M_POT) {
		// satin white glaze, faintly uneven; the inside, below the rim, in shadow and dulled
		albedo = vec3(0.8, 0.79, 0.76) * (0.96 + 0.08 * m);
		type = COAT; rough = 0.35; specK = 0.25;
		if (n.y < 0.5 && dot(n, vec3(p.x, 0.0, p.z)) < 0.0) albedo *= 0.5;
	}
	else if (id == M_SOIL) {
		// dark potting mix with chips of bark and the odd grain of perlite
		albedo = vec3(0.035, 0.022, 0.014) * (0.5 + 1.3 * m);
		float h = hash13(floor(p * 0.8));
		albedo = mix(albedo, vec3(0.17, 0.1, 0.055) * (0.5 + m), step(0.8, h));
		albedo = mix(albedo, vec3(0.75, 0.74, 0.7), step(0.975, hash13(floor(p * 1.7))));
	}
}
/* ==================== MONSTERA: primitives and materials (end) ==================== */
`

const DISPATCH = `			else if (k >= K_LEAF) {   // MONSTERA: cluster box / leaf blade / frustum
				vec4 a3 = texelFetch(tPrims, ivec2(3, i), 0);
				if (k == K_CLUSTER) {
					// a ray that misses the box skips the primitives inside it
					if (BoundingBoxIntersect(a1.xyz, a2.xyz, lro, 1.0 / lrd) >= t) i += int(a3.x + 0.5);
				}
				else {
					vec4 a4 = texelFetch(tPrims, ivec2(4, i), 0);
					if (k == K_LEAF) d = LeafIntersect(a1.xyz, a2.xyz, a3.xyz, a4.xyz, lro, lrd, n);
					else {
						d = ConeFrustumIntersect(a1.xyz, a2.xyz, a3.x, a3.y, a3.z, a4.x, lro, lrd, n);
					}
				}
			}
`

const TRANSMISSION = `		// MONSTERA: thin-leaf diffuse transmission (backlit leaves glow green): with probability PT
		// the diffuse bounce below continues on the far side of the blade, tinted yellow-green
		if (hitMat == M_LEAF) {
			const float PT = 0.3;
			if (rng() < PT) { nl = -nl; albedo = vec3(0.045, 0.12, 0.015) / PT; }
			else albedo /= 1.0 - PT;
		}
`

/** [anchor, replacement] pairs for fs-room, applied after ONE_TRACE. */
export const MONSTERA_EDITS: [string, string][] = [
  // material ids and primitive kinds
  ['#define M_GROUT 21\n', '#define M_GROUT 21\n' + DEFINES],
  // per-hit leaf data
  [
    'int hitPrim;\n',
    'int hitPrim;\nfloat primAux, hitAux;   // MONSTERA: per-hit leaf data: side * (2 + 16 * freshness + blemish + vein)\n',
  ],
  // the primitives' intersections and the materials
  [
    '/* ---------------- scene ---------------- */',
    FUNCTIONS + '\n/* ---------------- scene ---------------- */',
  ],
  // the new kinds in the intersection loop, and the leaf data of the closest hit
  [
    '\t\t\telse { d = SphereIntersect(a1.w, a1.xyz, lro, lrd); n = (lro + lrd * d) - a1.xyz; }',
    DISPATCH +
      '\t\t\telse { d = SphereIntersect(a1.w, a1.xyz, lro, lrd); n = (lro + lrd * d) - a1.xyz; }',
  ],
  [
    '\t\t\t\thitPrim = i;\n',
    '\t\t\t\thitPrim = i;\n\t\t\t\thitAux = primAux;   // MONSTERA\n',
  ],
  // the materials
  [
    '\telse if (id == M_GROUT) { albedo = vec3(0.5); }\n',
    '\telse if (id == M_GROUT) { albedo = vec3(0.5); }\n\telse if (id >= M_LEAF) monsteraMaterial(id, p, n, type, albedo, rough, specK);   // MONSTERA\n',
  ],
  // light through the leaves
  [
    '\t\t// diffuse\n\t\tmask *= albedo;\n',
    TRANSMISSION + '\t\t// diffuse\n\t\tmask *= albedo;\n',
  ],
]

/** Texels per leaf map side; the target is LEAF_RES wide and LEAF_RES * leaves high. */
export const LEAF_RES = 256

/**
 * The leaf-map shader: a fragment shader for the room's full-screen triangle,
 * uniform uLeafMat[16] (the scene's leafMats: one code per leaf).
 */
export const LEAF_MAPS = `precision highp float;
precision highp int;

/* MONSTERA leaf maps: the blades' outlines, splits, holes, veins, relief and blemishes, baked once
 * on the GPU into a texture the path tracer samples filtered (fs-room, LeafIntersect): a few
 * lookups per ray instead of all of this, whose code alone, inlined in the scene's intersection
 * loop, cost every pixel about 1 ms per sample.
 *
 * One LEAF_RES x LEAF_RES map per leaf, stacked vertically (the target is LEAF_RES wide and
 * LEAF_RES * uLeafCount high), over the blade's own coordinates: X in [-0.3, 1.05] along the
 * midrib (0 = petiole junction, 1 = tip) and Y in [-0.65, 0.65] across, in leaf lengths. Texel:
 *   r  signed distance to the blade's edge, roughly (> 0 on the blade): outline, splits, holes
 *   g  how far the surface there lies below the leaf's curved sheet: the straps' droop, a fold
 *      along the midrib, the margin's ripple, and the relief of the veins (sunk into the upper
 *      face, the blade between them puffed up), whose slope the blade's shading normal takes
 *   b  vein strength, for shading: midrib, primary veins out to the margin, faint secondary
 *      veins between them, the paler rim
 *   a  freshness in 16ths (0 old, dark .. 15 new, light) plus 0.9 * blemish (0 healthy, 0.5
 *      yellowed, 1 dry and brown), for shading
 */
uniform float uLeafMat[16];   // each leaf's code, from the scene (ROOM_SCENE.leafMats): maturity in 64ths + 64 * freshness in 16ths + 1024 if torn + 2048 * age (0..2)
#define LEAF_RES 256.0

float hash13(vec3 p) { p = fract(p * 0.1031); p += dot(p, p.zyx + 31.32); return fract((p.x + p.y) * p.z); }

// Lateral veins are the lines X = s + |Y| (C0 + C1 s) + K Y^2 leaving the midrib at s; the margin
// sits at |Y| = marginY(s) along each vein, so splits and holes can follow the veins exactly.
#define LEAF_C0 -0.9
#define LEAF_C1 2.6
#define LEAF_K 0.35

// The pinnate split in interval i (between lateral veins i and i + 1), from the margin in along
// the middle of the interval: x = where it ends inside, as a share of the half-width (2 = no
// split; negative = a tear ending at -x, a narrow slit); y = its line's offset across the
// interval. Young blades have none; mature ones are
// split at nearly every interval, about half way to the midrib.
bool gTorn;   // this leaf is torn (set by main)

vec2 leafSplit(float i, float side, float mat, float seed, float nv)
{
	float h1 = hash13(vec3(seed, i * 1.7 + side, 1.3)), h2 = hash13(vec3(seed, i * 2.3 + side, 5.9));
	bool split = i >= 0.0 && i < nv && h1 < mat * 1.4 - 0.3;
	// a torn leaf has one more split, a ragged tear, in an interval chosen by its seed
	bool tear = gTorn && !split && i == floor(1.0 + hash13(vec3(seed, 9.1, 2.2)) * (nv - 2.0)) && side == (hash13(vec3(seed, 4.3, 1.7)) < 0.5 ? -1.0 : 1.0);
	return vec2(split ? 0.22 + 0.35 * h2 + 0.3 * (1.0 - mat) : tear ? -0.6 - 0.15 * h2 : 2.0, (hash13(vec3(seed, i * 3.1 + side, 4.4)) - 0.5) * 0.4);
}

void main()
{
	float li = floor(gl_FragCoord.y / LEAF_RES);
	vec2 uv = vec2(gl_FragCoord.x, gl_FragCoord.y - li * LEAF_RES) / LEAF_RES;
	float X = -0.3 + uv.x * 1.35, Y = -0.65 + uv.y * 1.3;
	float code = uLeafMat[int(li)], seed = 11.0 + li * 7.0;
	float age = floor(code / 2048.0);
	code -= age * 2048.0;
	gTorn = code > 1023.5;
	code -= gTorn ? 1024.0 : 0.0;
	float mat = mod(code, 64.0) / 63.0, fresh = floor(code / 64.0);
	float nv = floor(4.0 + 3.0 * mat), ds = 0.77 / nv;

	// the two halves differ: in width, and in where their veins leave the midrib
	float side = Y < 0.0 ? -1.0 : 1.0, ha = hash13(vec3(seed, 3.1, 7.7));
	float ay = abs(Y) * (1.0 + 0.2 * side * (ha - 0.5));
	float s = (X - LEAF_K * ay * ay - ay * LEAF_C0) / (1.0 + ay * LEAF_C1);
	float sc = clamp(s, 0.001, 0.999);
	float Ym = 1.62 * pow(sc, 0.62) * pow(1.0 - sc, 1.8) * (0.92 + 0.12 * mat);   // half-width along the vein
	Ym *= 1.0 + 0.025 * sin(s * 23.0 + seed + side) + 0.02 * sin(s * 51.0 - seed * 2.0);   // an uneven margin
	float f = ay / Ym, c = LEAF_C0 + LEAF_C1 * s;
	float w = ds * (1.0 + ay * LEAF_C1) * inversesqrt(1.0 + c * c);   // the interval's width across the veins
	float u = (s - 0.03 - (side < 0.0 ? 0.45 * ds * (ha - 0.5) : 0.0)) / ds, i = floor(u), fr = u - i - 0.5;   // fr: -0.5 at vein i .. 0.5 at vein i + 1

	// distance to the edge: the outline ...
	float dist = s <= 0.0 || s >= 1.0 ? -0.02 : (Ym - ay) * inversesqrt(1.0 + c * c) * 1.2;
	float droop = 0.0;
	if (i >= 0.0 && i < nv && dist > -0.05) {
		vec2 sp = leafSplit(i, side, mat, seed, nv);
		float tw = sp.x < 0.0 ? 0.3 : 1.0;   // a tear is narrow
		sp.x = abs(sp.x);
		float d = (fr - sp.y) * w;   // across the interval from the split's line
		// ... a split: a slit with a rounded inner end that opens into a wide gap at the margin
		if (f > sp.x) {
			float g = (f - sp.x) / (1.0 - sp.x);
			float hw = (0.006 + 0.016 * g + 0.012 * pow(max(0.0, (f - 0.8) / 0.2), 2.0)) * sqrt(min(1.0, (f - sp.x) / 0.035));
			dist = min(dist, abs(d) - hw * tw);
		}
		// ... and holes on the same line: an outer row of long ones inside the split (in an
		// unsplit interval, where it would be) and on mature blades an inner row of small ones by
		// the midrib. Lens-shaped, long along the veins, wider at the outer end, not quite straight.
		float h3 = hash13(vec3(seed, i * 0.7 + side, 2.1)), h4 = hash13(vec3(seed, i * 1.3 + side, 8.2));
		float hlO = (0.1 + 0.09 * h4) * (0.6 + 0.4 * mat);
		float fcO = min(sp.x, 0.86) - 0.05 - 0.05 * h3 - hlO;
		bool outer = h3 < mat * 2.0 - 0.9 && fcO - hlO > 0.19;
		bool inner = fract(h3 * 7.31) < mat * 1.6 - 1.0;
		bool isOuter = f > 0.19 || !inner;
		if (isOuter ? outer : inner) {
			float fc = isOuter ? fcO : 0.12 + 0.05 * h3, hl = isOuter ? hlO : 0.035 + 0.03 * h4;
			float e = (f - fc) / hl, ec = clamp(e, -1.0, 1.0);
			float hw = w * (isOuter ? 0.1 + 0.1 * h4 : 0.08 + 0.05 * h3) * sqrt(1.0 - ec * ec) * (0.85 + 0.25 * ec) * (1.0 + 0.12 * sin(ec * 4.0 + h3 * 20.0));
			float dc = d - w * ((h4 - 0.5) * 0.12 + (h3 - 0.5) * 0.15 * ec * ec);
			dist = min(dist, -min(hw - abs(dc), (1.0 - abs(e)) * hl * Ym));
		}
		// the strap: one lateral vein and the blade either side of it, out to the splits. It hangs
		// free beyond the shallower of its two splits, each by its own amount, and turns a little
		// about its vein, so the blade is no longer one sheet
		float right = fr > sp.y ? 1.0 : 0.0, j = i + right;
		float f0 = max(sp.x, abs(leafSplit(i + 2.0 * right - 1.0, side, mat, seed, nv).x));
		if (f0 < 1.0 && f > f0) {
			float e = min(f - f0, 1.0 - f0) / (1.0 - f0), h = hash13(vec3(seed, j * 1.9 + side, 6.6));
			float dv = (fr + 0.5 - right) * w;   // across the strap from its vein
			droop = mat * e * ((0.04 + 0.1 * h) * e * (1.0 - f0) * Ym + (h - 0.5) * 0.25 * dv);
		}
	}
	float vd = (0.5 - abs(fr)) * w, fm = min(f, 1.0);
	float mid = exp(-pow(Y / (0.011 * (1.0 - 0.5 * max(0.0, X))), 2.0)) * (1.0 - 0.6 * max(0.0, X));
	float pri = exp(-pow(vd / 0.0055, 2.0)) * (1.0 - fm * 0.6);
	float sec = pow(abs(cos(6.2832 * fr * 2.0 + 0.6 * sin(f * 9.0 + i))), 24.0) * (1.0 - fm * 0.5);
	float vein = max(max(pri * 0.65, mid), max(sec * 0.12, smoothstep(0.95, 1.0, f) * 0.3));
	// the relief: a fold along the midrib, the margin rippling, the midrib's channel and each
	// vein sunk into the upper face, the blade between the veins puffed up
	float h5 = hash13(vec3(seed, 5.5, 3.3));
	float height = droop + (0.03 + 0.04 * h5) * ay
	             + (0.004 + 0.008 * mat) * fm * fm * fm * (1.0 + sin(s * (30.0 + 12.0 * h5) + seed * 3.0 + side))
	             + 0.0008 * mid + 0.0005 * pri + 0.00012 * sec + 0.0009 * (1.0 - sin(3.1416 * (fr + 0.5))) * (1.0 - 0.5 * fm);
	// blemishes: on ageing leaves a yellowing margin, toward the tip, and on the oldest a dry,
	// brown tip and edge; on any leaf the odd small spot
	float blem = min(age, 1.0) * smoothstep(0.62, 1.0, f + 0.1 * sin(s * 37.0 + seed)) * smoothstep(0.15, 0.8, s) * 0.5;
	float dry = step(1.5, age) * smoothstep(0.9, 0.97, s + 0.05 * f + 0.02 * sin(Y * 60.0 + seed))
	          + step(1.5, age) * smoothstep(0.93, 0.99, f) * step(0.55, fract(s * 3.0 + h5));
	for (int k = 0; k < 3; k++) {
		vec3 hk = vec3(hash13(vec3(seed, float(k), 1.9)), hash13(vec3(seed, float(k), 2.7)), hash13(vec3(seed, float(k), 3.4)));
		vec2 c0 = vec2(0.1 + 0.8 * hk.x, (hk.y - 0.5) * 0.6);
		dry = max(dry, smoothstep(1.0, 0.6, length(vec2(X, Y) - c0) / (0.004 + 0.01 * hk.z)) * step(0.45, hk.z));
	}
	blem = dry > 0.5 ? 0.55 + 0.45 * min(dry, 1.0) : max(blem, 0.5 * dry);
	gl_FragColor = vec4(clamp(dist, -0.05, 0.05), height, vein, fresh + 0.9 * blem);
}
`
