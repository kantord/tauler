// The theme as d3 wants it: every token as `rgb(...)`, plus the few derived
// colours every chart here needs.
//
// The built-in themes write tokens as `oklch(L C H)`, which d3-color 3 cannot
// parse (see examples/weather-map/CityScene.jsx for how that first showed up),
// so they are converted once here.
import * as d3 from "d3";
import { contentKey } from "./keys.js";

export function toRgb(color) {
  const m = /^oklch\(\s*([\d.]+)(%?)\s+([\d.]+)\s+([\d.]+)(?:deg)?\s*(?:\/\s*([\d.]+)(%?))?\s*\)$/i.exec(color ?? "");
  if (!m) return color;
  const L = m[2] ? m[1] / 100 : +m[1];
  // Not `h`: that name is the JSX factory in a layout file (`h(type, props)`).
  const hue = (m[4] * Math.PI) / 180;
  const a = m[3] * Math.cos(hue);
  const b = m[3] * Math.sin(hue);
  // OKLab → linear sRGB → sRGB (Björn Ottosson's published matrices)
  const l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3;
  const mm = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3;
  const s = (L - 0.0894841775 * a - 1.291485548 * b) ** 3;
  const lin = [
    4.0767416621 * l - 3.3077115913 * mm + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * mm - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * mm + 1.707614701 * s,
  ];
  const [r, g, bl] = lin.map((c) => {
    const v = c <= 0.0031308 ? 12.92 * c : 1.055 * c ** (1 / 2.4) - 0.055;
    return Math.round(255 * Math.min(1, Math.max(0, v)));
  });
  return `rgb(${r},${g},${bl})`;
}

export function chartTheme(theme) {
  const c = Object.fromEntries(Object.entries(theme.colors).map(([k, v]) => [k, toRgb(v)]));
  const mix = (a, b, t) => d3.interpolateRgb(a, b)(t);
  return {
    ...c,
    mix,
    // Gridlines and axes sit well back from the data.
    grid: mix(c.background, c.foreground, 0.12),
    axis: mix(c.background, c.foreground, 0.45),
    // Categorical series, in the theme's own order of accents. A missing token
    // falls back to the foreground in place: charts address series by index
    // (`series[1]` is CPU), so dropping one would shift every colour after it.
    series: [c.primary, c["ansi-cyan"], c["ansi-magenta"], c["ansi-yellow"], c["ansi-green"], c["ansi-red"]]
      .map((color) => color ?? c.foreground),
  };
}

// A key for "this theme": a D3Figure redraws when it changes. The colours
// themselves, not the name — a theme file edited in place keeps its name.
export const themeKey = (theme) => contentKey(theme.colors);
