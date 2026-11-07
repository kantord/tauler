/**
 * The wallpaper's GLSL: the two passes wallpaper-gl.ts draws (see its module
 * doc), and the textures the cells pass reads. CELLS is computeWallpaper()'s
 * inner loop and the room's composite; any change to it must keep
 * tests/wallpaper.spec.ts's byte-for-byte match with the JS module.
 */
import { ROOM_HUE_SUB } from './wallpaper-grid'

export const VERTEX = `#version 300 es
void main() {
  gl_Position = vec4(gl_VertexID == 1 ? 3.0 : -1.0, gl_VertexID == 2 ? 3.0 : -1.0, 0.0, 1.0);
}`

/** Pass 1: one fragment per cell; computeWallpaper()'s inner loop, and the room. */
export const CELLS = `#version 300 es
precision highp float;
precision highp int;
precision highp sampler2D;

uniform sampler2D uCols, uRows;     // per column / row: hue corner, hue weight, wash corner, wash weight
uniform sampler2D uHue, uWash;      // the lattices' corners the slice touches
uniform sampler2D uThreshold;       // 8 x 8 dither thresholds
uniform sampler2D uPalette;         // x: hue level; y: (lightness level - uPalL0) * uPalNC + chroma level
uniform ivec2 uOrigin;              // the slice's first cell: column, row on the page
uniform uint uSeedHueNoise, uSeedBias, uSeedEdge;
uniform float uHueAmp, uBiasAmp, uIntensity, uDL, uDC, uSL, uSC, uSH;
uniform int uNH, uPalL0, uPalNC;
uniform ivec2 uWallL;               // the wallpaper's lightness levels, first and last
uniform int uWallC;                 // its last chroma level

// the room (RoomCells in wallpaper-gl.ts)
uniform bool uRoom;
uniform sampler2D uRoomImage;
uniform ivec3 uRoomCells;           // columns; the image's first and last page row
uniform int uRoomFrom;              // the first page row the room reaches
uniform vec3 uRoomEdges;            // device px: top fade's start, bottom fade's start, height
uniform vec3 uFade;                 // L, C, h: each ramp's share of the height
uniform vec4 uWave;                 // depth (share of height), wavelength (CSS px), lacunarity, persistence
uniform vec3 uWarp;                 // phase warp (turns), amplitude warp, rise
uniform int uOctaves;
uniform float uPc;                  // CSS px per cell
uniform float uMatch;               // the mean match's reach (share of height)
uniform float uDetail;              // how far below its top the room's detail eases in (share of height)
uniform vec3 uPresence;             // L, C, h: how much of the room shows (RoomCells.presence)
uniform ivec2 uRoomL;               // the room's lightness levels, first and last
uniform int uRoomC;                 // its last chroma level
uniform vec2 uBg;                   // the background's OKLCH lightness and chroma
uniform int uDevP;
out vec4 colour;

vec4 table(sampler2D t, int i) { return texelFetch(t, ivec2(i & 1023, i >> 10), 0); }

// tauler-wallpaper.js hash3(); Math.imul is a wrapping 32-bit multiply
uint hash3(uint seed, uint i, uint j) {
  uint h = (i * 0x27d4eb2du) ^ (j * 0x165667b1u) ^ (seed * 0x9E3779B1u);
  h = (h ^ (h >> 15u)) * 0x85ebca6bu;
  h = (h ^ (h >> 13u)) * 0xc2b2ae35u;
  return h ^ (h >> 16u);
}
float unit(uint h) { return float(h) * (1.0 / 4294967296.0); }

// the 4-corner mean of lattice t at corner (i, j), weights w
float corners(sampler2D t, int i, int j, vec2 w) {
  float tl = texelFetch(t, ivec2(i, j), 0).r, tr = texelFetch(t, ivec2(i + 1, j), 0).r;
  float bl = texelFetch(t, ivec2(i, j + 1), 0).r, br = texelFetch(t, ivec2(i + 1, j + 1), 0).r;
  float a = tl + (tr - tl) * w.x, b = bl + (br - bl) * w.x;
  return a + (b - a) * w.y;
}

vec3 smootherstep(vec3 t) {
  t = clamp(t, 0.0, 1.0);
  return t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
}
// smooth value noise along x (u >= 0), 0..1: seeded knots at whole u, eased between
float noise1(uint seed, float u) {
  float k = floor(u), t = u - k;
  float a = unit(hash3(seed, uint(k), 0u)), b = unit(hash3(seed, uint(k) + 1u, 0u));
  return mix(a, b, t * t * (3.0 - 2.0 * t));
}
// Where a fade starts at page x (CSS px), -0.5..0.5 of uWave.x: octaves of
// sines, each with a seeded phase that smooth noise bends and an amplitude
// smooth noise scales, summed and soft-limited (ROOM_WAVE).
float wave(uint seed, float x) {
  float h = 0.0, norm = 0.0, f = 1.0 / uWave.y, a = 1.0;
  for (int k = 0; k < 8; k++) {
    if (k >= uOctaves) break;
    uint s = seed + uint(k) * 7919u;
    float phase = unit(hash3(s, 0u, 1u)) + uWarp.x * (2.0 * noise1(s + 1u, x * f * 0.61) - 1.0);
    float amp = a * (1.0 + uWarp.y * (2.0 * noise1(s + 2u, x * f * 0.43) - 1.0));
    h += amp * sin(6.2831853 * (fract(x * f) + phase));
    norm += a * a;
    f *= uWave.z;
    a *= uWave.w;
  }
  return 0.5 * tanh(1.2 * h / sqrt(norm));
}
vec3 srgbToLin(vec3 c) {
  return mix(c / 12.92, pow((c + 0.055) / 1.055, vec3(2.4)), step(vec3(0.04045), c));
}
// Bjorn Ottosson's OKLab (public domain)
vec3 linToOklab(vec3 c) {
  vec3 lms = pow(max(vec3(
    0.4122214708 * c.r + 0.5363325363 * c.g + 0.0514459929 * c.b,
    0.2119034982 * c.r + 0.6806995451 * c.g + 0.1073969566 * c.b,
    0.0883024619 * c.r + 0.2817188376 * c.g + 0.6299787005 * c.b), 0.0), vec3(1.0 / 3.0));
  return vec3(0.2104542553 * lms.x + 0.7936177850 * lms.y - 0.0040720468 * lms.z,
              1.9779984951 * lms.x - 2.4285922050 * lms.y + 0.4505937099 * lms.z,
              0.0259040371 * lms.x + 0.7827717662 * lms.y - 0.8086757660 * lms.z);
}

// The room's mean colour (OKLab) around column x of texel row y: a wide,
// short window (five rows, every 8th column over 128). Its columns sit on a
// fixed grid and the window slides between two neighbouring placements, so
// the mean changes smoothly along x: taps that moved with x would sample
// fresh grain at every column, and the mean would streak.
vec3 roomMean(int x, int y) {
  int base = x - (x & 7);
  float t = float(x & 7) / 8.0;
  vec3 sum = vec3(0.0);
  for (int k = -8; k <= 9; k++) {
    float weight = (k <= 8 ? 1.0 - t : 0.0) + (k >= -7 ? t : 0.0);
    for (int dy = -2; dy <= 2; dy++) {
      ivec2 p = clamp(ivec2(base + 8 * k, y + dy), ivec2(0), ivec2(uRoomCells.x - 1, uRoomCells.z - uRoomCells.y));
      sum += weight * srgbToLin(texelFetch(uRoomImage, p, 0).rgb);
    }
  }
  return linToOklab(sum / 85.0);
}

void main() {
  ivec2 cell = ivec2(gl_FragCoord.xy);
  vec4 cx = table(uCols, cell.x), ry = table(uRows, cell.y);
  uvec2 page = uvec2(uOrigin + cell);

  // 1. hue: 4-corner mean (hues blend as numbers along the ring) + noise
  float hue = corners(uHue, int(cx.x), int(ry.x), vec2(cx.y, ry.y))
    + uHueAmp * (unit(hash3(uSeedHueNoise, page.x, page.y)) - 0.5);
  // 2. fade amount: intensity x (1 - wash) + bias noise
  float amt = uIntensity * (1.0 - corners(uWash, int(cx.z), int(ry.z), vec2(cx.w, ry.w)))
    + uBiasAmp * (unit(hash3(uSeedBias, page.x, page.y)) - 0.5);
  amt = clamp(amt, 0.0, 1.0);
  // the same, smooth: no per-cell noise (for the room's mean match)
  float smooth_ = clamp(uIntensity * (1.0 - corners(uWash, int(cx.z), int(ry.z), vec2(cx.w, ry.w))), 0.0, 1.0);

  // 3. each channel, in grid steps from the background (hue from 0)
  float vL = (uDL * amt) / uSL, vC = (uDC * amt) / uSC, vH = hue / uSH;
  ivec2 rangeL = uWallL;
  int lastC = uWallC;
  bool roomCell = false;

  // the room, where it covers the cell: each channel eases from the
  // wallpaper's (weight 1) to the room's (0) down from the top fade's start,
  // and back up to the wallpaper's down from the bottom fade's
  int row = int(page.y);
  if (uRoom && row >= uRoomFrom && row <= uRoomCells.z && cell.x < uRoomCells.x) {
    float y = float(row * uDevP);
    float x = (float(page.x) + 0.5) * uPc;
    float top = (y - uRoomEdges.x) / uRoomEdges.z - uWave.x * (wave(uSeedEdge, x) + 0.5 - uWarp.z);
    float bottom = (y - uRoomEdges.y) / uRoomEdges.z - uWave.x * (0.5 + wave(uSeedEdge + 101u, x));
    vec3 w = max(1.0 - smootherstep(vec3(top) / uFade), smootherstep(vec3(bottom) / uFade));
    w = 1.0 - (1.0 - w) * uPresence;  // the room fades in as it gathers samples
    if (any(lessThan(w, vec3(1.0)))) {
      // The room's detail eases in below its top, over ROOM_DETAIL: above
      // that it is its wide mean (row by row; above the image, its first
      // row's). Repeating the first row up would draw vertical streaks of
      // its grain, and detail starting at once, a line along the room's top.
      int texel = uRoomCells.z - max(row, uRoomCells.y);
      float detail = smoothstep(0.0, uDetail, (y - uRoomEdges.x) / uRoomEdges.z);
      vec3 lab = detail > 0.0
        ? linToOklab(srgbToLin(texelFetch(uRoomImage, ivec2(cell.x, texel), 0).rgb))
        : vec3(0.0);
      if (detail < 1.0) lab = mix(roomMean(cell.x, texel), lab, detail);
      // Match the room's mean to the wallpaper's near its edges: the
      // difference between the wallpaper's smooth colour and the room's
      // local mean at the edge row is added to the room, all of it at the
      // edge, none at uMatch in. So the fade carries only the room's detail,
      // not a jump in lightness or chroma that would read as a band.
      float y0 = (y - uRoomEdges.x) / uRoomEdges.z, y1 = (y - uRoomEdges.y) / uRoomEdges.z;
      bool nearTop = y0 < -y1;
      float pull = 1.0 - smootherstep(vec3((nearTop ? y0 : -y1) / uMatch)).x;
      if (pull > 0.0) {
        int edgeRow = nearTop ? uRoomCells.z - int(uRoomEdges.x) / uDevP
                              : uRoomCells.z - int(uRoomEdges.y) / uDevP;
        vec3 mean = roomMean(cell.x, edgeRow);
        lab.x += (uBg.x + uDL * smooth_ - mean.x) * pull;
        // a and b move by the difference too, toward the wallpaper's smooth
        // chroma at its smooth hue (scaling the room's chroma instead would
        // amplify the grain of a near-grey room into streaks of colour)
        float hueS = corners(uHue, int(cx.x), int(ry.x), vec2(cx.y, ry.y));
        vec2 wallAB = (uBg.y + uDC * smooth_) * vec2(cos(hueS), sin(hueS));
        lab.yz += (wallAB - mean.yz) * pull;
      }
      float C = length(lab.yz), n = float(uNH);
      float rH = C < 0.005 ? vH : atan(lab.z, lab.y) / uSH;  // a grey cell has no hue to give
      float dH = mod(vH - rH + 1.5 * n, n) - 0.5 * n;          // the shorter way round
      vL = mix((lab.x - uBg.x) / uSL, vL, w.x);
      vC = mix((C - uBg.y) / uSC, vC, w.y);
      vH = rH + dH * w.z;
      rangeL = uRoomL;
      lastC = uRoomC;
      roomCell = true;
    }
  }

  // 4. the one quantize: each channel on its grid, one shared threshold
  float th = texelFetch(uThreshold, ivec2(page & 7u), 0).r;
  int qL = clamp(int(floor(vL + th)), rangeL.x, rangeL.y);
  int qC = clamp(int(floor(vC + th)), 0, lastC);
  // hue in palette columns, SUB per wallpaper level (see ROOM_HUE_SUB): a
  // room cell on the fine grid, a wallpaper cell on its own grid, scaled
  const int SUB = ${ROOM_HUE_SUB};
  int nF = uNH * SUB;
  int qH = roomCell ? int(floor(vH * float(SUB) + th)) : int(floor(vH + th)) * SUB;
  qH = qH < 0 ? qH + nF * ((nF - 1 - qH) / nF) : qH - nF * (qH / nF);
  colour = texelFetch(uPalette, ivec2(qH, (qL - uPalL0) * uPalNC + qC), 0);
}`

/** The textures pass 1 reads, and its sampler uniforms. */
export const SAMPLERS = [
  ['cols', 'uCols'],
  ['rows', 'uRows'],
  ['hue', 'uHue'],
  ['wash', 'uWash'],
  ['threshold', 'uThreshold'],
  ['palette', 'uPalette'],
] as const

/** Pass 2: every device pixel copies its cell. */
export const PRESENT = `#version 300 es
precision highp float;
precision highp int;
precision highp sampler2D;

uniform sampler2D uCells;
uniform int uDevP, uHeight, uShift;  // uShift: the canvas top's offset into its first cell row
out vec4 colour;

void main() {
  ivec2 p = ivec2(gl_FragCoord.xy);
  int y = uHeight - 1 - p.y + uShift;  // device px down from the first cell row's top
  colour = texelFetch(uCells, ivec2(p.x / uDevP, y / uDevP), 0);
}`
