/** Type declarations for tauler-wallpaper.js (see README.md). */

export type DitherMatrix = 'bayer8' | 'bayer4' | 'checker';
export type Ease = 'smooth' | 'linear';
export type RGB = [number, number, number];

export interface WallpaperOptions {
  /** Which FALLBACK colours to use when the CSS variables are missing. */
  mode?: 'light' | 'dark';
  /** Size of one dither pixel, in pixelUnit units. Default 3. */
  pixelSize?: number;
  /** 'device': pixelSize counts physical device px (same physical size at every zoom level).
   *  'css': pixelSize counts CSS px, rounded to whole device px (scales with zoom). Default 'device'. */
  pixelUnit?: 'device' | 'css';
  /** Dither pattern. Default 'bayer8'. */
  matrix?: DitherMatrix;
  /** How far colours go from the background toward the ring, 0..1. Default 0.1. */
  intensity?: number;
  /** Number of hues on the corner lattice. Default 3. */
  cellHues?: number;
  /** Corner spacing of the hue lattice, CSS px. Default 420. */
  cell?: number;
  /** Corner weighting. Default 'smooth'. */
  ease?: Ease;
  /** Strongest extra fade toward the background, 0..1. Default 1. */
  wash?: number;
  /** Corner spacing of the wash lattice, CSS px. Default 300. */
  washSize?: number;
  /** Per-pixel random hue nudge, in hue steps. Default 0.1. */
  noise?: number;
  /** Per-pixel random nudge of the fade amount, in lightness steps. Default 0.05. */
  bias?: number;
  /** OKLCH lightness quantization step. Default 0.035. */
  stepL?: number;
  /** OKLCH chroma quantization step. Default 0.04. */
  stepC?: number;
  /** Hue quantization step in degrees. Default 60. */
  stepH?: number;
  /** Any integer; the same seed always gives the same wallpaper. Default 18. */
  seed?: number;
  /** Override the background (sRGB 0..255). Normally read from --wallpaper-bg. */
  background?: RGB;
  /** Override the hue ring's OKLCH lightness. Normally read from --wallpaper-ring-l. */
  ringL?: number;
  /** Override the hue ring's OKLCH chroma. Normally read from --wallpaper-ring-c. */
  ringC?: number;
}

export interface ComputeOptions extends WallpaperOptions {
  /** Size in CSS px. */
  width: number;
  height: number;
  /** Device pixel ratio. Default 1. */
  dpr?: number;
  /** Exact size in device px, if measured (preferred over width/height × dpr). */
  deviceWidth?: number;
  deviceHeight?: number;
  /** [tauler] The area's top-left, in whole dither pixels from the page's top-left. Default 0. */
  col0?: number;
  row0?: number;
}

export interface ComputeResult {
  /** RGBA bytes at dither-pixel resolution (cols × rows), ready for ImageData. */
  data: Uint8ClampedArray;
  /** The same buffer as packed 32-bit pixels; pass back in to reuse it. */
  pixels: Uint32Array;
  cols: number;
  rows: number;
  /** Size of the drawn area in device px. */
  devW: number;
  devH: number;
  /** Device px per dither pixel (whole number). */
  devP: number;
  dpr: number;
  /** CSS px per dither pixel. */
  pixelCss: number;
  /** Distinct output colours used. */
  colours: number;
  /** Number of quantization levels covered per channel. */
  grid: { L: number; C: number; H: number };
  /** True if the area exceeded the browser canvas limit and the bottom was cut off. */
  clipped: boolean;
}

export interface RenderInfo {
  ms: number;
  cols: number;
  rows: number;
  devW: number;
  devH: number;
  devP: number;
  dpr: number;
  pixelCss: number;
  /** True if the size came from an exact device-pixel measurement. */
  exact: boolean;
  colours: number;
  grid: { L: number; C: number; H: number };
  clipped: boolean;
}

export interface MountOptions extends WallpaperOptions {
  /** Called after every render. */
  onRender?: (info: RenderInfo) => void;
  /** Milliseconds to wait after a resize before re-rendering. Default 120. */
  debounce?: number;
  /** Re-render on theme changes (attributes on <html>/host, prefers-color-scheme). Default true. */
  watchTheme?: boolean;
}

export interface WallpaperController {
  readonly canvas: HTMLCanvasElement;
  readonly layer: HTMLDivElement;
  /** Info from the most recent render, or null. */
  readonly last: RenderInfo | null;
  /** Merge new options and re-render if anything changed. */
  update(next: MountOptions): RenderInfo | null;
  /** Re-render now (e.g. after changing CSS variables without an observed attribute). */
  refresh(): RenderInfo | null;
  /** Remove the layer and all observers / listeners. */
  destroy(): void;
}

export const DEFAULTS: Readonly<Required<Omit<WallpaperOptions, 'background' | 'ringL' | 'ringC'>>>;
export const FALLBACK: Readonly<Record<'light' | 'dark', { background: RGB; ringL: number; ringC: number }>>;
export const CSS_VARS: Readonly<{ background: string; ringL: string; ringC: string }>;

export function computeWallpaper(options: ComputeOptions, reuse?: Uint32Array): ComputeResult;
export function renderWallpaper(canvas: HTMLCanvasElement, options?: Partial<ComputeOptions> & { cssColors?: boolean }): RenderInfo;
export function mountWallpaper(host: HTMLElement, options?: MountOptions): WallpaperController;
export function readCssColors(el: Element): { background?: RGB; ringL?: number; ringC?: number };
export function rgbToOklab(rgb: RGB): [number, number, number];
export function oklchToRgb(L: number, C: number, h: number): RGB;

// [tauler] Internals exported for the WebGL wallpaper (src/lib/wallpaper/wallpaper-gl.ts),
// so it reads the same lattices, hash and thresholds as computeWallpaper.
export function hash3(seed: number, i: number, j: number): number;
export const U32: number;
export function distinctLattice(seed: number, values: number[]): (i: number, j: number) => number;
export const THRESHOLDS: Readonly<Record<DitherMatrix, Float32Array>>;
export const EASE: Readonly<Record<Ease, (t: number) => number>>;
