
precision highp float;
precision highp int;
uniform sampler2D tImg;
uniform sampler2D tBlue;
uniform bool uPixel;
uniform int uDevP;          // device pixels per dither pixel
uniform int uMatrix;        // 0 bayer 8x8, 1 bayer 4x4, 2 checker, 3 blue noise
uniform bool uShared;       // one threshold for all channels (tauler) or offset per channel
uniform float uStepL, uStepC, uStepH, uAnchorL, uCut, uDither, uHueOffset;

float bayer8(ivec2 p) {
	int x = p.x & 7, y = p.y & 7, xr = x ^ y;
	int m = ((xr & 1) << 5) | ((y & 1) << 4) | ((xr & 2) << 2) | ((y & 2) << 1) | ((xr & 4) >> 1) | ((y & 4) >> 2);
	return (float(m) + 0.5) / 64.0;
}
float bayer4(ivec2 p) {
	int x = p.x & 3, y = p.y & 3, xr = x ^ y;
	int m = ((xr & 1) << 3) | ((y & 1) << 2) | ((xr & 2)) | ((y & 2) >> 1);
	return (float(m) + 0.5) / 16.0;
}
float threshold(ivec2 p) {
	float t;
	if (uMatrix == 0) t = bayer8(p);
	else if (uMatrix == 1) t = bayer4(p);
	else if (uMatrix == 2) t = (float((p.x + p.y) & 1) + 0.5) / 2.0;
	else t = texelFetch(tBlue, ivec2(p.x & 127, p.y & 127), 0).r;
	return 0.5 + (t - 0.5) * uDither;
}
// Bjorn Ottosson's OKLab (public domain)
vec3 lin2oklab(vec3 c) {
	float l = 0.4122214708 * c.r + 0.5363325363 * c.g + 0.0514459929 * c.b;
	float m = 0.2119034982 * c.r + 0.6806995451 * c.g + 0.1073969566 * c.b;
	float s = 0.0883024619 * c.r + 0.2817188376 * c.g + 0.6299787005 * c.b;
	l = pow(l, 1.0 / 3.0); m = pow(m, 1.0 / 3.0); s = pow(s, 1.0 / 3.0);
	return vec3(0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
	            1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
	            0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s);
}
vec3 oklab2lin(vec3 c) {
	float l = c.x + 0.3963377774 * c.y + 0.2158037573 * c.z;
	float m = c.x - 0.1055613458 * c.y - 0.0638541728 * c.z;
	float s = c.x - 0.0894841775 * c.y - 1.2914855480 * c.z;
	l = l * l * l; m = m * m * m; s = s * s * s;
	return vec3( 4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
	            -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
	            -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s);
}
bool inGamut(vec3 c) { return all(greaterThanEqual(c, vec3(-0.0005))) && all(lessThanEqual(c, vec3(1.0005))); }
// out-of-gamut colours lose chroma, keeping lightness and hue (as in tauler-wallpaper)
vec3 lchToLin(float L, float C, float h) {
	vec2 ab = vec2(cos(h), sin(h));
	vec3 lin = oklab2lin(vec3(L, C * ab));
	if (!inGamut(lin)) {
		float lo = 0.0, hi = C;
		for (int i = 0; i < 12; i++) { float mid = 0.5 * (lo + hi); if (inGamut(oklab2lin(vec3(L, mid * ab)))) lo = mid; else hi = mid; }
		lin = oklab2lin(vec3(L, lo * ab));
	}
	return clamp(lin, 0.0, 1.0);
}

void main() {
	ivec2 dp = ivec2(gl_FragCoord.xy) / uDevP;     // the dither pixel this device pixel belongs to
	vec3 disp = texelFetch(tImg, dp, 0).rgb;
	if (!uPixel) { pc_fragColor = vec4(disp, 1.0); return; }
	ivec2 g = ivec2(dp.x, -dp.y);                    // threshold grid anchored to the top-left
	vec3 lin = disp * disp;                          // the output pass encodes with sqrt()
	vec3 lab = lin2oklab(max(lin, vec3(0.0)));
	float L = lab.x, C = length(lab.yz), h = degrees(atan(lab.z, lab.y));
	float tL = threshold(g);
	float tC = uShared ? tL : threshold(g + ivec2(3, 5));
	float tH = uShared ? tL : threshold(g + ivec2(6, 2));
	// each channel on its own equal grid; lightness measured from the anchor (the wall), so it stays flat
	float Lq = clamp(uAnchorL + floor((L - uAnchorL) / uStepL + tL) * uStepL, 0.0, 1.0);
	float Cq = max(floor(max(C - uCut, 0.0) / uStepC + tC), 0.0) * uStepC;
	float nH = max(1.0, floor(360.0 / uStepH + 0.5)), stepH = 360.0 / nH;
	float hq = mod(floor(fract(h / 360.0 - uHueOffset) * nH + tH), nH) * stepH + uHueOffset * 360.0;
	pc_fragColor = vec4(sqrt(lchToLin(Lq, Cq, radians(hq))), 1.0);   // same display encoding as the smooth view
}
