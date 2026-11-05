/**
 * The room's materials: physically based, procedural, and different
 * everywhere, as load-time edits of the room shader (after the cove
 * and the plant's, withPlant() in whiteroom.ts), each at an anchor that must
 * appear exactly once.
 *
 * - BSDF (BSDF_LOOP): the page's lobes (a mirror blurred toward a cosine
 *   lobe, with a Fresnel-ish weight scaled by hand) become GGX microfacets,
 *   sampled by their visible normals (Dupuy & Benyoub 2023) and weighted by
 *   Smith's G1, so a rough lobe darkens and widens the way a real one does.
 *   Metals (SPEC) reflect with Schlick's Fresnel on their measured F0.
 *   Dielectrics (COAT) pick the specular lobe with Schlick's probability at
 *   the shading normal (roughness-aware, Fdez-Agüera 2019) and the diffuse
 *   lobe otherwise, so the two never add up to more than the light that came
 *   in. A clearcoat (lacquered teak) is one more such layer on top. Leaves
 *   keep the plant's thin-sheet transmission, now from a per-leaf tint.
 * - Textures (MATERIAL): no images. Each hit evaluates at most three
 *   noise calls whatever its material: a one-octave warp, a 2–4 octave fBm
 *   and, on camera and mirror paths only, a fine detail octave, each at a
 *   frame (scale, stretch, offset) its material picks; the materials only
 *   map those numbers. Every noise call is inlined, and its code and
 *   registers cost every pixel, so one call site each is the budget. The
 *   noise is value noise on a pcg3d integer hash (Jarzynski & Olano 2020),
 *   three channels at once, in object space, with octaves rotated so the
 *   lattice never lines up: nothing repeats. Each group and primitive has
 *   its own seed (hitGroup, hitPrim), so the two chairs and every teak board
 *   differ.
 * - Roughness varies as much as colour: fingerprints on chrome and glass,
 *   lap marks in the paint's sheen, a floor sealer hazed along walking paths
 *   and dusty along the walls, burnished leather, dusty leaves, worn edges.
 * - Albedos are measured or plausible, none above 0.85 (walls and floor
 *   still take theirs from the site theme's page colour).
 *
 * The plant's material function and transmission in monstera-shader.ts are
 * replaced here, at load time; its pot uses MAT.POT (monstera-scene.js),
 * not the chairs' LEATHER.
 *
 * The G-buffer's albedo (denoise.ts) is the albedo getMaterial returns, so
 * every procedural colour is demodulated out of the irradiance and kept
 * sharp by the denoise.
 */

type Edit = [string, string]
type Span = { start: string; end: string; text: string }

/** Globals, hash and noise, the GGX sampler; before the scene's intersection code. */
const HELPERS = `// [tauler] materials
int hitGroup;
vec3 matN, matNc;
vec3 matF0;
float matCoat, matCoatRough;
vec3 matTrans;
bool matDetail;

// pcg3d
uvec3 pcg3d(uvec3 v) {
	v = v * 1664525u + 1013904223u;
	v.x += v.y * v.z; v.y += v.z * v.x; v.z += v.x * v.y;
	v ^= v >> 16u;
	v.x += v.y * v.z; v.y += v.z * v.x; v.z += v.x * v.y;
	return v;
}
vec3 hash3(ivec3 c) { return vec3(pcg3d(uvec3(c))) * (1.0 / 4294967295.0); }
// three independent value noises
vec3 noise3(vec3 p) {
	ivec3 i = ivec3(floor(p)); vec3 f = fract(p); f = f * f * (3.0 - 2.0 * f);
	const ivec3 X = ivec3(1, 0, 0), Y = ivec3(0, 1, 0), Z = ivec3(0, 0, 1);
	return mix(mix(mix(hash3(i), hash3(i + X), f.x), mix(hash3(i + Y), hash3(i + X + Y), f.x), f.y),
	           mix(mix(hash3(i + Z), hash3(i + X + Z), f.x), mix(hash3(i + Y + Z), hash3(i + X + Y + Z), f.x), f.y), f.z);
}
// fBm of noise3
vec3 fbm3(vec3 p, int oct) {
	const mat3 R = mat3(0.0, 1.6, 1.2, -1.6, 0.72, -0.96, -1.2, -0.96, 1.28);
	vec3 s = vec3(0); float a = 1.0, w = 0.0;
	for (int i = 0; i < 4; i++) { if (i >= oct) break; s += a * noise3(p); w += a; p = R * p + 5.3; a *= 0.5; }
	return 2.0 * s / w - 1.0;
}
// 2D cells about jittered points: the gap from the nearest to the second nearest, the nearest's hash and offset
float cell2(vec2 p, out vec3 h, out vec2 d) {
	vec2 i = floor(p); float f1 = 9.0, f2 = 9.0;
	for (int k = 0; k < 9; k++) {
		vec2 c = i + vec2(k % 3 - 1, k / 3 - 1); vec3 r = hash3(ivec3(ivec2(c), 11));
		vec2 v = c + r.xy - p; float e = dot(v, v);
		if (e < f1) { f2 = f1; f1 = e; d = v; h = r; } else if (e < f2) f2 = e;
	}
	return sqrt(f2) - sqrt(f1);
}
// distance to the nearest edge of the hit box, and its size and centre
float boxEdge(vec3 p, out vec3 size, out vec3 mid) {
	vec3 mn = texelFetch(tPrims, ivec2(1, hitPrim), 0).xyz, mx = texelFetch(tPrims, ivec2(2, hitPrim), 0).xyz;
	size = mx - mn; mid = 0.5 * (mn + mx);
	if (texelFetch(tPrims, ivec2(0, hitPrim), 0).x > 0.5) return 1e3;
	vec3 d = min(p - mn, mx - p);
	return d.x + d.y + d.z - min(d.x, min(d.y, d.z)) - max(d.x, max(d.y, d.z));
}
// GGX half vector for view v about n
vec3 ggxHalf(vec3 n, vec3 v, float a) {
	float s = n.z >= 0.0 ? 1.0 : -1.0, k = -1.0 / (s + n.z), b0 = n.x * n.y * k;
	vec3 t = vec3(1.0 + s * n.x * n.x * k, s * b0, -s * n.x), b = vec3(b0, s + n.y * n.y * k, -n.y);
	vec3 vh = normalize(vec3(a * dot(v, t), a * dot(v, b), dot(v, n)));
	float phi = TWO_PI * rng(), z = (1.0 - rng()) * (1.0 + vh.z) - vh.z, r = sqrt(clamp(1.0 - z * z, 0.0, 1.0));
	vec3 h = vec3(r * cos(phi), r * sin(phi), z) + vh;
	return normalize(t * (a * h.x) + b * (a * h.y) + n * max(h.z, 0.0));
}
`

/**
 * getMaterial, rewritten: same signature (specK is unused), plus the
 * globals above. Albedo sources, linear sRGB, roughly: matte white paint
 * 0.8–0.9 (here the theme's page colour × 0.93); teak 0.15–0.3; tan leather
 * 0.25–0.4; Carrara marble 0.75–0.8; wool 0.5–0.6; leaves 0.03–0.15 by
 * wavelength (adaxial, green peak); black paint, PVC, granite 0.03–0.05.
 * Chromium's F0 is (0.55, 0.56, 0.55) (Hoffman, Physics and Math of Shading,
 * SIGGRAPH 2015); dielectrics 0.04 (n ≈ 1.5), marble 0.05 (calcite).
 *
 * The materials, in the order of the shader's branches:
 * - Wall (and cove): matte emulsion (COAT, roughness ~0.72–0.92), the
 *   theme's page colour; rolled on in long vertical strokes, so its tone
 *   drifts by a few per cent over a metre or two and its sheen in lap marks;
 *   touched-up patches a shade off and glossier (paint "flashing"), faint
 *   plaster waviness in the shading normal, roller stipple up close, grime
 *   along the floor and in the corner.
 * - Floor: sealed pale micro-cement (COAT, 0.03–0.6), a little darker than
 *   the walls (page colour × 0.76) so what it reflects shows: trowelled
 *   clouds (±14 %) with darker, glossier burnish marks where strokes
 *   overlapped; the sealer near mirror (0.07) in patches and satin (0.3) in
 *   others, so chair legs, the plant and the sideboard mirror softly and the
 *   blur changes across the slab; hazed along two walking paths (in from the
 *   camera, along the sideboard) and dull with dust along the walls; dark
 *   rubber scuffs on the paths; a slab not quite flat, so reflections waver.
 *   What a pale floor reflects is mostly the equally pale wall, so most of
 *   the time the clouds are what reads; reflections read where dark things
 *   stand on it, and in the dark theme.
 * - Teak: crown-cut veneer. The log's rings are cut at a slant to the pith,
 *   so they run in arches along the board (a cone of rings cut by the face,
 *   its slant and direction per board, some boards near straight); upright
 *   panels are book-matched. Each ring darkens through its latewood and
 *   breaks sharply into the next ring's earlywood, its strength and width
 *   its own; broad colour bands, long straight streaks of tone and the odd
 *   dark mineral line run with the grain. Open pores, teak's tell, are dark
 *   dashes along the grain from a hashed grid that follows the rings, most
 *   in the earlywood (camera and mirror paths only). Interlocked fibres tilt
 *   the oiled wood's normal along the grain in ribbons, so its sheen swaps
 *   with the view (chatoyancy); over it a satin lacquer clearcoat with its
 *   own orange-peel normal (matNc), worn through, lighter and duller, on
 *   every edge.
 * - Leather: thick veg-tan saddle leather, tan (not dyed ochre), mottled in
 *   value and hue; darker, redder and burnished glossy where it is sat on
 *   and held (seat, lumbar, arm pads), lighter and drier where it rolls over
 *   the tubes; hide grain as 2D cells (cell2) of uneven size up close, each
 *   pebble its own facet in a net of rougher grooves; growth creases, and
 *   wrinkles across the width where the sling sags; a burnished edge, a
 *   creased line and a saddle stitch along the sling's sides and round the
 *   arm slings.
 * - Chrome: chromium F0 (SPEC), near mirror, fingerprints in patches.
 * - Granite: polished black, fine feldspar and mica flecks.
 * - Black paint, the cable's PVC, the chairs' feet: satin (0.42–0.5).
 * - Marble: Carrara, soft grey veins and clouding along a strongly warped
 *   field, polish duller in places.
 * - Smoked glass: a mirror-flat face hazed and lightened by fingerprints
 *   and dust.
 * - Rug: knotted cream wool pile in soft-edged tufts (cell2, camera and
 *   mirror paths only), each its own shade of heathered yarn, a few of
 *   undyed brown or sage, fibres combed along each tuft's lean; faint knot
 *   rows and dye-lot streaks; the pile leans one way and is brushed the
 *   other in broad patches, so its shade changes with the view. Whipped
 *   binding on the sides, a flat-woven band with two green stripes at each
 *   end. The fringe: the warp's ends, cords of uneven length and width,
 *   knotted at the rug, wandering and frayed, the floor (its colour and
 *   sealer) showing between them; each cord cream or the stripes' dull
 *   cotton green, in the scene's fringe's irregular runs.
 * - Frame: oiled light ash, grain along each piece. Print: matte paper behind
 *   glazing (a COAT of roughness 0.02) that reflects the room.
 * - Leaves: a waxy cuticle as a real GGX lobe (0.17 new to 0.3 old, duller
 *   where dust lies on old leaves and where a leaf is drying) over a dark,
 *   slightly blue, desaturated green; paler veins, each leaf its own shade and
 *   mottled; an ageing leaf yellowed and dry at its margin (the leaf map's
 *   blemish); diffuse transmission tinted per leaf (matTrans). Stalks satin,
 *   streaked along their length (noise read off the normal), paler at
 *   the knee; stems corky.
 * - Bowl: ochre glaze on stoneware (COAT 0.07–0.11) with orange peel in its
 *   reflections, thinner and paler over the rim. Soil: lumpy potting mix,
 *   bark chips, perlite.
 */
const MATERIAL: Span = {
  start: '// material: type, albedo, emission, roughness, specular scale',
  end: '/* next-event estimation toward one randomly chosen light: returns',
  text: `// [tauler] material
void getMaterial(int id, vec3 p, vec3 n, out int type, out vec3 albedo, out vec3 emission, out float rough, out float specK)
{
	type = COAT; albedo = vec3(0.5); emission = vec3(0); rough = 0.5; specK = 1.0;
	matN = n; matNc = n; matF0 = vec3(0.04); matCoat = 0.0; matCoatRough = 0.1; matTrans = vec3(0);
	if (id == M_WINDOW) { type = LIGHT; emission = windowEmission(p); return; }
	if (id == M_UPLIGHT) { type = LIGHT; emission = warmColor() * uUplight * 110.0; return; }
	if (id == M_BULB) { type = LIGHT; emission = warmColor() * uCandles * 60.0; return; }

	// each object its own
	bool piece = id == M_TEAK || id == M_TEAK_DARK || id == M_FRAME || id == M_LEAF;
	vec3 seed = hash3(ivec3(hitGroup, piece ? hitPrim : -1, id));
	vec3 o = seed * 97.0;
	// wood
	vec3 size, mid, pw = p, gax = vec3(1, 0, 0); float edge = 1e3;
	if (id == M_TEAK || id == M_FRAME || id == M_RUG) {
		edge = boxEdge(p, size, mid);
		if (size.y > 0.8 * max(size.x, size.z)) {
			// upright panels are book-matched: two leaves, one mirrored
			pw = p.yxz; mid = mid.yxz; gax = vec3(0, 1, 0); if (id == M_TEAK) pw.yz = mid.yz + abs(pw.yz - mid.yz);
		}
		else if (size.z > size.x) { pw = p.zyx; mid = mid.zyx; gax = vec3(0, 0, 1); }
	}
	// the frame each material reads the shared noise in
	vec3 q = p / 20.0 + o, qd = p * 2.0; float wa = 0.6;
	if (id == M_WALL) { q = p * vec3(0.025, 0.006, 0.025) + o; qd = p * 2.5; wa = 0.4; }
	else if (id == M_FLOOR) { q = p * vec3(0.022, 0.0, 0.022); qd = vec3(p.x * 0.07 + p.z * 0.04, 0.0, (p.z * 0.07 - p.x * 0.04) * 12.0); wa = 1.4; }
	else if (id == M_TEAK || id == M_TEAK_DARK || id == M_FRAME) { q = (pw + o) * vec3(0.02, 0.25, 0.25); qd = p * 2.5; }
	else if (id == M_LEATHER) { q = p / 11.0 + o; qd = p * 4.0; wa = 1.0; }
	else if (id == M_MARBLE) { q = p / 14.0 + o; qd = p * 0.6; wa = 1.8; }
	else if (id == M_RUG) { q = p * vec3(0.02, 0.1, 0.12); qd = p * vec3(1.8, 1.8, 1.2); wa = 0.5; }
	else if (id == M_FRINGE) { q = p / 12.0; }
	else if (id == M_CHROME || id == M_GLASS_DARK) { q = p / 5.0 + o; qd = p * 0.5; }
	else if (id == M_STONE) { q = p / 8.0; qd = p * 7.0; }
	else if (id == M_POT) { q = p / 4.0; qd = p * 3.0; wa = 1.0; }
	else if (id == M_SOIL) { q = p / 1.6; qd = p * 2.2; wa = 1.2; }
	else if (id == M_PETIOLE || id == M_STEM || id == M_AERIAL_ROOT || id == M_FURL) {
		// stalks: streaked along their length, so the noise runs slowly along the stalk and fast
		// around it, read off the normal, which turns around a tube but not along it
		q = p * 0.08 + n * 2.0 + o; qd = p * 0.25 + n * 7.0;
	}
	else if (id >= M_LEAF) { q = p / 6.0 + o; qd = p * 1.7; }
	vec3 W = noise3(q * 0.37 - 3.1);
	vec3 N = fbm3(q + wa * W, matDetail ? 4 : 2);
	vec3 D = matDetail ? noise3(qd) : vec3(0.5);

	if (id == M_WALL) {
		// matte emulsion
		albedo = backdropAlbedo(backdropCoord(p, abs(n.y) > 0.999 ? n : vec3(n.x, 0.0, n.z) / length(n.xz))) * 0.93;
		float grime = exp(-max(p.y, 0.0) / 4.0) * 0.07 + exp(-min(176.0 - p.x, max(p.z, 0.0)) / 6.0) * 0.04;
		// rolled on in long vertical strokes, a touched-up patch here and there a shade off and glossier
		float touch = smoothstep(0.6, 0.66, W.z);
		albedo *= (1.0 + 0.1 * (W.y - 0.5) + 0.025 * N.x + 0.012 * touch) * (1.0 - grime * (0.6 + 0.6 * N.z));
		rough = 0.84 + 0.08 * N.y + 0.1 * (D.x - 0.5) - 0.12 * touch;
		matN = n + 0.012 * (W - 0.5) + 0.06 * (D - 0.5);
	}
	else if (id == M_FLOOR) {
		// sealed pale micro-cement
		albedo = backdropAlbedo(backdropCoord(p, n)) * 0.76;
		float burnish = pow(1.0 - abs(N.z), 6.0), cloud = smoothstep(-0.45, 0.45, N.x);
		float path = exp(-pow((p.x + 30.0 - 70.0 * sin(p.z / 190.0)) / 60.0, 2.0)) * smoothstep(160.0, 280.0, p.z)
		           + exp(-pow((p.z - 92.0) / 22.0, 2.0)) * smoothstep(150.0, 90.0, abs(p.x + 20.0));
		path *= 0.6 + 0.5 * N.y;
		float dust = exp(-min(176.0 - p.x, max(p.z, 0.0)) / 7.0);
		float scuff = smoothstep(0.88, 0.94, D.x) * path;
		albedo *= (0.86 + 0.28 * cloud - 0.08 * burnish) * (1.0 - 0.25 * scuff) * (1.0 - 0.04 * path);
		albedo = mix(albedo, vec3(dot(albedo, vec3(0.33))), 0.25 * dust);
		rough = clamp(0.07 + 0.24 * smoothstep(-0.2, 0.6, N.y) - 0.06 * burnish + 0.16 * path + 0.2 * dust + 0.2 * scuff, 0.03, 0.6);
		matN = n + 0.02 * vec3(W.x - 0.5, 0.0, W.z - 0.5);
	}
	else if (id == M_TEAK) {
		// crown-cut teak veneer: the log's rings, cut at a slant to the pith, run in arches along the board
		vec2 c = pw.yz - mid.yz + (seed.yz - 0.5) * 12.0;
		float r = length(c) - (seed.x < 0.5 ? -1.0 : 1.0) * (0.05 + 0.2 * abs(seed.x - 0.5)) * (pw.x - mid.x) + 4.0 * (W.x - 0.5) + 0.5 * N.x;
		float yr = r * 1.6 + 1.4 * sin(r * 0.23 + o.x) + 0.3 * N.y, ring = fract(yr);
		vec3 rh = hash3(ivec3(floor(yr), hitPrim, 5));
		// latewood darkens through each ring, sharp into the next ring's earlywood
		float late = pow(ring, 2.0 + 4.0 * rh.y) * (0.3 + 0.7 * rh.x);
		float bi = yr * 0.14, band = mix(hash3(ivec3(floor(bi), hitPrim, 6)).x, hash3(ivec3(floor(bi) + 1.0, hitPrim, 6)).x, smoothstep(0.0, 1.0, fract(bi)));
		// long streaks of tone and the odd mineral line, then open pores: dark dashes along the grain, most in earlywood
		vec3 sh = hash3(ivec3(floor(r * 3.0), hitPrim, 7));
		float pore = 0.12;
		if (matDetail) {
			float pv = r * 11.0; vec3 ph = hash3(ivec3(floor(pv), hitPrim, 8));
			float pu = pw.x * 0.45 + ph.x * 9.0; vec3 pq = hash3(ivec3(floor(pv), floor(pu), hitPrim + 4096));
			float t = fract(pu) - 0.6 * pq.y;
			pore = step(pq.z, 0.8 - 0.6 * ring) * smoothstep(0.3, 0.1, abs(fract(pv) - 0.3 - 0.4 * pq.x)) * smoothstep(0.0, 0.06, t) * smoothstep(0.4, 0.3, t);
		}
		albedo = vec3(0.46, 0.2, 0.06) * vec3(1.0, 1.0 + 0.12 * (seed.y - 0.5), 1.0 + 0.3 * (seed.y - 0.5));
		albedo *= (0.9 + 0.2 * seed.x) * (0.9 + 0.16 * band) * (0.88 + 0.24 * sh.x) * (1.0 - 0.22 * late) * (1.0 - 0.45 * pore);
		albedo = mix(albedo, vec3(0.08, 0.06, 0.04), 0.6 * step(0.975, sh.y) * smoothstep(0.0, 0.6, N.z + 0.3));
		float worn = 1.0 - smoothstep(0.0, 0.6, edge);
		albedo *= 1.0 + 0.3 * worn;
		// interlocked fibres: ribbons whose sheen swaps with the view (chatoyancy), under a lacquer with orange peel
		matN = n + 0.22 * sin(r * 1.3 + 3.0 * W.y) * (uGRot[hitGroup] * gax);
		rough = 0.36 + 0.1 * pore;
		matCoat = 1.0 - 0.7 * worn; matCoatRough = 0.12 + 0.05 * N.y + 0.12 * worn;
		matNc = n + 0.02 * (D - 0.5);
	}
	else if (id == M_TEAK_DARK) { albedo = vec3(0.06, 0.03, 0.014) * (0.9 + 0.2 * N.x); rough = 0.45; }
	else if (id == M_GAP) { type = DIFF; albedo = vec3(0.02, 0.012, 0.007); }
	else if (id == M_LEATHER) {
		// thick veg-tan saddle leather: the sling, its arm slings and the rolled arm pads
		vec3 ln = n * uGRot[hitGroup];
		bool pad = abs(p.x) > 33.0, side = abs(ln.x) > 0.5;
		vec2 uv = side ? p.zy : vec2(p.x, p.y - p.z);
		float e = pad ? (side && p.y < 56.0 ? min(min(p.z + 19.3, 26.7 - p.z), min(p.y - 41.9, 55.7 - p.y)) : 9.0) : 32.3 - abs(p.x);
		// darker, burnished and glossy where it is sat on and held; lighter and drier where it bends over the tubes
		float use = clamp(0.5 + 0.6 * N.y, 0.0, 1.0) * clamp(pad ? smoothstep(50.0, 57.0, p.y) : smoothstep(44.0, 36.0, p.y) + 0.7 * exp(-pow((p.z + 28.0) / 9.0, 2.0)), 0.0, 1.0);
		float bend = pad ? 0.0 : smoothstep(26.0, 33.0, p.z) + smoothstep(68.0, 74.0, p.y);
		float crease = smoothstep(0.95, 1.0, 1.0 - abs(N.z)) * smoothstep(-0.2, 0.4, N.y);
		float sag = pad ? 0.0 : max(1.0 - p.x * p.x / 1100.0, 0.0) * exp(-pow((p.z + 25.0) / 9.0, 2.0)) * step(p.y, 58.0);
		float wv = (p.y - p.z) * 1.7 + 4.0 * N.x, wr = sag * smoothstep(0.2, 1.0, sin(wv));
		// hide grain: pebbles of uneven size, each its own facet, in a net of grooves
		vec3 ch = vec3(0.5); vec2 cd; float gap = 0.12;
		if (matDetail) gap = cell2(uv * 2.2 + 0.8 * W.xy, ch, cd);
		float groove = 1.0 - smoothstep(0.0, 0.25, gap);
		// a burnished edge, a creased line and a saddle stitch along it
		float st = smoothstep(0.09, 0.03, abs(e - 0.85)) * step(0.4, fract((uv.x + uv.y) / 0.55));
		albedo = vec3(0.36, 0.18, 0.07) * (1.0 + 0.2 * N.x) * vec3(1.0 + 0.06 * N.y, 1.0, 1.0 - 0.12 * N.y) * (0.93 + 0.14 * seed.x);
		albedo *= mix(vec3(1.0), vec3(0.6, 0.5, 0.42), use) * (1.0 + 0.2 * min(bend, 1.0)) * (1.0 - 0.16 * groove) * (1.0 - 0.1 * crease) * (1.0 - 0.1 * sag - 0.12 * wr);
		albedo *= (1.0 - 0.5 * smoothstep(0.35, 0.1, e)) * (1.0 - 0.25 * smoothstep(0.06, 0.0, abs(e - 0.5)));
		albedo = mix(albedo, vec3(0.4, 0.29, 0.16), st);
		rough = 0.48 - 0.2 * use + 0.12 * min(bend, 1.0) + 0.12 * groove + 0.08 * crease + 0.08 * wr + 0.1 * st;
		matN = n + 0.3 * (ch - 0.5) * (1.0 - groove) + 0.15 * crease * (D - 0.5) + 0.35 * sag * cos(wv) * (uGRot[hitGroup] * vec3(0.0, 0.7, -0.7));
	}
	else if (id == M_CHROME) {
		// chromium plate
		type = SPEC; albedo = vec3(0.55, 0.556, 0.554);
		rough = 0.04 + 0.12 * smoothstep(0.3, 0.8, N.x);
	}
	else if (id == M_STONE) {
		// polished black granite
		albedo = mix(vec3(0.03), vec3(0.09, 0.085, 0.08), smoothstep(0.75, 0.9, D.x)) * (1.0 + 0.2 * N.x);
		rough = 0.06 + 0.03 * N.y;
	}
	else if (id == M_LAMPHEAD) { albedo = vec3(0.03) * (1.0 + 0.2 * N.x); rough = 0.42 + 0.08 * N.y; }
	else if (id == M_MARBLE) {
		// honed-to-polished Carrara
		float v = smoothstep(0.78, 0.985, 1.0 - abs(N.x)) * 0.4 + smoothstep(0.92, 0.995, 1.0 - abs(N.y)) * 0.2 + 0.12 * smoothstep(-0.2, 0.8, N.z);
		albedo = mix(vec3(0.8, 0.79, 0.77) * (1.0 + 0.03 * N.z), vec3(0.42, 0.43, 0.45), v);
		matF0 = vec3(0.05); rough = 0.07 + 0.05 * smoothstep(0.0, 1.0, N.z);
	}
	else if (id == M_GLASS_DARK) {
		// smoked glass
		float smudge = smoothstep(0.2, 0.9, N.x);
		albedo = vec3(0.008) + 0.03 * smudge * W; rough = 0.015 + 0.2 * smudge * smudge;
	}
	else if (id == M_RUG) {
		// knotted wool pile in tufts; whipped sides, flat-woven ends
		type = DIFF;
		float lean = W.y - 0.5, band = min(p.z - 63.0, 213.0 - p.z);
		vec3 th = vec3(0.5); vec2 td = vec2(0.4); float gap = 0.15;
		if (matDetail) gap = cell2(p.xz * 1.8 + 1.2 * W.xz, th, td);
		vec2 dir = normalize(vec2(th.x - 0.3, sign(lean) + 0.6 * (th.y - 0.5)));
		float tuft = 1.0 - smoothstep(0.15, 0.8, length(td)), fib = sin(dot(td, vec2(-dir.y, dir.x)) * 30.0 + th.z * 30.0);
		vec3 wool = vec3(0.6, 0.56, 0.46) * (0.86 + 0.24 * th.z) * vec3(1.0, 1.0, 1.0 - 0.1 * (th.y - 0.5));
		wool = mix(wool, th.y > 0.5 ? vec3(0.3, 0.25, 0.19) : vec3(0.33, 0.37, 0.29), 0.4 * step(0.95, th.x));
		albedo = wool * (1.0 + 0.08 * N.x) * (1.0 + 0.05 * N.y) * (0.8 + 0.2 * tuft) * (1.0 + 0.08 * fib) * (0.94 + 0.06 * smoothstep(0.0, 0.2, gap)) * (1.0 + 0.25 * smoothstep(0.0, 0.2, abs(lean)) * sign(lean) * rayDirection.z) * (0.96 + 0.04 * sin(p.z * 11.4 + 3.0 * N.x));
		albedo *= mix(0.72, 1.0, smoothstep(0.3, 1.6 + 2.0 * D.y, edge));
		if (n.y > 0.5) matN = n - 0.5 * vec3(td.x, 0.0, td.y) + 0.5 * vec3(dir.x, 0.0, dir.y) * abs(lean);
		else albedo *= 0.75 + 0.2 * sin((p.x + p.z + p.y) * 9.0);
		if (band < 3.4 && n.y > 0.5) {
			albedo = mix(vec3(0.58, 0.55, 0.46), vec3(0.065, 0.115, 0.085), step(0.5, band) * step(band, 1.3) + step(1.9, band) * step(band, 2.7));
			albedo *= 0.86 + 0.14 * sin(p.x * 22.0) * sin(band * 22.0);
			matN = n;
		}
	}
	else if (id == M_FRINGE) {
		// the warp's ends: cords of uneven length, knotted in pairs at the rug, wandering, frayed; the floor between;
		// green or cream by the scene's fringe's 1.1 cm bars (same hash)
		float zf = p.z > 138.0 ? p.z - 213.0 : 63.0 - p.z;
		float x = p.x / 0.7, u = 9.0, w = 0.0, len = 0.0, cc = 0.0; vec3 h;
		for (int k = -1; k <= 1; k++) {
			float c = floor(x) + float(k); vec3 hk = hash3(ivec3(c, p.z > 138.0 ? 1 : 0, 21));
			float lk = 5.0 + 2.5 * hk.y, uk = x - c - 0.5 - (hk.x - 0.5) * 1.4 * zf * zf / 56.0;
			float wk = 0.18 + 0.12 * hk.z + 0.12 * smoothstep(lk - 1.2, lk, zf) + 0.15 * smoothstep(0.4, 0.0, abs(zf - 1.0));
			if (abs(uk) - wk < abs(u) - w && zf < lk) { u = uk; w = wk; len = lk; h = hk; cc = c; }
		}
		if (abs(u) < w && zf < len && (zf < len - 1.2 || D.x > 0.45)) {
			type = DIFF;
			vec3 dye = hash13(vec3(floor((cc + 0.5) * 0.7 / 1.1), 0.0, 7.0)) > 0.62 ? vec3(0.065, 0.115, 0.085) * vec3(1.0 + 0.3 * (h.y - 0.5), 1.0, 1.0 - 0.2 * (h.y - 0.5)) : vec3(0.64, 0.61, 0.53);
			albedo = dye * (0.85 + 0.25 * h.z) * (0.86 + 0.14 * sin((zf * 1.7 + u) * 12.0)) * (0.75 + 0.25 * sqrt(1.0 - u * u / (w * w)));
			matN = n + vec3(1.2 * u / w, 0.0, 0.0);
		}
		else {
			albedo = backdropAlbedo(backdropCoord(p, vec3(0, 1, 0))) * 0.76 * (0.8 + 0.2 * smoothstep(w, w + 0.3, abs(u))) * (0.75 + 0.25 * smoothstep(0.0, 2.0, zf));
			rough = 0.18;
		}
	}
	else if (id == M_FRAME) {
		// light ash
		float grain = smoothstep(0.4, 1.0, sin(pw.y * 9.0 + 5.0 * N.x) * 0.5 + 0.5);
		albedo = vec3(0.5, 0.41, 0.28) * (1.0 + 0.08 * N.y) * (1.0 - 0.12 * grain);
		rough = 0.48;
	}
	else if (id == M_ART) {
		// a matte print behind glass
		vec2 uv = vec2((p.x - (32.0 - 32.0 + 2.2)) / 59.6, 1.0 - (p.y - 80.2) / 77.6);
		albedo = artwork(uv) * 0.93; rough = 0.02;
	}
	else if (id == M_BOOK_CREAM) { type = DIFF; albedo = vec3(0.7, 0.67, 0.58) * (1.0 + 0.05 * N.x); }
	else if (id == M_BOOK_RED) { albedo = vec3(0.28, 0.045, 0.03) * (1.0 + 0.08 * N.x); rough = 0.7; }
	else if (id == M_CERAMIC) { albedo = vec3(0.8, 0.79, 0.76); rough = 0.06 + 0.03 * N.x; }
	else if (id == M_GROUT) { type = DIFF; albedo = vec3(0.5); }
	else if (id == M_LEAF) {
		// a waxy cuticle over a dark, slightly blue green, desaturated as real leaves are (adaxial
		// albedo ~0.02-0.1); new leaves lighter and yellower, each leaf its own shade, mottled;
		// the veins paler, the midrib palest; ageing leaves yellowed and dry at the margin
		float aux = abs(hitAux) - 2.0, fresh = floor(aux / 16.0) / 15.0, blem = mod(floor(aux), 16.0) / 15.0, vein = fract(aux) / 0.99;
		bool top = hitAux > 0.0;
		vec3 base = top ? mix(vec3(0.018, 0.034, 0.023), vec3(0.04, 0.074, 0.03), fresh)
		                : mix(vec3(0.05, 0.075, 0.045), vec3(0.07, 0.105, 0.05), fresh);
		base *= (0.85 + 0.3 * seed.x) * vec3(1.0 + 0.25 * (seed.y - 0.5), 1.0, 1.0 + 0.2 * (seed.z - 0.5)) * (1.0 + 0.2 * N.x + 0.1 * (D.y - 0.5));
		albedo = mix(base, top ? vec3(0.07, 0.12, 0.045) : vec3(0.12, 0.15, 0.08), vein * (top ? 0.65 : 0.7));
		albedo = mix(albedo, vec3(0.2, 0.16, 0.035), smoothstep(0.0, 0.5, blem) * (0.8 + 0.2 * N.z));
		albedo = mix(albedo, vec3(0.13, 0.08, 0.035), smoothstep(0.5, 0.9, blem));
		float dust = top ? smoothstep(-0.1, 0.6, N.y) * (1.0 - fresh) : 0.0;
		albedo = mix(albedo, vec3(0.16, 0.15, 0.13), 0.06 * dust);
		rough = top ? mix(0.3, 0.17, fresh) + 0.1 * N.z + 0.2 * dust + 0.06 * vein + 0.4 * smoothstep(0.4, 0.9, blem) : 0.62;
		matTrans = mix(vec3(0.04, 0.08, 0.015), vec3(0.055, 0.115, 0.02), fresh) * (1.0 + 0.4 * vein) * (1.0 - 0.6 * smoothstep(0.5, 0.9, blem));
	}
	else if (id == M_PETIOLE || id == M_FURL) {
		// satin stalks, finely streaked along their length; the knee and the rolled new leaf paler
		float m = 0.47 + 0.27 * N.x, st = 0.85 + 0.3 * D.x;
		albedo = (id == M_FURL ? vec3(0.075, 0.15, 0.04) : vec3(0.04, 0.085, 0.028)) * (0.8 + 0.4 * m) * st;
		rough = (id == M_FURL ? 0.28 : 0.38) + 0.1 * (D.y - 0.5);
	}
	else if (id == M_STEM) {
		// the stems and the petioles' sheaths: olive green going brown and corky, streaked
		float m = 0.47 + 0.27 * N.x, cork = smoothstep(0.1, 0.6, N.y);
		albedo = mix(vec3(0.045, 0.07, 0.025), vec3(0.1, 0.08, 0.045), cork) * (0.75 + 0.5 * m) * (0.85 + 0.3 * D.x);
		rough = 0.45 + 0.25 * cork;
	}
	else if (id == M_AERIAL_ROOT) { type = DIFF; albedo = vec3(0.15, 0.09, 0.045) * (0.8 + 0.4 * N.x) * (0.8 + 0.4 * D.x); }
	else if (id == M_POT) {
		// ochre glaze on stoneware
		float rim = smoothstep(31.8, 33.0, p.y);
		albedo = mix(vec3(0.4, 0.24, 0.035) * (1.0 + 0.06 * N.x), vec3(0.5, 0.38, 0.2), 0.5 * rim);
		rough = 0.07 + 0.04 * N.y;
		matN = n + 0.02 * (W - 0.5) + 0.01 * (D - 0.5);
	}
	else if (id == M_SOIL) {
		// dark potting mix, lumpy, with chips of bark and grains of perlite
		type = DIFF;
		float m = 0.47 + 0.27 * N.x;
		albedo = vec3(0.03, 0.02, 0.013) * (0.5 + 1.3 * m);
		albedo = mix(albedo, vec3(0.16, 0.095, 0.05) * (0.6 + 0.8 * W.y), smoothstep(0.62, 0.68, N.z * 0.5 + 0.5));
		albedo = mix(albedo, vec3(0.7, 0.69, 0.65), smoothstep(0.86, 0.9, D.z) * step(0.55, W.x));
		matN = n + 0.5 * vec3(N.x, 0.0, N.y) + 0.3 * (D - 0.5);
	}
	matN = normalize(matN);
}

`,
}

/**
 * The path loop's surface scattering, replacing the page's lobes and the
 * plant's transmission (its comment block included): GGX for metals,
 * clearcoat and dielectric coats, then diffuse, leaves transmitting.
 */
const BSDF_LOOP: Span = {
  start: '\t\tfloat cosi = max(0.0, dot(-rayDirection, nl));',
  end: '\t\t// diffuse\n\t\tmask *= albedo;',
  text: `		// [tauler] scattering
		vec3 ns = dot(matN, nl) < 0.0 ? -matN : matN;
		if (dot(ns, rayDirection) >= 0.0) ns = nl;
		float nv = dot(ns, -rayDirection), fc = pow(1.0 - nv, 5.0), a = -1.0;
		if (type == SPEC) a = rough * rough;
		else if (matCoat > 0.0 && rng() < matCoat * (0.04 + 0.96 * fc)) { a = matCoatRough * matCoatRough; vec3 nc = normalize(matNc); ns = dot(nc, nl) < 0.0 ? -nc : nc; }
		else if (type == COAT && rng() < matF0.x + (max(1.0 - rough, matF0.x) - matF0.x) * fc) a = rough * rough;
		if (a >= 0.0) {
			a = max(a, 1e-4);
			vec3 h = ggxHalf(ns, -rayDirection, a);
			float vh = dot(-rayDirection, h);
			rayDirection = reflect(rayDirection, h);
			float nlv = dot(ns, rayDirection);
			if (nlv <= 0.0 || dot(nl, rayDirection) <= 0.0) break;
			mask *= 2.0 * nlv / (nlv + sqrt(a * a + (1.0 - a * a) * nlv * nlv));
			if (type == SPEC) mask *= albedo + (1.0 - albedo) * pow(1.0 - vh, 5.0);
			rayOrigin = x + nl * 0.02;
			lastSpec = true;
			continue;
		}
		// leaves
		if (hitMat == M_LEAF) {
			const float PT = 0.3;
			if (rng() < PT) { nl = -nl; ns = -ns; albedo = matTrans / PT; }
			else albedo /= 1.0 - PT;
		}
`,
}

/** [anchor, replacement] pairs, applied after the spans. */
const EDITS: Edit[] = [
  // the globals and helpers, before the scene's intersection code
  [
    '/* ---------------- scene ---------------- */',
    HELPERS + '\n/* ---------------- scene ---------------- */',
  ],
  // the hit's group, for per-object seeds
  [
    '\t\t\t\thitAux = primAux;   // MONSTERA\n',
    '\t\t\t\thitAux = primAux;   // MONSTERA\n\t\t\t\thitGroup = g;   // [tauler] materials\n',
  ],
  // detail on camera and mirror paths only: after a diffuse bounce only the mean albedo matters
  [
    'getMaterial(hitMat, hitLocal, n, type, albedo, emission, rough, specK);',
    'matDetail = lastSpec;   // [tauler] materials\n\t\tgetMaterial(hitMat, hitLocal, n, type, albedo, emission, rough, specK);',
  ],
  // the diffuse lobe and its light sample about the shading normal; a bounce under the surface
  // is mirrored back out
  [
    'vec3 w = sampleLight(xo, nl, shadowDir, shadowTarget);',
    'vec3 w = sampleLight(xo, ns, shadowDir, shadowTarget);',
  ],
  [
    '\t\trayDirection = randomCosWeightedDirectionInHemisphere(nl);\n',
    '\t\trayDirection = randomCosWeightedDirectionInHemisphere(ns);\n\t\tif (dot(rayDirection, nl) < 0.0) rayDirection = reflect(rayDirection, nl);\n',
  ],
]

/** The plant's own material function, folded into getMaterial above. */
const PLANT_MATERIAL: Span = {
  start: 'void monsteraMaterial(',
  end: '/* ==================== MONSTERA: primitives and materials (end)',
  text: '',
}

/**
 * The room shader with these materials; after withPlant(), before the
 * firefly clamp and the G-buffer edits, whose anchors it keeps.
 */
export function withMaterials(
  fsRoom: string,
  edit: (shader: string, edits: Edit[]) => string,
  span: (shader: string, s: Span) => string,
) {
  let shader = span(fsRoom, MATERIAL)
  shader = span(shader, BSDF_LOOP)
  shader = span(shader, PLANT_MATERIAL)
  return edit(shader, EDITS)
}
