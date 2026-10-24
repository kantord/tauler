// The theme as d3 wants it: every token as `rgb(...)`, plus the few derived
// colours every chart here needs.
//
// The built-in themes write tokens as `oklch(L C H)`, which d3-color 3 cannot
// parse (see examples/weather-map/CityScene.jsx for how that first showed up),
// so they are converted once here.
import { d3 } from "./.data/d3dom.js";

export function toRgb(color) {
  const m = /^oklch\(\s*([\d.]+)(%?)\s+([\d.]+)\s+([\d.]+)(?:deg)?\s*(?:\/\s*([\d.]+)(%?))?\s*\)$/i.exec(color ?? "");
  if (!m) return color;
  const L = m[2] ? m[1] / 100 : +m[1];
  const h = (m[4] * Math.PI) / 180;
  const a = m[3] * Math.cos(h);
  const b = m[3] * Math.sin(h);
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
    // Categorical series, in the theme's own order of accents.
    series: [c.primary, c["ansi-cyan"], c["ansi-magenta"], c["ansi-yellow"], c["ansi-green"], c["ansi-red"]]
      .filter(Boolean),
  };
}

// A stable key for "this theme": D3View redraws when it changes.
export const themeKey = (theme) => `${theme.name}/${theme.mode}`;
