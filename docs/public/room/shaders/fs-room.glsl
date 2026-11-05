precision highp float;
precision highp int;
precision highp sampler2D;

#include <pathtracing_uniforms_and_defines>

#define MAX_GROUPS 8
uniform sampler2D tPrims;
uniform int uNumGroups;
uniform mat4 uGInv[MAX_GROUPS];
uniform mat3 uGRot[MAX_GROUPS];
uniform vec3 uGMin[MAX_GROUPS];
uniform vec3 uGMax[MAX_GROUPS];
uniform ivec2 uGRange[MAX_GROUPS];

uniform vec3 uWinC0, uWinE1, uWinE2;
uniform vec3 uLampC0, uLampE1, uLampE2;
uniform vec3 uBulbs[12];
uniform float uSky, uWindow, uUplight, uCandles, uWarmth;
// backdrop ("the canvas"): the colour escaping rays pick up, and the base colour of walls and floor
uniform vec3 uBgLab;                 // backdrop in OKLab
uniform float uRingL, uRingC;        // tauler hue ring
uniform float uCellI, uCellSize, uCellHues, uSeed, uWash, uWashSize;

// material ids (must match scene.js)
#define M_WALL 0
#define M_FLOOR 1
#define M_CHROME 2
#define M_TEAK 3
#define M_TEAK_DARK 4
#define M_GAP 5
#define M_LEATHER 6
#define M_STONE 7
#define M_LAMPHEAD 8
#define M_MARBLE 9
#define M_GLASS_DARK 10
#define M_RUG 11
#define M_FRAME 12
#define M_ART 13
#define M_WINDOW 14
#define M_UPLIGHT 15
#define M_BULB 16
#define M_BOOK_CREAM 17
#define M_BOOK_RED 18
#define M_CERAMIC 19
#define M_FRINGE 20
#define M_GROUT 21

vec3 rayOrigin, rayDirection;
vec3 hitNormal, hitLocal;
int hitMat;
int hitPrim;

#include <pathtracing_random_functions>
#include <pathtracing_sphere_intersect>
#include <pathtracing_opencylinder_intersect>
#include <pathtracing_cappedcylinder_intersect>
#include <pathtracing_capsule_intersect>
#include <pathtracing_box_intersect>
#include <pathtracing_boundingbox_intersect>
#include <pathtracing_quad_intersect>

/* ---------------- procedural helpers ---------------- */
float hash13(vec3 p) { p = fract(p * 0.1031); p += dot(p, p.zyx + 31.32); return fract((p.x + p.y) * p.z); }
float vnoise(vec3 p) {
	vec3 i = floor(p), f = fract(p); f = f * f * (3.0 - 2.0 * f);
	return mix(mix(mix(hash13(i), hash13(i + vec3(1,0,0)), f.x), mix(hash13(i + vec3(0,1,0)), hash13(i + vec3(1,1,0)), f.x), f.y),
	           mix(mix(hash13(i + vec3(0,0,1)), hash13(i + vec3(1,0,1)), f.x), mix(hash13(i + vec3(0,1,1)), hash13(i + vec3(1,1,1)), f.x), f.y), f.z);
}
float fbm(vec3 p) { float s = 0.0, a = 0.5; for (int i = 0; i < 4; i++) { s += a * vnoise(p); p *= 2.03; a *= 0.5; } return s; }

vec3 teak(vec3 p) {
	float warp = fbm(p * vec3(0.012, 0.25, 0.25)) * 5.0;
	float ring = 0.5 + 0.5 * sin((p.y * 1.3 + p.z * 0.35 + warp * 3.0) * 3.14159);
	float fine = vnoise(p * vec3(0.05, 3.0, 3.0));
	float k = 0.8 + ring * 0.22 + (fine - 0.5) * 0.18 + (fbm(p * 0.02) - 0.5) * 0.25;
	return vec3(0.40, 0.17, 0.05) * k;
}

float inRect(vec2 p, vec2 a, vec2 b) { return step(a.x, p.x) * step(p.x, b.x) * step(a.y, p.y) * step(p.y, b.y); }
float lineDist(vec2 p, vec2 a, vec2 b) { vec2 pa = p - a, ba = b - a; float h = clamp(dot(pa, ba) / dot(ba, ba), 0.0, 1.0); return length(pa - ba * h); }

// original abstract print; uv in [0,1] with v measured from the top of the mat
vec3 artwork(vec2 uv) {
	vec3 navy = vec3(0.016, 0.04, 0.16), cream = vec3(0.86, 0.82, 0.72), red = vec3(0.57, 0.035, 0.025);
	vec3 black = vec3(0.008), blue = vec3(0.018, 0.06, 0.36), yellow = vec3(0.75, 0.46, 0.03);
	vec2 px = uv * vec2(596.0, 776.0);               // print area including the mat, in "canvas" units
	vec2 m = vec2(30.0, 30.0);
	if (px.x < m.x || px.y < m.y || px.x > 596.0 - m.x || px.y > 776.0 - m.y) return navy;
	vec2 p = px - m; float AW = 536.0, AH = 716.0;
	vec3 c = cream;
	c = mix(c, red, inRect(p, vec2(0.56 * AW, 0.48 * AH), vec2(AW, AH)));
	c = mix(c, black, step(length(p - vec2(0.38 * AW, 0.30 * AH)), 0.27 * AW));
	c = mix(c, cream, inRect(p, vec2(0.10 * AW, 0.30 * AH), vec2(0.70 * AW, 0.35 * AH)));
	c = mix(c, blue, step(length(p - vec2(0.56 * AW, 0.62 * AH)), 0.22 * AW) * step(p.x, 0.56 * AW));
	c = mix(c, yellow, inRect(p, vec2(0.66 * AW, 0.18 * AH), vec2(0.79 * AW, 0.18 * AH + 0.13 * AW)));
	for (int i = 0; i < 7; i++) {
		float y0 = (0.72 + float(i) * 0.035) * AH;
		c = mix(c, black, inRect(p, vec2(0.12 * AW, y0), vec2(0.46 * AW, y0 + 0.014 * AH)));
	}
	c = mix(c, black, step(lineDist(p, vec2(0.08 * AW, 0.55 * AH), vec2(0.92 * AW, 0.42 * AH)), 2.5));
	c = mix(c, black, step(abs(p.x - 0.8 * AW), 2.5) * step(0.05 * AH, p.y) * step(p.y, 0.95 * AH));
	c = mix(c, black, step(length(p - vec2(0.56 * AW, AH)), 0.2 * AW) * step(p.x, 0.56 * AW));
	return c * (0.96 + 0.08 * vnoise(vec3(px * 0.8, 1.0)));
}

vec3 windowEmission(vec3 p) {
	float gx = fract((p.x + 1400.0) / 20.0), gy = fract((p.y - 313.0) / 18.4);
	float grout = 1.0 - step(0.03, gx) * step(gx, 0.97) * step(0.033, gy) * step(gy, 0.967);
	vec2 c = vec2(gx, gy) - 0.5;
	float glow = 0.82 + 0.18 * (1.0 - dot(c, c) * 2.0) + 0.05 * sin(gy * 40.0 + gx * 9.0);
	return mix(vec3(0.97, 0.99, 1.04) * glow, vec3(0.05), grout) * uWindow * 4.0;
}
/* ---- tauler-style colour cells, in OKLCH (after tauler-wallpaper.js) ---- */
vec3 okToLin(vec3 c) {
	float l = c.x + 0.3963377774 * c.y + 0.2158037573 * c.z;
	float m = c.x - 0.1055613458 * c.y - 0.0638541728 * c.z;
	float s = c.x - 0.0894841775 * c.y - 1.2914855480 * c.z;
	l = l * l * l; m = m * m * m; s = s * s * s;
	return vec3( 4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
	            -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
	            -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s);
}
float ihash(ivec2 p, float salt) { return hash13(vec3(vec2(p) + 0.5, uSeed * 17.0 + salt)); }
// hue index of a lattice corner (one of uCellHues evenly spaced ring hues)
float cornerHue(ivec2 c) { return floor(ihash(c, 1.0) * uCellHues); }
vec3 backdropAlbedo(vec2 q) {
	vec3 bg = okToLin(uBgLab);
	if (uCellI <= 0.0) return clamp(bg, 0.0, 1.0);
	// hue: smoothstep-weighted mean of the 4 surrounding corners, blended as numbers along the ring
	vec2 f = q / uCellSize, i0 = floor(f), w = smoothstep(0.0, 1.0, f - i0);
	ivec2 c = ivec2(i0);
	float h00 = cornerHue(c), h10 = cornerHue(c + ivec2(1, 0)), h01 = cornerHue(c + ivec2(0, 1)), h11 = cornerHue(c + ivec2(1, 1));
	float hIdx = mix(mix(h00, h10, w.x), mix(h01, h11, w.x), w.y);
	float hue = hIdx / max(uCellHues, 1.0) * 6.28318530718;
	// fade amount: a second lattice of wash values, squared so most corners wash lightly
	vec2 fw = q / uWashSize, j0 = floor(fw), ww = smoothstep(0.0, 1.0, fw - j0);
	ivec2 d = ivec2(j0);
	float a00 = pow(ihash(d, 5.0), 2.0), a10 = pow(ihash(d + ivec2(1, 0), 5.0), 2.0), a01 = pow(ihash(d + ivec2(0, 1), 5.0), 2.0), a11 = pow(ihash(d + ivec2(1, 1), 5.0), 2.0);
	float wash = mix(mix(a00, a10, ww.x), mix(a01, a11, ww.x), ww.y) * uWash;
	float amount = uCellI * (1.0 - wash);
	// ease from the backdrop toward the ring colour in OKLCH
	float bgC = length(uBgLab.yz);
	float L = mix(uBgLab.x, uRingL, amount), C = mix(bgC, uRingC, amount);
	vec3 lin = okToLin(vec3(L, C * cos(hue), C * sin(hue)));
	// out of gamut: drop chroma, keep lightness and hue
	if (any(lessThan(lin, vec3(0.0))) || any(greaterThan(lin, vec3(1.0)))) {
		float lo = 0.0, hi = C;
		for (int k = 0; k < 10; k++) { float mid = 0.5 * (lo + hi); vec3 t = okToLin(vec3(L, mid * cos(hue), mid * sin(hue))); if (all(greaterThanEqual(t, vec3(0.0))) && all(lessThanEqual(t, vec3(1.0)))) lo = mid; else hi = mid; }
		lin = okToLin(vec3(L, lo * cos(hue), lo * sin(hue)));
	}
	return clamp(lin, 0.0, 1.0);
}
// surface coordinate on the room's big planes (cm), so the cells run continuously over walls and floor
vec2 backdropCoord(vec3 p, vec3 n) {
	if (abs(n.y) > 0.5) return vec2(p.x, -p.z - 400.0);      // floor
	if (abs(n.x) > 0.5) return vec2(176.0 + p.z, p.y);       // right wall, unrolled from the corner
	return vec2(p.x, p.y);                                   // back wall
}

vec3 warmColor() { return mix(vec3(1.0, 0.86, 0.68), vec3(1.0, 0.62, 0.30), uWarmth); }
vec3 sky(vec3 d) { return clamp(okToLin(uBgLab), 0.0, 1.0) * uSky; }

/* ---------------- scene ---------------- */
float SceneIntersect(vec3 ro, vec3 rd)
{
	float t = INFINITY;
	hitPrim = -1;
	vec3 n;
	int ex;
	for (int g = 0; g < MAX_GROUPS; g++)
	{
		if (g >= uNumGroups) break;
		vec3 lro = (uGInv[g] * vec4(ro, 1.0)).xyz;
		vec3 lrd = mat3(uGInv[g]) * rd;
		if (BoundingBoxIntersect(uGMin[g], uGMax[g], lro, 1.0 / lrd) >= t) continue;
		for (int i = uGRange[g].x; i < uGRange[g].y; i++)
		{
			vec4 a0 = texelFetch(tPrims, ivec2(0, i), 0);
			vec4 a1 = texelFetch(tPrims, ivec2(1, i), 0);
			vec4 a2 = texelFetch(tPrims, ivec2(2, i), 0);
			int k = int(a0.x + 0.5);
			float d = INFINITY;
			if (k == 0) d = BoxIntersect(a1.xyz, a2.xyz, lro, lrd, n, ex);
			else if (k == 1) d = CapsuleIntersect(a1.xyz, a1.w, a2.xyz, a1.w, lro, lrd, n);
			else if (k == 2) d = CappedCylinderIntersect(a1.xyz, a2.xyz, a1.w, lro, lrd, n);
			else if (k == 3) {
				vec3 a3 = texelFetch(tPrims, ivec2(3, i), 0).xyz;
				vec3 a4 = texelFetch(tPrims, ivec2(4, i), 0).xyz;
				d = QuadIntersect(a1.xyz, a2.xyz, a3, a4, lro, lrd, TRUE);
				n = cross(a2.xyz - a1.xyz, a4 - a1.xyz);
			}
			else { d = SphereIntersect(a1.w, a1.xyz, lro, lrd); n = (lro + lrd * d) - a1.xyz; }
			if (d < t)
			{
				t = d;
				hitNormal = uGRot[g] * n;
				hitLocal = lro + lrd * d;
				hitMat = int(a0.y + 0.5);
				hitPrim = i;
			}
		}
	}
	return t;
}

// material: type, albedo, emission, roughness, specular scale
void getMaterial(int id, vec3 p, vec3 n, out int type, out vec3 albedo, out vec3 emission, out float rough, out float specK)
{
	type = DIFF; albedo = vec3(0.8); emission = vec3(0); rough = 1.0; specK = 1.0;
	if (id == M_WALL) { albedo = backdropAlbedo(backdropCoord(p, n)) * 0.93 * (0.985 + 0.03 * vnoise(p * 0.08)); }
	else if (id == M_FLOOR) { type = COAT; rough = 0.22; specK = 0.9; albedo = backdropAlbedo(backdropCoord(p, n)) * 0.84 * (0.94 + 0.1 * fbm(p * 0.015)); }
	else if (id == M_CHROME) { type = SPEC; albedo = vec3(0.9, 0.9, 0.91); rough = 0.04; }
	else if (id == M_TEAK) { type = COAT; rough = 0.3; specK = 0.7; albedo = teak(p); }
	else if (id == M_TEAK_DARK) { albedo = vec3(0.07, 0.035, 0.015); }
	else if (id == M_GAP) { albedo = vec3(0.012, 0.007, 0.004); }
	else if (id == M_LEATHER) { type = COAT; rough = 0.45; specK = 0.6; albedo = vec3(0.52, 0.36, 0.05) * (0.92 + 0.12 * vnoise(p * 1.5)); }
	else if (id == M_STONE) { type = COAT; rough = 0.08; albedo = vec3(0.008); }
	else if (id == M_LAMPHEAD) { albedo = vec3(0.015); }
	else if (id == M_MARBLE) {
		type = COAT; rough = 0.05;
		float tt = fbm(p * 0.12);
		float v = abs(sin((p.x * 0.12 + p.z * 0.2 + tt * 7.0) * 1.2));
		float vein = pow(1.0 - v, 14.0) * 0.35 + pow(1.0 - abs(sin(p.x * 0.3 - p.z * 0.1 + tt * 11.0)), 30.0) * 0.2;
		albedo = vec3(0.86) * (1.0 - vein);
	}
	else if (id == M_GLASS_DARK) { type = COAT; rough = 0.0; albedo = vec3(0.006); }
	else if (id == M_RUG) {
		vec2 c = fract(p.xz / 0.8) - 0.5;
		float knot = 1.0 - dot(c, c) * 2.2;
		float fleck = step(0.8, vnoise(vec3(p.xz * 1.3, 3.0)));
		albedo = mix(vec3(0.66, 0.64, 0.53), vec3(0.42, 0.48, 0.42), fleck) * (0.75 + 0.25 * knot) * (0.92 + 0.12 * fbm(p * 0.05));
	}
	else if (id == M_FRINGE) {
		float s = fract(p.x / 1.1);
		float green = step(0.62, vnoise(vec3(floor(p.x / 1.1), 0.0, 7.0)));
		albedo = mix(vec3(0.75, 0.72, 0.62), vec3(0.07, 0.15, 0.11), green) * (0.6 + 0.4 * sin(s * 3.14159));
	}
	else if (id == M_FRAME) { albedo = vec3(0.72, 0.63, 0.48); }
	else if (id == M_ART) {
		vec2 uv = vec2((p.x - (32.0 - 32.0 + 2.2)) / 59.6, 1.0 - (p.y - 80.2) / 77.6);
		albedo = artwork(uv);
	}
	else if (id == M_WINDOW) { type = LIGHT; emission = windowEmission(p); }
	else if (id == M_UPLIGHT) { type = LIGHT; emission = warmColor() * uUplight * 110.0; }
	else if (id == M_BULB) { type = LIGHT; emission = warmColor() * uCandles * 60.0; }
	else if (id == M_BOOK_CREAM) { albedo = vec3(0.78, 0.75, 0.66); }
	else if (id == M_BOOK_RED) { albedo = vec3(0.30, 0.05, 0.03); }
	else if (id == M_CERAMIC) { type = COAT; rough = 0.04; albedo = vec3(0.85); }
	else if (id == M_GROUT) { albedo = vec3(0.5); }
}

/* next-event estimation toward one randomly chosen light */
vec3 sampleLights(vec3 x, vec3 nl)
{
	float r = rng();
	vec3 P, ln, Le; float A, p; int target;
	if (r < 0.5) {
		p = 0.5; target = M_WINDOW;
		P = uWinC0 + rng() * uWinE1 + rng() * uWinE2; ln = vec3(0, 0, 1);
		A = length(cross(uWinE1, uWinE2)); Le = windowEmission(P);
	} else if (r < 0.75) {
		p = 0.25; target = M_UPLIGHT;
		P = uLampC0 + rng() * uLampE1 + rng() * uLampE2; ln = vec3(0, 1, 0);
		A = length(cross(uLampE1, uLampE2)); Le = warmColor() * uUplight * 110.0;
	} else {
		// one of the 12 candle bulbs, sampled by solid angle
		int bi = int(min(rng() * 12.0, 11.0));
		vec3 c = uBulbs[bi]; float rad = 1.5;
		vec3 L = c - x; float d2 = dot(L, L);
		float cosMax = sqrt(max(0.0, 1.0 - rad * rad / d2));
		float ct = mix(cosMax, 1.0, rng()), st = sqrt(max(0.0, 1.0 - ct * ct)), ph = rng() * TWO_PI;
		vec3 w = L / sqrt(d2);
		vec3 U = normalize(cross(abs(w.y) < 0.9 ? vec3(0, 1, 0) : vec3(1, 0, 0), w)), Vv = cross(w, U);
		vec3 dir = normalize(U * cos(ph) * st + Vv * sin(ph) * st + w * ct);
		float cx = dot(nl, dir);
		if (cx <= 0.0) return vec3(0);
		float t = SceneIntersect(x, dir);
		if (hitMat != M_BULB || t >= INFINITY) return vec3(0);
		return warmColor() * uCandles * 60.0 * cx * 2.0 * (1.0 - cosMax) * 12.0 / 0.25;
	}
	vec3 L = P - x; float d2 = dot(L, L); vec3 dir = L * inversesqrt(d2);
	float cx = dot(nl, dir), cl = dot(-dir, ln);
	if (cx <= 0.0 || cl <= 0.0) return vec3(0);
	float t = SceneIntersect(x, dir);
	if (hitMat != target || t >= INFINITY) return vec3(0);
	return Le * cx * cl * A / (PI * d2) / p;
}

vec3 CalculateRadiance(out vec3 objectNormal, out vec3 objectColor, out float objectID, out float pixelSharpness)
{
	vec3 acc = vec3(0), mask = vec3(1);
	bool lastSpec = true;
	int type; vec3 albedo, emission; float rough, specK;

	for (int bounces = 0; bounces < 7; bounces++)
	{
		float t = SceneIntersect(rayOrigin, rayDirection);
		if (t >= INFINITY) {
			acc += mask * sky(rayDirection);
			if (bounces == 0) pixelSharpness = 1.0;
			break;
		}
		vec3 x = rayOrigin + rayDirection * t;
		vec3 n = normalize(hitNormal);
		vec3 nl = dot(n, rayDirection) < 0.0 ? n : -n;
		getMaterial(hitMat, hitLocal, n, type, albedo, emission, rough, specK);

		if (bounces == 0) {
			objectNormal += nl; objectColor += albedo; objectID = float(hitPrim);
		}
		if (type == LIGHT) {
			if (lastSpec) acc += mask * emission;
			if (bounces == 0) pixelSharpness = 1.0;
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
		acc += mask * min(sampleLights(xo, nl), vec3(14.0));
		rayOrigin = xo;
		rayDirection = randomCosWeightedDirectionInHemisphere(nl);
		lastSpec = false;

		if (bounces >= 2) {
			float q = max(max(mask.r, mask.g), mask.b);
			if (rng() > q) break;
			mask /= q;
		}
	}
	return min(acc, vec3(24.0));
}

void SetupScene(void) {}

#include <pathtracing_main>
