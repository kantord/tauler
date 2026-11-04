/**
 * tauler-wallpaper — a dithered, e-ink-coloured background for the tauler site.
 *
 * A dependency-free ES module (no build step). It draws one <canvas> behind a page's
 * content: soft colour zones that fade toward the page background, rendered as crisp
 * pixel art with an ordered (Bayer) dither. Every pixel is opaque and lands exactly on the
 * device-pixel grid, at any zoom level.
 *
 * Quick start
 *   import { mountWallpaper } from './tauler-wallpaper.js';
 *   const wp = mountWallpaper(document.querySelector('.page'));   // host: position: relative
 *   // later: wp.update({ seed: 7 }); wp.destroy();
 *
 * Colours come from CSS custom properties on the host (or any ancestor):
 *   --wallpaper-bg       page background, any CSS colour (oklch() recommended)
 *   --wallpaper-ring-l   OKLCH lightness of the hue ring, 0..1 or %
 *   --wallpaper-ring-c   OKLCH chroma of the hue ring, e.g. 0.12
 * A theme switch is just a change to these variables; the wallpaper picks it up.
 *
 * Pipeline (see README.md for the full explanation and the design decisions):
 *   1. hue    = 4-corner mean of a lattice of cell hues (+ tiny per-pixel noise)
 *   2. amount = intensity × (1 − 4-corner mean of a random wash lattice) (+ tiny noise)
 *   3. colour = OKLCH(bg.L + (ringL − bg.L)·amount, bg.C + (ringC − bg.C)·amount, hue)
 *   4. pixel  = each OKLCH channel snapped to its own equal grid (measured from the
 *               background) with an ordered dither: level = floor(value / step + bayer(x, y))
 *
 * [tauler] Changes from the designer's file are marked [tauler]: a slice of the page can be
 * computed on its own (col0 / row0), the background level is the background colour itself,
 * and a few internals are exported for the WebGL wallpaper (wallpaper-gl.ts).
 *
 * Colour maths: OKLab / OKLCH by Björn Ottosson (public domain / MIT).
 * Output is deterministic: the same options and size always give the same pixels, and
 * resizing never changes pixels that were already visible.
 */

const TAU = 2 * Math.PI;

/* ────────────────────────────────────────────────────────────────────────────────────
   Options
   ──────────────────────────────────────────────────────────────────────────────────── */

/**
 * Default options: the tuned look. Every option can be overridden per call.
 * Colours (background, ring lightness / chroma) normally come from the CSS variables;
 * the `FALLBACK` values below are only used when those variables are missing.
 */
export const DEFAULTS = Object.freeze({
  mode: 'light',        // which FALLBACK colours to use when the CSS variables are missing
  pixelSize: 3,         // size of one dither pixel, in pixelUnit units
  pixelUnit: 'device',  // 'device': pixelSize counts physical device pixels, so the dither keeps
                        //   the same physical size and grid at every browser zoom level
                        // 'css': pixelSize counts CSS px (rounded to whole device px), so the
                        //   dither scales with zoom
  matrix: 'bayer8',     // dither pattern: 'bayer8' | 'bayer4' | 'checker' (2×2)

  // continuous colour
  intensity: 0.1,       // how far colours go from the background toward the ring (0..1)
  cellHues: 3,          // number of hues on the corner lattice (evenly spaced on the ring)
  cell: 420,            // corner spacing of the hue lattice, CSS px
  ease: 'smooth',       // corner weighting: 'smooth' (smoothstep) | 'linear'
  wash: 1,              // strongest extra fade toward the background (0..1)
  washSize: 300,        // corner spacing of the wash lattice, CSS px
  noise: 0.1,           // per-pixel random hue nudge, in hue steps
  bias: 0.05,           // per-pixel random nudge of the fade amount, in lightness steps

  // quantization grid: one per OKLCH channel, equal steps, measured from the background
  stepL: 0.035,         // lightness step
  stepC: 0.04,          // chroma step
  stepH: 60,            // hue step, degrees (rounded so a whole number fits round the ring)

  seed: 18,             // any integer; same seed → same wallpaper
});

/** Colours used when the CSS variables are absent (sRGB background, ring L / C). */
export const FALLBACK = Object.freeze({
  light: { background: [248, 245, 238], ringL: 0.66, ringC: 0.12 },
  dark:  { background: [18, 17, 15],    ringL: 0.72, ringC: 0.11 },
});

/** Names of the CSS custom properties read from the host element. */
export const CSS_VARS = Object.freeze({
  background: '--wallpaper-bg',
  ringL: '--wallpaper-ring-l',
  ringC: '--wallpaper-ring-c',
});

/* ────────────────────────────────────────────────────────────────────────────────────
   Colour maths: sRGB ↔ OKLab ↔ OKLCH (Björn Ottosson's reference formulas)
   ──────────────────────────────────────────────────────────────────────────────────── */

const toLinear = (c) => { c /= 255; return c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4); };
const fromLinear = (c) => 255 * (c <= 0.0031308 ? 12.92 * c : 1.055 * Math.pow(c, 1 / 2.4) - 0.055);

/** sRGB [0..255]×3 → OKLab [L, a, b]. */
export function rgbToOklab([R, G, B]) {
  const r = toLinear(R), g = toLinear(G), b = toLinear(B);
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
  return [0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
          1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
          0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s];
}

function oklabToLinear(L, a, b) {
  const l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3;
  const m = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3;
  const s = (L - 0.0894841775 * a - 1.2914855480 * b) ** 3;
  return [ 4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
          -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
          -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s];
}

const inGamut = (v) => v[0] >= -1e-4 && v[0] <= 1 + 1e-4 && v[1] >= -1e-4 && v[1] <= 1 + 1e-4 && v[2] >= -1e-4 && v[2] <= 1 + 1e-4;

/**
 * OKLCH → sRGB [0..255]×3. Colours outside the sRGB gamut keep their lightness and hue
 * and lose chroma (binary search) instead of being clipped per channel, which would
 * shift the hue.
 */
export function oklchToRgb(L, C, h) {
  const ca = Math.cos(h), sa = Math.sin(h);
  let lin = oklabToLinear(L, C * ca, C * sa);
  if (!inGamut(lin)) {
    let lo = 0, hi = C;
    for (let i = 0; i < 20; i++) {
      const mid = (lo + hi) / 2;
      if (inGamut(oklabToLinear(L, mid * ca, mid * sa))) lo = mid; else hi = mid;
    }
    lin = oklabToLinear(L, lo * ca, lo * sa);
  }
  return lin.map((x) => Math.round(Math.min(255, Math.max(0, fromLinear(Math.min(1, Math.max(0, x)))))));
}

// One 32-bit RGBA value per pixel, in the byte order of the platform (ImageData is RGBA bytes)
const LITTLE_ENDIAN = new Uint8Array(new Uint32Array([1]).buffer)[0] === 1;
const pack = ([r, g, b]) => (LITTLE_ENDIAN ? ((255 << 24) | (b << 16) | (g << 8) | r) >>> 0
                                           : ((r << 24) | (g << 16) | (b << 8) | 255) >>> 0);

/* ────────────────────────────────────────────────────────────────────────────────────
   Randomness: a seeded integer hash (deterministic, no state)
   ──────────────────────────────────────────────────────────────────────────────────── */

export function hash3(seed, i, j) {
  let h = Math.imul(i | 0, 0x27d4eb2d) ^ Math.imul(j | 0, 0x165667b1) ^ Math.imul(seed | 0, 0x9E3779B1);
  h = Math.imul(h ^ (h >>> 15), 0x85ebca6b);
  h = Math.imul(h ^ (h >>> 13), 0xc2b2ae35);
  return (h ^ (h >>> 16)) >>> 0;
}
export const U32 = 1 / 4294967296;   // hash → 0..1

/* ────────────────────────────────────────────────────────────────────────────────────
   Lattices: values on whole-number grid corners (i, j)
   ──────────────────────────────────────────────────────────────────────────────────── */

/**
 * Hue lattice: a seeded pick from `values` per corner, never equal to its left, upper or
 * upper-left neighbour (so neighbouring corners always differ in hue). Each corner depends
 * on those three neighbours, so this is a small dynamic-programming table, filled on
 * demand and memoized. Corners further than 2 outside the page have no constraint.
 */
export function distinctLattice(seed, values) {
  const memo = new Map(), n = values.length;
  const idx = (i, j) => {
    if (i < -2 || j < -2) return hash3(seed, i, j) % n;
    const key = (i + 4096) * 8192 + (j + 4096);
    let v = memo.get(key);
    if (v !== undefined) return v;
    const left = idx(i - 1, j), up = idx(i, j - 1), ul = idx(i - 1, j - 1);
    v = hash3(seed, i, j) % n;
    for (let t = 0; t < n && (v === left || v === up || v === ul); t++) v = (v + 1) % n;
    memo.set(key, v);
    return v;
  };
  return (i, j) => values[idx(i, j)];
}

/* ────────────────────────────────────────────────────────────────────────────────────
   Dither thresholds, as 8×8 tables (smaller matrices are tiled): t = (m + 0.5) / n
   ──────────────────────────────────────────────────────────────────────────────────── */

const BAYER4 = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];
const BAYER8 = [
   0, 32,  8, 40,  2, 34, 10, 42, 48, 16, 56, 24, 50, 18, 58, 26,
  12, 44,  4, 36, 14, 46,  6, 38, 60, 28, 52, 20, 62, 30, 54, 22,
   3, 35, 11, 43,  1, 33,  9, 41, 51, 19, 59, 27, 49, 17, 57, 25,
  15, 47,  7, 39, 13, 45,  5, 37, 63, 31, 55, 23, 61, 29, 53, 21];
export const THRESHOLDS = {
  bayer8: Float32Array.from({ length: 64 }, (_, i) => (BAYER8[i] + 0.5) / 64),
  bayer4: Float32Array.from({ length: 64 }, (_, i) => (BAYER4[((i >> 3) & 3) * 4 + (i & 3)] + 0.5) / 16),
  checker: Float32Array.from({ length: 64 }, (_, i) => ((((i >> 3) + i) & 1) + 0.5) / 2),
};

export const EASE = { linear: (t) => t, smooth: (t) => t * t * (3 - 2 * t) };
const f32 = Math.fround;   // values pass through 32-bit floats at fixed points: keeps output identical to the reference prototype

/* ────────────────────────────────────────────────────────────────────────────────────
   computeWallpaper: the pure computation (no DOM)
   ──────────────────────────────────────────────────────────────────────────────────── */

// Browser canvas limits (Chrome/Firefox/Safari): ~32767 px per side, ~268M px in area.
const MAX_SIDE = 32767, MAX_AREA = 268000000;

/**
 * Compute the wallpaper at dither-pixel resolution.
 *
 * @param {object} options  DEFAULTS overrides plus:
 *   width, height   size in CSS px (required)
 *   dpr             device pixel ratio (default 1)
 *   deviceWidth, deviceHeight   exact size in device px, if measured (preferred)
 *   col0, row0      [tauler] the area's top-left, in whole dither pixels from the page's
 *                   top-left (default 0, 0): the pixels are the page's from there on, so a
 *                   slice of the page computes exactly what the whole page would show there
 *   background      sRGB [r, g, b] (default: FALLBACK[mode])
 *   ringL, ringC    hue-ring OKLCH lightness / chroma (default: FALLBACK[mode])
 * @param {Uint32Array} [reuse]  optional buffer to write into (reused between renders)
 * @returns {{data: Uint8ClampedArray, pixels: Uint32Array, cols: number, rows: number,
 *            devW: number, devH: number, devP: number, dpr: number, pixelCss: number,
 *            colours: number, grid: {L: number, C: number, H: number}, clipped: boolean}}
 */
export function computeWallpaper(options, reuse) {
  const o = Object.assign({}, DEFAULTS, options);
  const fb = FALLBACK[o.mode] || FALLBACK.light;
  const bgLab = rgbToOklab(o.background || fb.background);
  const bgL = bgLab[0], bgC = Math.hypot(bgLab[1], bgLab[2]);
  const ringL = o.ringL != null ? o.ringL : fb.ringL, ringC = o.ringC != null ? o.ringC : fb.ringC;

  // Pixel geometry. One dither pixel = devP whole device pixels (never fractional), which
  // is what keeps every square identical at fractional zoom levels. With pixelUnit
  // 'device' devP is fixed, so zooming changes the layout (cells, fades) but never the
  // physical size of the dither pixels.
  const dpr = o.dpr > 0 ? o.dpr : 1;
  const devP = o.pixelUnit === 'css'
    ? Math.max(1, Math.round(Math.max(1, Math.round(o.pixelSize)) * dpr))
    : Math.max(1, Math.round(o.pixelSize));
  const pc = devP / dpr;                                         // one dither pixel in CSS px
  let devW = o.deviceWidth || Math.round(Math.max(1, o.width) * dpr);
  let devH = o.deviceHeight || Math.round(Math.max(1, o.height) * dpr);
  let clipped = false;
  if (devW > MAX_SIDE) { devW = MAX_SIDE; clipped = true; }
  if (devH > MAX_SIDE || devW * devH > MAX_AREA) { devH = Math.min(MAX_SIDE, Math.floor(MAX_AREA / devW)); clipped = true; }
  const cols = Math.max(1, Math.ceil(devW / devP)), rows = Math.max(1, Math.ceil(devH / devP));
  const col0 = o.col0 | 0, row0 = o.row0 | 0;   // [tauler] the area's offset on the page, in dither pixels

  // Quantization grid
  const sL = Math.max(0.002, o.stepL), sC = Math.max(0.001, o.stepC);
  const nHq = Math.max(1, Math.round(TAU / (Math.max(0.5, o.stepH) * Math.PI / 180)));   // whole hue steps round the ring
  const sHq = TAU / nHq;
  const intensity = Math.max(0, Math.min(1, o.intensity));
  const dL = ringL - bgL, dC = ringC - bgC;                       // full ease from background to ring

  // Noise amplitudes (applied as amount · (noise − ½))
  const hueAmp = Math.max(0, o.noise) * 2 * sHq;
  const biasAmp = Math.abs(dL) > 1e-6 ? Math.max(0, o.bias) * 2 * sL / Math.abs(dL) : 0;
  const seedHueNoise = (o.seed | 0) + 1, seedWash = (o.seed | 0) + 2, seedBias = (o.seed | 0) + 3;

  // Lattices
  const nCell = Math.max(2, o.cellHues | 0);
  const hueLat = distinctLattice(o.seed | 0, Array.from({ length: nCell }, (_, k) => (k / nCell) * TAU));
  const washMax = Math.max(0, Math.min(1, o.wash));
  const washLat = (i, j) => { const v = hash3(seedWash, i, j) * U32; return washMax * v * v; };   // squared: mostly light washes, a few strong
  const E = EASE[o.ease] || EASE.smooth;

  // Per-column corner index and eased weight, for both lattices (computed once)
  const cellH = Math.max(1, o.cell), cellW = Math.max(1, o.washSize);
  const hIx = new Int32Array(cols), hTx = new Float64Array(cols), wIx = new Int32Array(cols), wTx = new Float64Array(cols);
  for (let c = 0; c < cols; c++) {
    const x = (c + col0 + 0.5) * pc;
    let u = x / cellH, i = Math.floor(u); hIx[c] = i; hTx[c] = E(u - i);
    u = x / cellW; i = Math.floor(u); wIx[c] = i; wTx[c] = E(u - i);
  }
  const hI0 = hIx[0], hN = hIx[cols - 1] - hI0 + 2, wI0 = wIx[0], wN = wIx[cols - 1] - wI0 + 2;
  const hTop = new Float64Array(hN), hBot = new Float64Array(hN), wTop = new Float64Array(wN), wBot = new Float64Array(wN);
  let hJ = null, wJ = null;

  // Output colours: grid levels (qL, qC, qH) → packed sRGB, converted on first use
  const qL0 = Math.floor(Math.min(0, dL * intensity) / sL) - 2, nqL = Math.ceil(Math.abs(dL) * intensity / sL) + 5;
  const nqC = Math.ceil(Math.max(0, dC) * intensity / sC) + 4;
  const nCache = nqL * nqC * nHq;
  const cache = new Uint32Array(nCache), have = new Uint8Array(nCache);
  let used = 0;
  const colourAt = (qL, qC, qH) => {
    const k = ((qL - qL0) * nqC + qC) * nHq + qH;
    if (!have[k]) { cache[k] = pack(oklchToRgb(bgL + qL * sL, Math.max(0, bgC + qC * sC), qH * sHq)); have[k] = 1; used++; }
    return cache[k];
  };
  // [tauler] The background level is the background colour itself, byte for byte. The
  // designer's file drew it as the grid point at hue 0 (colourAt(0, 0, 0)): the background's
  // lightness and chroma at hue 0°, which is not its own hue, so the "background" pixels
  // came out a pinkish rgb(233, 226, 228) on a #e6e4de page.
  const bgPacked = pack(o.background || fb.background); used++;
  const qLmin = qL0, qLmax = qL0 + nqL - 1, qCmax = nqC - 1;

  const T = THRESHOLDS[o.matrix] || THRESHOLDS.bayer8;
  const N = cols * rows;
  const px = reuse && reuse.length === N ? reuse : new Uint32Array(N);

  // One fused pass: every pixel is computed and written exactly once.
  for (let r = 0, p = 0; r < rows; r++) {
    const y = (r + row0 + 0.5) * pc;
    // corner values of the current lattice row, refreshed only when the row changes
    let v = y / cellH, j = Math.floor(v); const hTy = E(v - j);
    if (j !== hJ) { for (let k = 0; k < hN; k++) { hTop[k] = hueLat(hI0 + k, j); hBot[k] = hueLat(hI0 + k, j + 1); } hJ = j; }
    v = y / cellW; j = Math.floor(v); const wTy = E(v - j);
    if (j !== wJ) { for (let k = 0; k < wN; k++) { wTop[k] = washLat(wI0 + k, j); wBot[k] = washLat(wI0 + k, j + 1); } wJ = j; }
    const tRow = ((r + row0) & 7) << 3;

    for (let c = 0; c < cols; c++, p++) {
      // 1. hue: 4-corner mean (hues blend as numbers along the ring) + noise
      let k = hIx[c] - hI0, t = hTx[c];
      let a = hTop[k] + (hTop[k + 1] - hTop[k]) * t, b = hBot[k] + (hBot[k + 1] - hBot[k]) * t;
      const hue = f32(f32(a + (b - a) * hTy) + hueAmp * (f32(hash3(seedHueNoise, c + col0, r + row0) * U32) - 0.5));

      // 2. fade amount: intensity × (1 − wash) + bias noise
      k = wIx[c] - wI0; t = wTx[c];
      a = wTop[k] + (wTop[k + 1] - wTop[k]) * t; b = wBot[k] + (wBot[k + 1] - wBot[k]) * t;
      let amt = f32(f32(intensity * (1 - f32(a + (b - a) * wTy))) + biasAmp * (f32(hash3(seedBias, c + col0, r + row0) * U32) - 0.5));
      amt = amt < 0 ? 0 : amt > 1 ? 1 : amt;

      // 3 + 4. each OKLCH channel, measured from the background, snapped to its grid with
      //        the shared ordered-dither threshold: level = floor(value / step + threshold)
      const th = T[tRow | ((c + col0) & 7)];
      let qL = Math.floor((dL * amt) / sL + th);
      let qC = Math.floor((dC * amt) / sC + th);
      if (qC < 0) qC = 0; else if (qC > qCmax) qC = qCmax;
      if (qL < qLmin) qL = qLmin; else if (qL > qLmax) qL = qLmax;
      let qH = Math.floor(hue / sHq + th) % nHq; if (qH < 0) qH += nHq;
      px[p] = qC === 0 && qL === 0 ? bgPacked : colourAt(qL, qC, qH);
    }
  }

  return {
    data: new Uint8ClampedArray(px.buffer, px.byteOffset, N * 4), pixels: px,
    cols, rows, devW, devH, devP, dpr, pixelCss: pc,
    colours: used, grid: { L: nqL - 5, C: nqC - 4, H: nHq }, clipped,
  };
}

/* ────────────────────────────────────────────────────────────────────────────────────
   CSS variables
   ──────────────────────────────────────────────────────────────────────────────────── */

let probe = null;
/**
 * Read the wallpaper's CSS custom properties from `el`. Colours accept any CSS colour
 * syntax (oklch(), hex, …): each is painted onto a 1×1 canvas and read back as sRGB, which
 * is exactly how the browser itself converts it. Numbers accept plain values or %.
 * Missing or invalid values are left out (computeWallpaper then uses FALLBACK).
 * @param {Element} el
 * @returns {{background?: number[], ringL?: number, ringC?: number}}
 */
export function readCssColors(el) {
  if (!el || typeof getComputedStyle === 'undefined') return {};
  const cs = getComputedStyle(el), out = {};
  if (!probe) {
    const cv = document.createElement('canvas'); cv.width = cv.height = 1;
    probe = cv.getContext('2d', { willReadFrequently: true });
  }
  const colour = (v) => {
    v = v.trim();
    if (!v || (typeof CSS !== 'undefined' && CSS.supports && !CSS.supports('color', v))) return null;
    probe.clearRect(0, 0, 1, 1); probe.fillStyle = v; probe.fillRect(0, 0, 1, 1);
    const d = probe.getImageData(0, 0, 1, 1).data;
    return d[3] === 255 ? [d[0], d[1], d[2]] : null;           // translucent values are ignored
  };
  const number = (v) => { v = v.trim(); if (!v) return null; const n = parseFloat(v); return Number.isFinite(n) ? (v.endsWith('%') ? n / 100 : n) : null; };
  const bg = colour(cs.getPropertyValue(CSS_VARS.background)); if (bg) out.background = bg;
  const l = number(cs.getPropertyValue(CSS_VARS.ringL)); if (l != null) out.ringL = l;
  const c = number(cs.getPropertyValue(CSS_VARS.ringC)); if (c != null) out.ringC = c;
  return out;
}

/* ────────────────────────────────────────────────────────────────────────────────────
   renderWallpaper: draw onto a canvas
   ──────────────────────────────────────────────────────────────────────────────────── */

const state = new WeakMap();   // per-canvas reusable buffers: { scratch, imageData, pixels }

/**
 * Compute and draw onto `canvas`. The canvas must be CSS-sized to fill its parent
 * (width/height 100%). Its backing store is set to the exact device-pixel size, so the
 * browser shows it 1:1 and never resamples; the dither image is drawn at dither-pixel
 * resolution and enlarged by the whole number devP with smoothing off. Partial pixels at
 * the right / bottom edge are cropped.
 * @param {HTMLCanvasElement} canvas
 * @param {object} [options]  as computeWallpaper; width/height default to the parent's size,
 *                            CSS colours are read from the parent unless cssColors: false
 */
export function renderWallpaper(canvas, options = {}) {
  const t0 = performance.now();
  const parent = canvas.parentElement;
  const width = options.width != null ? options.width : parent.clientWidth;
  const height = options.height != null ? options.height : parent.clientHeight;
  const dpr = options.dpr || window.devicePixelRatio || 1;
  const css = options.cssColors !== false ? readCssColors(parent) : {};

  let st = state.get(canvas);
  if (!st) { st = { scratch: document.createElement('canvas'), imageData: null, pixels: null }; state.set(canvas, st); }
  const res = computeWallpaper(Object.assign({}, css, options, { width, height, dpr }), st.pixels);
  if (res.clipped && !st.warned) { console.warn('tauler-wallpaper: area exceeds the browser canvas limit; the bottom is not drawn. See README → Limits.'); st.warned = true; }

  if (canvas.width !== res.devW) canvas.width = res.devW;
  if (canvas.height !== res.devH) canvas.height = res.devH;

  // Reuse the ImageData (and its buffer) while the size is unchanged
  if (!st.imageData || st.imageData.width !== res.cols || st.imageData.height !== res.rows || st.pixels !== res.pixels) {
    st.imageData = new ImageData(res.data, res.cols, res.rows);
    st.pixels = res.pixels;
  }
  const s = st.scratch;
  if (s.width !== res.cols) s.width = res.cols;
  if (s.height !== res.rows) s.height = res.rows;
  s.getContext('2d').putImageData(st.imageData, 0, 0);

  const ctx = canvas.getContext('2d');
  ctx.imageSmoothingEnabled = false;
  ctx.drawImage(s, 0, 0, res.cols * res.devP, res.rows * res.devP);

  return {
    ms: performance.now() - t0, cols: res.cols, rows: res.rows, devW: res.devW, devH: res.devH,
    devP: res.devP, dpr, pixelCss: res.pixelCss, exact: !!options.deviceWidth,
    colours: res.colours, grid: res.grid, clipped: res.clipped,
  };
}

/* ────────────────────────────────────────────────────────────────────────────────────
   mountWallpaper: the integration helper
   ──────────────────────────────────────────────────────────────────────────────────── */

/**
 * Insert the wallpaper behind the content of `host` and keep it up to date.
 * The host must establish a containing block (e.g. position: relative). The layer is
 * absolutely positioned (inset: 0, z-index: 0), aria-hidden and ignores the pointer.
 *
 * Re-renders automatically (debounced) when:
 *   · the host is resized (exact device-pixel size via ResizeObserver),
 *   · the device pixel ratio changes (zoom, moving to another screen),
 *   · the theme changes: class / style / data-* attributes on <html> or the host, or the
 *     prefers-color-scheme media query (unless watchTheme: false).
 * A render only happens if the size, options or CSS colours actually changed.
 *
 * @param {HTMLElement} host
 * @param {object} [options]  DEFAULTS overrides plus:
 *   onRender(info)  called after every render with timing / size / colour stats
 *   debounce        ms to wait after resizes (default 120)
 *   watchTheme      re-render on theme changes (default true)
 * @returns {{canvas: HTMLCanvasElement, layer: HTMLDivElement, readonly last: object|null,
 *            update(next: object): object, refresh(): object, destroy(): void}}
 */
export function mountWallpaper(host, options = {}) {
  let opts = Object.assign({}, options);
  const layer = document.createElement('div');
  layer.className = 'tauler-wallpaper';
  layer.setAttribute('aria-hidden', 'true');
  Object.assign(layer.style, { position: 'absolute', inset: '0', zIndex: '0', overflow: 'hidden', pointerEvents: 'none' });
  const canvas = document.createElement('canvas');
  Object.assign(canvas.style, { position: 'absolute', left: '0', top: '0', width: '100%', height: '100%', display: 'block', imageRendering: 'pixelated' });
  layer.appendChild(canvas);
  host.insertBefore(layer, host.firstChild);

  let last = null, timer = 0, lastKey = '', devSize = null, destroyed = false;
  const renderOptions = () => {
    const { onRender, debounce, watchTheme, ...rest } = opts;   // eslint-disable-line no-unused-vars
    return rest;
  };
  function render(force) {
    if (destroyed) return last;
    const rect = canvas.getBoundingClientRect();
    const w = rect.width, h = rect.height, dpr = window.devicePixelRatio || 1;
    if (w <= 0 || h <= 0) return last;                           // hidden / not laid out yet
    const css = readCssColors(layer);
    const key = JSON.stringify([w, h, dpr, devSize, Object.assign({}, DEFAULTS, renderOptions()), css]);   // effective options
    if (!force && key === lastKey) return last;
    lastKey = key;
    last = renderWallpaper(canvas, Object.assign({}, renderOptions(), { width: w, height: h, dpr },
      devSize ? { deviceWidth: devSize[0], deviceHeight: devSize[1] } : {}));
    if (typeof opts.onRender === 'function') opts.onRender(last);
    return last;
  }
  const schedule = () => { clearTimeout(timer); timer = setTimeout(() => render(false), opts.debounce != null ? opts.debounce : 120); };

  // Size: exact device pixels where supported (Chrome, Firefox); Safari falls back to CSS px × dpr
  const ro = new ResizeObserver((entries) => {
    const box = entries[0].devicePixelContentBoxSize && entries[0].devicePixelContentBoxSize[0];
    devSize = box ? [box.inlineSize, box.blockSize] : null;
    if (devSize && last && !last.exact) render(false);           // first exact measurement: no delay
    else schedule();
  });
  try { ro.observe(canvas, { box: 'device-pixel-content-box' }); } catch (e) { ro.observe(canvas); }

  // Device pixel ratio: a resolution media query that fires when the DPR changes
  let dprQuery = null;
  const onDpr = () => { watchDpr(); schedule(); };
  const watchDpr = () => {
    if (dprQuery) dprQuery.removeEventListener('change', onDpr);
    dprQuery = matchMedia(`(resolution: ${window.devicePixelRatio || 1}dppx)`);
    dprQuery.addEventListener('change', onDpr);
  };
  watchDpr();

  // Theme: attribute changes on <html> / host, and the OS colour scheme
  let mo = null, schemeQuery = null;
  if (opts.watchTheme !== false) {
    mo = new MutationObserver(schedule);
    const attrs = { attributes: true, attributeFilter: ['class', 'style', 'data-theme', 'data-mode'] };
    mo.observe(document.documentElement, attrs);
    if (host !== document.documentElement) mo.observe(host, attrs);
    schemeQuery = matchMedia('(prefers-color-scheme: dark)');
    schemeQuery.addEventListener('change', schedule);
  }

  render(true);
  return {
    canvas, layer,
    get last() { return last; },
    /** Merge new options and re-render if anything changed. */
    update(next) { opts = Object.assign({}, opts, next); return render(false); },
    /** Re-render now, e.g. after changing CSS variables without an observed attribute. */
    refresh() { return render(true); },
    /** Remove the layer and all observers / listeners. */
    destroy() {
      destroyed = true; clearTimeout(timer); ro.disconnect();
      if (dprQuery) dprQuery.removeEventListener('change', onDpr);
      if (mo) mo.disconnect();
      if (schemeQuery) schemeQuery.removeEventListener('change', schedule);
      layer.remove();
    },
  };
}
