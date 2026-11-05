/**
 * The wallpaper as a WebGL 2 shader: computeWallpaper() (tauler-wallpaper.js)
 * on the GPU, for one horizontal slice of the page at a time, and the one
 * place the page's background becomes pixel art: a room layer (the home
 * page's path-traced room, whiteroom.ts) is composited here in continuous
 * colour and quantized with the wallpaper, once. The JS module stays the
 * reference: for the same options, colours, device pixel ratio and page
 * position, and no room, this draws its pixels (tests/wallpaper.spec.ts
 * compares them).
 *
 * Two passes:
 *
 * 1. Cells: one fragment per dither pixel ("cell"), into an RGBA8 target the
 *    slice's size in cells. It runs computeWallpaper()'s inner loop up to the
 *    continuous colour: hue and fade from two lattices with eased 4-corner
 *    means and the per-cell noise hash, giving each OKLCH channel as a
 *    number of grid steps from the background. Where a room layer covers the
 *    cell, the room's colour takes over, channel by channel (see Room
 *    below). Then the one quantize: one Bayer threshold for every channel,
 *    level = floor(value + threshold), and the levels pick the colour from
 *    a palette texture. Room cells take hue on a finer grid than the
 *    wallpaper's (ROOM_HUE_SUB), with the same threshold.
 * 2. Present: every device pixel of the canvas copies its cell, by integer
 *    division, so a cell is exactly devP × devP device pixels on the page's
 *    grid however the slice's top falls.
 *
 * What the CPU prepares instead of the shader, per draw (all small):
 *
 * - The palette: every grid point the slice can reach, the wallpaper's and
 *   the room's (any lightness, chroma to 0.4, ROOM_HUE_SUB hue columns per
 *   wallpaper hue level), converted to sRGB by the
 *   module's own oklchToRgb() (gamut mapping included), so a colour is the
 *   module's to the byte; the shader only computes levels.
 * - The hue lattice: a corner never equals its left, upper or upper-left
 *   neighbour, a dynamic-programming table filled from the page's top-left
 *   (the module's distinctLattice()). It is filled on the CPU for the corners
 *   the slice touches and uploaded; the wash lattice goes up with it.
 * - Each column's and row's lattice corner and eased weight, computed in
 *   double precision as the module does and uploaded as floats. The shader
 *   would otherwise divide page coordinates (tens of thousands of px deep in
 *   a long page) by the lattice spacing in single precision.
 *
 * The per-cell noise is the module's integer hash in `uint` arithmetic,
 * exact. What is left in single precision is the mixing of corners and the
 * final divisions, where the module works in doubles (rounding to floats at
 * fixed points). A cell could land on the other side of a level boundary
 * only if its value were within about 1e-7 of it: none did in testing (on
 * SwiftShader and an NVIDIA GPU, millions of cells, three device pixel
 * ratios, both themes, deep into long pages), so the tests ask for every
 * byte. If a GPU ever differs, it is by one level on a rare cell.
 *
 * The room: an image of continuous colour (display sRGB, one texel per
 * cell) over a band of the page's rows, the full width. Each OKLCH channel
 * is the wallpaper's at the band's top and bottom edges and eases to the
 * room's over its own share of the room's height (RoomCells.fade), along a
 * smootherstep; the hue takes the shorter way round, and a grey room cell
 * takes the wallpaper's. The fades start along a wave (RoomCells.wave,
 * wave() in CELLS): a sum of sines along the page's x, each with a seeded
 * phase and warped by smooth seeded noise, so no straight line marks where
 * the room begins. Where every
 * channel is the wallpaper's the cell is the wallpaper's own computation,
 * byte for byte, so the room's edges are the wallpaper by construction.
 * The room also fades in over time (RoomCells.presence, a weight multiplied
 * into every channel's): before its first sample it is not drawn at all.
 */
import {
  EASE,
  THRESHOLDS,
  U32,
  distinctLattice,
  hash3,
  oklchToRgb,
  rgbToOklab,
} from '@/lib/wallpaper/tauler-wallpaper.js'
import { WALLPAPER, ditherPixel, type WallpaperColours } from './options'

/** A slice of the page: what one canvas shows. */
export interface Slice {
  /** The canvas's top edge, in device px from the page's top. */
  top: number
  /** The canvas's size in device px. */
  width: number
  height: number
  dpr: number
}

/** The share of the room's height over which each OKLCH channel eases. */
export interface Fade {
  L: number
  C: number
  h: number
}

/** A fade's starting line along the page's x: see ROOM_WAVE. */
export interface Wave {
  octaves: number
  /** CSS px */
  wavelength: number
  lacunarity: number
  persistence: number
  /** turns */
  phaseWarp: number
  ampWarp: number
  /** share of the room's height, peak to peak */
  depth: number
  /** share of the depth above the room's top (the bottom fade's wave starts at the box's bottom) */
  rise: number
}

/**
 * A room layer for pass 1: an image of continuous colour on the page's
 * cells, which the wallpaper fades into at its top and bottom.
 */
export interface RoomCells {
  /** One texel per cell, display sRGB; texel row 0 is the band's last row. */
  texture: WebGLTexture
  /** The page's cell rows the image covers, first and last. */
  rows: [number, number]
  /**
   * The first page cell row the room reaches: the top fade's wave rises
   * above the image. Rows above the image take its first row's wide mean
   * (see `detail`).
   */
  from: number
  /** Its width in cells, from the page's left edge. */
  cols: number
  /** Device px from the page's top: where the room's top fade starts (a row's top). */
  top: number
  /** Device px from the page's top: where its bottom fade starts. */
  bottom: number
  /** Device px: the height the fades' shares are of. */
  height: number
  fade: Fade
  /** The wave each fade starts along (ROOM_WAVE in whiteroom/room-size.ts). */
  wave: Wave
  /** How far in from each edge (share of height) the room's mean is pulled to the wallpaper's. */
  match: number
  /** How far below its top (share of height) the room's detail eases in over its wide mean. */
  detail: number
  /**
   * How much of the room shows, 0..1 per OKLCH channel, multiplied into that
   * channel's fade: it rises as the image gathers samples (ROOM_EASE_IN). At
   * 0 in every channel the room is not drawn at all, so every cell is the
   * wallpaper's own.
   */
  presence: Fade
}

export interface SliceRenderer {
  readonly kind: 'webgl2' | '2d'
  draw(slice: Slice, colours: WallpaperColours, room?: RoomCells | null): void
  destroy(): void
}

export interface GlSliceRenderer extends SliceRenderer {
  readonly kind: 'webgl2'
  /** The context, which a room layer renders its image in. */
  readonly gl: WebGL2RenderingContext
}

const TAU = 2 * Math.PI

/** The highest chroma a room colour keeps a level for (no sRGB colour goes past 0.37). */
const ROOM_MAX_CHROMA = 0.4

/**
 * How many times finer than the wallpaper's the room's hue grid is: 4, so
 * 15° steps where the wallpaper's are 60° (WALLPAPER.stepH).
 *
 * On the wallpaper's own grid the dither spreads a room colour over the two
 * hue levels around it, up to 60° away: a green leaf between 120° and
 * 180° drew cyan specks, and the tan rug and floor pink and yellow ones. The
 * wallpaper's colours are near grey, where hue barely shows, so its grid
 * was never chosen with saturated colours in mind. On the finer grid a room
 * cell's hue is within one 15° step of its colour's, and the dither still
 * blends the two levels around it, so no gradient bands.
 *
 * The palette holds both grids at once: column h is hue h × sHq /
 * ROOM_HUE_SUB, so the wallpaper's level q is column q × ROOM_HUE_SUB, the
 * very colour it was before. Wallpaper cells are unchanged to the byte, and
 * a room cell fading into the wallpaper picks from a superset of its
 * colours. (ADR 0047, "the room's hue grid is finer".)
 */
export const ROOM_HUE_SUB = 4

/** The quantization grid and lattices for one set of colours. */
export function wallpaperGrid(colours: WallpaperColours) {
  const o = WALLPAPER
  const lab = rgbToOklab(colours.background)
  const bgL = lab[0],
    bgC = Math.hypot(lab[1], lab[2])
  const sL = Math.max(0.002, o.stepL),
    sC = Math.max(0.001, o.stepC)
  const nHq = Math.max(
    1,
    Math.round(TAU / ((Math.max(0.5, o.stepH) * Math.PI) / 180)),
  )
  const sHq = TAU / nHq
  const intensity = Math.max(0, Math.min(1, o.intensity))
  const dL = colours.ringL - bgL,
    dC = colours.ringC - bgC
  const seed = o.seed | 0
  const nCell = Math.max(2, o.cellHues | 0)
  const washMax = Math.max(0, Math.min(1, o.wash))
  // The levels the module keeps colours for (its colour cache's range).
  const qL0 = Math.floor(Math.min(0, dL * intensity) / sL) - 2
  const nqL = Math.ceil((Math.abs(dL) * intensity) / sL) + 5
  const nqC = Math.ceil((Math.max(0, dC) * intensity) / sC) + 4
  // The levels a room colour can take: lightness inside 0..1, chroma to
  // ROOM_MAX_CHROMA (tests/docs.spec.ts builds the same grid).
  const roomL = [Math.ceil(-bgL / sL), Math.floor((1 - bgL) / sL)]
  const roomC = Math.floor(ROOM_MAX_CHROMA / sC + 1e-9)
  // Palette: x = the room's hue level (the wallpaper's level q is column
  // q * ROOM_HUE_SUB), y = (lightness level - palL0) * palNC + chroma level.
  const palL0 = Math.min(qL0, roomL[0])
  const palNL = Math.max(qL0 + nqL - 1, roomL[1]) - palL0 + 1
  const palNC = Math.max(nqC, roomC + 1)
  const palNH = nHq * ROOM_HUE_SUB
  const palette = new Uint8Array(palNH * palNL * palNC * 4)
  for (let l = 0; l < palNL; l++)
    for (let c = 0; c < palNC; c++)
      for (let h = 0; h < palNH; h++) {
        const qL = palL0 + l
        // the background level is the background colour itself (see the module)
        const rgb =
          qL === 0 && c === 0
            ? colours.background
            : oklchToRgb(
                bgL + qL * sL,
                Math.max(0, bgC + c * sC),
                (h * sHq) / ROOM_HUE_SUB,
              )
        palette.set([...rgb, 255], ((l * palNC + c) * palNH + h) * 4)
      }
  return {
    bgL,
    bgC,
    sL,
    sC,
    sHq,
    nHq,
    palNH,
    dL,
    dC,
    intensity,
    wallL: [qL0, qL0 + nqL - 1],
    wallC: nqC - 1,
    roomL,
    roomC,
    palL0,
    palNL,
    palNC,
    palette,
    hueAmp: Math.max(0, o.noise) * 2 * sHq,
    biasAmp:
      Math.abs(dL) > 1e-6 ? (Math.max(0, o.bias) * 2 * sL) / Math.abs(dL) : 0,
    seedHueNoise: seed + 1,
    seedBias: seed + 3,
    seedEdge: seed + 4,
    hue: distinctLattice(
      seed,
      Array.from({ length: nCell }, (_, k) => (k / nCell) * TAU),
    ),
    wash: (i: number, j: number) => {
      const v = hash3(seed + 2, i, j) * U32
      return washMax * v * v
    },
  }
}
type Grid = ReturnType<typeof wallpaperGrid>

/**
 * Each column's (or row's) lattice corner and eased weight, for both
 * lattices, as computeWallpaper() works them out: [hue corner - first, hue
 * weight, wash corner - first, wash weight] per cell, and the first and last
 * corner of each lattice.
 */
function axis(first: number, count: number, pc: number) {
  const ease = EASE[WALLPAPER.ease] ?? EASE.smooth
  const cellH = Math.max(1, WALLPAPER.cell),
    cellW = Math.max(1, WALLPAPER.washSize)
  const out = new Float32Array(count * 4)
  const hI = new Int32Array(count),
    wI = new Int32Array(count)
  for (let k = 0; k < count; k++) {
    const x = (first + k + 0.5) * pc
    let u = x / cellH,
      i = Math.floor(u)
    hI[k] = i
    out[k * 4 + 1] = ease(u - i)
    u = x / cellW
    i = Math.floor(u)
    wI[k] = i
    out[k * 4 + 3] = ease(u - i)
  }
  for (let k = 0; k < count; k++) {
    out[k * 4] = hI[k] - hI[0]
    out[k * 4 + 2] = wI[k] - wI[0]
  }
  return {
    table: out,
    hue: [hI[0], hI[count - 1] + 1],
    wash: [wI[0], wI[count - 1] + 1],
  }
}

const VERTEX = `#version 300 es
void main() {
  gl_Position = vec4(gl_VertexID == 1 ? 3.0 : -1.0, gl_VertexID == 2 ? 3.0 : -1.0, 0.0, 1.0);
}`

/** Pass 1: one fragment per cell; computeWallpaper()'s inner loop, and the room. */
const CELLS = `#version 300 es
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
const SAMPLERS = [
  ['cols', 'uCols'],
  ['rows', 'uRows'],
  ['hue', 'uHue'],
  ['wash', 'uWash'],
  ['threshold', 'uThreshold'],
  ['palette', 'uPalette'],
] as const

/** Pass 2: every device pixel copies its cell. */
const PRESENT = `#version 300 es
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

/** True when the WebGL 2 context is software (SwiftShader, llvmpipe, ...). */
export function isSoftware(gl: WebGL2RenderingContext) {
  const debug = gl.getExtension('WEBGL_debug_renderer_info')
  const name = String(
    gl.getParameter(debug ? debug.UNMASKED_RENDERER_WEBGL : gl.RENDERER),
  )
  return /swiftshader|llvmpipe|softpipe|lavapipe|software|basic render driver/i.test(
    name,
  )
}

/**
 * A renderer drawing into `canvas` with WebGL 2, or null where there is none,
 * where it is software and `software` is false, or where its shaders do not
 * compile. `canvas` cannot take a 2D context after this, even on null: give
 * the fallback a fresh one.
 *
 * The context may be shared with a room layer that renders with three.js
 * (whiteroom.ts): draw() sets every piece of GL state it relies on, and the
 * room resets three.js's view of the state before it renders.
 */
export function createGlRenderer(
  canvas: HTMLCanvasElement,
  {
    software = false,
    powerPreference = 'low-power',
  }: { software?: boolean; powerPreference?: WebGLPowerPreference } = {},
): GlSliceRenderer | null {
  const gl = canvas.getContext('webgl2', {
    alpha: false,
    antialias: false,
    depth: false,
    stencil: false,
    preserveDrawingBuffer: false,
    powerPreference,
  })
  if (!gl) return null
  if (!software && isSoftware(gl)) {
    gl.getExtension('WEBGL_lose_context')?.loseContext()
    return null
  }

  function program(fragment: string) {
    const p = gl!.createProgram()
    for (const [type, source] of [
      [gl!.VERTEX_SHADER, VERTEX],
      [gl!.FRAGMENT_SHADER, fragment],
    ] as const) {
      const s = gl!.createShader(type)!
      gl!.shaderSource(s, source)
      gl!.compileShader(s)
      gl!.attachShader(p, s)
      gl!.deleteShader(s)
    }
    gl!.linkProgram(p)
    if (!gl!.getProgramParameter(p, gl!.LINK_STATUS)) {
      console.warn('tauler: wallpaper shader:', gl!.getProgramInfoLog(p))
      return null
    }
    const uniforms = new Map<string, WebGLUniformLocation | null>()
    return {
      p,
      at: (name: string) => {
        if (!uniforms.has(name))
          uniforms.set(name, gl!.getUniformLocation(p, name))
        return uniforms.get(name)!
      },
    }
  }
  const cells = program(CELLS),
    present = program(PRESENT)
  if (!cells || !present) {
    gl.getExtension('WEBGL_lose_context')?.loseContext()
    return null
  }
  // no attributes (the triangle comes from gl_VertexID), but a VAO of our
  // own, so a room's three.js buffers are never drawn from
  const vertices = gl.createVertexArray()

  /** The state draw() and upload() rely on, whatever ran in the context before. */
  function resetState() {
    gl!.bindVertexArray(vertices)
    for (const cap of [
      gl!.BLEND,
      gl!.CULL_FACE,
      gl!.DEPTH_TEST,
      gl!.STENCIL_TEST,
      gl!.SCISSOR_TEST,
      gl!.DITHER,
      gl!.RASTERIZER_DISCARD,
      gl!.POLYGON_OFFSET_FILL,
      gl!.SAMPLE_ALPHA_TO_COVERAGE,
      gl!.SAMPLE_COVERAGE,
    ])
      gl!.disable(cap)
    gl!.colorMask(true, true, true, true)
    gl!.bindBuffer(gl!.PIXEL_UNPACK_BUFFER, null)
    gl!.pixelStorei(gl!.UNPACK_ALIGNMENT, 1)
    gl!.pixelStorei(gl!.UNPACK_FLIP_Y_WEBGL, false)
    gl!.pixelStorei(gl!.UNPACK_PREMULTIPLY_ALPHA_WEBGL, false)
    for (const p of [
      gl!.UNPACK_ROW_LENGTH,
      gl!.UNPACK_IMAGE_HEIGHT,
      gl!.UNPACK_SKIP_PIXELS,
      gl!.UNPACK_SKIP_ROWS,
      gl!.UNPACK_SKIP_IMAGES,
    ])
      gl!.pixelStorei(p, 0)
  }

  const textures = new Map<string, WebGLTexture>()
  /** (Re)fills the named texture; float formats for tables and lattices. */
  function upload(
    name: string,
    width: number,
    height: number,
    format: 'r32f' | 'rgba32f' | 'rgba8',
    data: ArrayBufferView | null,
  ) {
    let texture = textures.get(name)
    if (!texture) {
      texture = gl!.createTexture()!
      textures.set(name, texture)
    }
    gl!.activeTexture(gl!.TEXTURE0)
    gl!.bindTexture(gl!.TEXTURE_2D, texture)
    for (const parameter of [gl!.TEXTURE_MIN_FILTER, gl!.TEXTURE_MAG_FILTER])
      gl!.texParameteri(gl!.TEXTURE_2D, parameter, gl!.NEAREST)
    const [internal, layout, type] = {
      r32f: [gl!.R32F, gl!.RED, gl!.FLOAT],
      rgba32f: [gl!.RGBA32F, gl!.RGBA, gl!.FLOAT],
      rgba8: [gl!.RGBA8, gl!.RGBA, gl!.UNSIGNED_BYTE],
    }[format]
    gl!.texImage2D(
      gl!.TEXTURE_2D,
      0,
      internal,
      width,
      height,
      0,
      layout,
      type,
      data,
    )
    return texture
  }
  /** A per-cell table, wrapped 1024 entries to a row (see table() in CELLS). */
  function uploadTable(name: string, table: Float32Array) {
    const n = table.length / 4
    const width = Math.min(n, 1024),
      height = Math.ceil(n / 1024)
    const padded = new Float32Array(width * height * 4)
    padded.set(table)
    upload(name, width, height, 'rgba32f', padded)
  }
  function uploadLattice(
    name: string,
    lattice: (i: number, j: number) => number,
    [i0, i1]: number[],
    [j0, j1]: number[],
  ) {
    const width = i1 - i0 + 1,
      height = j1 - j0 + 1
    const values = new Float32Array(width * height)
    for (let j = 0; j < height; j++)
      for (let i = 0; i < width; i++)
        values[j * width + i] = lattice(i0 + i, j0 + j)
    upload(name, width, height, 'r32f', values)
  }

  resetState()
  const threshold = THRESHOLDS[WALLPAPER.matrix] ?? THRESHOLDS.bayer8
  upload('threshold', 8, 8, 'r32f', threshold)
  const framebuffer = gl.createFramebuffer()

  let grid: Grid | null = null,
    gridKey = ''
  let columns = { key: '', hue: [0, 0], wash: [0, 0] }
  let cellsSize = ''

  return {
    kind: 'webgl2',
    gl,
    draw(slice, colours, room) {
      resetState()
      const key = JSON.stringify(colours)
      if (key !== gridKey) {
        grid = wallpaperGrid(colours)
        gridKey = key
        upload(
          'palette',
          grid.palNH,
          grid.palNL * grid.palNC,
          'rgba8',
          grid.palette,
        )
      }
      const g = grid!
      const devP = ditherPixel(slice.dpr),
        pc = devP / slice.dpr
      const row0 = Math.floor(slice.top / devP)
      const shift = slice.top - row0 * devP
      const cols = Math.ceil(slice.width / devP),
        rows = Math.ceil((slice.height + shift) / devP)
      if (
        gl.drawingBufferWidth !== slice.width ||
        gl.drawingBufferHeight !== slice.height
      )
        throw new Error('the canvas is larger than WebGL can draw')

      const columnsKey = `${cols} ${slice.dpr}`
      if (columns.key !== columnsKey) {
        const x = axis(0, cols, pc)
        uploadTable('cols', x.table)
        columns = { key: columnsKey, hue: x.hue, wash: x.wash }
      }
      const y = axis(row0, rows, pc)
      uploadTable('rows', y.table)
      uploadLattice('hue', g.hue, columns.hue, y.hue)
      uploadLattice('wash', g.wash, columns.wash, y.wash)

      // pass 1: cells
      const target =
        cellsSize === `${cols} ${rows}`
          ? textures.get('cells')!
          : upload('cells', cols, rows, 'rgba8', null)
      cellsSize = `${cols} ${rows}`
      gl.bindFramebuffer(gl.FRAMEBUFFER, framebuffer)
      gl.framebufferTexture2D(
        gl.FRAMEBUFFER,
        gl.COLOR_ATTACHMENT0,
        gl.TEXTURE_2D,
        target,
        0,
      )
      gl.viewport(0, 0, cols, rows)
      gl.useProgram(cells.p)
      const u = cells.at
      SAMPLERS.forEach(([name, uniform], unit) => {
        gl.activeTexture(gl.TEXTURE0 + unit)
        gl.bindTexture(gl.TEXTURE_2D, textures.get(name)!)
        gl.uniform1i(u(uniform), unit)
      })
      gl.uniform2i(u('uOrigin'), 0, row0)
      gl.uniform1ui(u('uSeedHueNoise'), g.seedHueNoise)
      gl.uniform1ui(u('uSeedBias'), g.seedBias)
      gl.uniform1ui(u('uSeedEdge'), g.seedEdge)
      gl.uniform1f(u('uHueAmp'), g.hueAmp)
      gl.uniform1f(u('uBiasAmp'), g.biasAmp)
      gl.uniform1f(u('uIntensity'), g.intensity)
      gl.uniform1f(u('uDL'), g.dL)
      gl.uniform1f(u('uDC'), g.dC)
      gl.uniform1f(u('uSL'), g.sL)
      gl.uniform1f(u('uSC'), g.sC)
      gl.uniform1f(u('uSH'), g.sHq)
      gl.uniform1i(u('uNH'), g.nHq)
      gl.uniform1i(u('uPalL0'), g.palL0)
      gl.uniform1i(u('uPalNC'), g.palNC)
      gl.uniform2i(u('uWallL'), g.wallL[0], g.wallL[1])
      gl.uniform1i(u('uWallC'), g.wallC)
      // a room with nothing to show yet is not drawn: the wallpaper's own cells
      const showRoom =
        room && room.presence.L + room.presence.C + room.presence.h > 0
      gl.uniform1i(u('uRoom'), showRoom ? 1 : 0)
      if (showRoom) {
        const unit = SAMPLERS.length
        gl.activeTexture(gl.TEXTURE0 + unit)
        gl.bindTexture(gl.TEXTURE_2D, room.texture)
        gl.uniform1i(u('uRoomImage'), unit)
        gl.uniform3i(u('uRoomCells'), room.cols, room.rows[0], room.rows[1])
        gl.uniform1i(u('uRoomFrom'), room.from)
        gl.uniform3f(u('uRoomEdges'), room.top, room.bottom, room.height)
        gl.uniform3f(u('uFade'), room.fade.L, room.fade.C, room.fade.h)
        const w = room.wave
        gl.uniform4f(
          u('uWave'),
          w.depth,
          w.wavelength,
          w.lacunarity,
          w.persistence,
        )
        gl.uniform3f(u('uWarp'), w.phaseWarp, w.ampWarp, w.rise)
        gl.uniform1i(u('uOctaves'), w.octaves)
        gl.uniform1f(u('uPc'), pc)
        gl.uniform1f(u('uMatch'), room.match)
        gl.uniform1f(u('uDetail'), room.detail)
        gl.uniform3f(
          u('uPresence'),
          room.presence.L,
          room.presence.C,
          room.presence.h,
        )
        gl.uniform2i(u('uRoomL'), g.roomL[0], g.roomL[1])
        gl.uniform1i(u('uRoomC'), g.roomC)
        gl.uniform2f(u('uBg'), g.bgL, g.bgC)
        gl.uniform1i(u('uDevP'), devP)
      }
      gl.drawArrays(gl.TRIANGLES, 0, 3)

      // pass 2: present
      gl.bindFramebuffer(gl.FRAMEBUFFER, null)
      gl.viewport(0, 0, slice.width, slice.height)
      gl.useProgram(present.p)
      gl.activeTexture(gl.TEXTURE0)
      gl.bindTexture(gl.TEXTURE_2D, target)
      gl.uniform1i(present.at('uCells'), 0)
      gl.uniform1i(present.at('uDevP'), devP)
      gl.uniform1i(present.at('uHeight'), slice.height)
      gl.uniform1i(present.at('uShift'), shift)
      gl.drawArrays(gl.TRIANGLES, 0, 3)
    },
    destroy() {
      for (const texture of textures.values()) gl.deleteTexture(texture)
      gl.deleteFramebuffer(framebuffer)
      gl.deleteVertexArray(vertices)
      gl.deleteProgram(cells.p)
      gl.deleteProgram(present.p)
      gl.getExtension('WEBGL_lose_context')?.loseContext()
    },
  }
}
