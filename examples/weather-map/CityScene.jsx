// CityScene: a few city blocks around a point, drawn as a tilted 3D view with
// building heights exaggerated, meant to sit behind a card's text.
//
// TERM: City scene — the drawing this file makes: ground, water, parks,
// roads and extruded buildings around one point, seen from above at an angle.
// It sits behind a card's text; nothing on the card is read from it.
// Avoid: backdrop (CONTEXT.md reserves it as an _Avoid_ for Wallpaper and
// tauler:root-bg), map (suggests panning, zoom, interactivity), terrain (no
// elevation model is involved), background (already a theme token).
// PROVISIONAL (Q4): the name.
//
// Paths, and what each needs:
//   P1 happy — the weather card shows the scene for the weather's own location.
//      Needs: coordinates (fetcher, below), cached geometry, a scene built off
//      the per-tick path, pixels drawn once. All four traced; see DECISIONs.
//   P2 no scene — first run (Overpass takes ~40 s), offline, Overpass refusing.
//      The card renders exactly as <WeatherSummary> does today, no gap, no error.
//   P3 location moves — new coordinates → new geometry → new scene; the old one
//      stays up until the new one is ready.
//   P4 theme switch — same geometry, new colours; no refetch.
//   P5 the browser renderer (ADR 0026) — out of the tracer.
// ASK (Q5): P5 in scope? The browser has no `tauler:picture/` binding; it would
// need the SVG itself, served as a data URI, which costs nothing there.
// Recommend: defer — no Dom surface shows the sidebar today.
//
// SPEC (P2): with no City geometry yet, CityScene renders nothing and the card
// keeps its normal background.
// SPEC (P3/P4): a scene is keyed by (geometry centre, size, theme colours); any
// other change reuses the existing one.
//
// TERM: City geometry — what a City scene is drawn from: `{center: [lon, lat],
// buildings: [{h, ring}], roads: [{major, line}], water: [ring], parks: [ring]}`,
// lon/lat pairs, heights in metres. Produced by `fetch-city.sh` for the tracer.
// Avoid: map data, tiles (there are none — it is vector geometry from OSM).
//
// ASK (Q1): what "exaggerated 3D projection" means — (a) a perspective camera
// pitched over the city with building heights ×3, (b) isometric, no
// perspective, (c) terrain relief from an elevation model, heights ×10.
// Recommend: (a); it is what the tracer draws.
// PROVISIONAL (Q1): "exaggerated 3D projection" is read as a perspective camera
// pitched down over the city, with building heights multiplied by EXAGGERATION.
// The alternatives in Q1 (isometric, terrain relief) change only `project()`.
//
// Pipeline, all of it once per (geometry, size, theme) — see `memo` below:
//   1. lon/lat → metres on a ground plane: d3.geoMercator scaled to 1 unit = 1 m
//   2. metres → camera space: pitch, distance, perspective divide (`project`)
//   3. buildings → wall quads + roof, painter's algorithm (far first)
//   4. everything → one SVG string, colours from the theme, fog by depth
import * as d3 from "./.data/d3.js";

// PROVISIONAL (Q1): the look. Every number here is taste, not measurement.
const EXAGGERATION = 3; // building height multiplier
const PITCH_DEG = 50; // camera angle below horizontal; 90 = straight down
const DISTANCE_M = 650; // camera distance from the centre point
const FOCAL = 1.5; // larger = flatter; smaller = stronger perspective
const NEAR_M = 30; // geometry closer to the camera than this is dropped

const EARTH_RADIUS_M = 6378137;

// Step 1. Mercator at scale R·cos(lat) makes one projected unit one metre at the
// centre's latitude; at 500 m radius the scale error across the area is <0.01%.
function groundPlane(center) {
  const p = d3
    .geoMercator()
    .center(center)
    .scale(EARTH_RADIUS_M * Math.cos((center[1] * Math.PI) / 180))
    .translate([0, 0]);
  // x east, f north (Mercator's y grows south)
  return ([lon, lat]) => {
    const [x, y] = p([lon, lat]);
    return [x, -y];
  };
}

// Step 2. Camera behind the centre point (to the south), looking north and down.
function camera(width, height) {
  const pitch = (PITCH_DEG * Math.PI) / 180;
  const sin = Math.sin(pitch);
  const cos = Math.cos(pitch);
  const scale = (FOCAL * width) / 2;
  // Returns [screenX, screenY, depth], or null behind the near plane.
  return (x, f, z) => {
    const depth = f * cos - z * sin + DISTANCE_M;
    if (depth < NEAR_M) return null;
    const up = f * sin + z * cos;
    return [
      width / 2 + (x * scale) / depth,
      height * 0.62 - (up * scale) / depth,
      depth,
    ];
  };
}

const fmt = (n) => (Math.round(n * 10) / 10).toString();

function polygon(points) {
  return "M" + points.map((p) => fmt(p[0]) + "," + fmt(p[1])).join("L") + "Z";
}

function offscreen(points, width, height) {
  const m = 40;
  return points.every(
    (p) => p[0] < -m || p[0] > width + m || p[1] < -m || p[1] > height + m,
  );
}

// DECISION: theme colours go through `toRgb` before d3 sees them. The built-in
// themes write their tokens as `oklch(L C H)` (default dark: background
// `oklch(0.145 0 0)`), and d3-color 3 does not parse OKLCH: `d3.color` returns
// null and every mix came out black — the first tracer run drew only the parks,
// whose `ansi-green` happens to be hex. Rejected: asking themes for hex.
function toRgb(color) {
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

// THEME-GAP: the scene needs a ramp (ground → roof), fog and water colours;
// the theme has none of these, so they are mixed from the tokens it does have.
function palette(themeColors) {
  const colors = Object.fromEntries(Object.entries(themeColors).map(([k, v]) => [k, toRgb(v)]));
  const mix = (a, b, t) => d3.interpolateRgb(a, b)(t);
  const bg = colors.background;
  const fg = colors.foreground;
  return {
    ground: mix(bg, fg, 0.04),
    road: mix(bg, fg, 0.18),
    major: mix(bg, colors.primary ?? fg, 0.35),
    water: mix(bg, colors["ansi-blue"] ?? fg, 0.35),
    park: mix(bg, colors["ansi-green"] ?? fg, 0.18),
    roof: mix(bg, fg, 0.3),
    wallLit: mix(bg, fg, 0.2),
    wallDark: mix(bg, fg, 0.09),
    // Far geometry fades into the background: the depth cue that sells the 3D.
    fog: (color, depth) =>
      mix(color, bg, Math.min(1, Math.max(0, (depth - 350) / 900))),
  };
}

export function citySceneSvg(city, width, height, colors) {
  const toGround = groundPlane(city.center);
  const cam = camera(width, height);
  const pal = palette(colors);
  const flat = (ring) => ring.map((p) => toGround(p));
  const parts = [];

  const flatShapes = (rings, color, kind) => {
    for (const ring of rings) {
      const pts = flat(ring).map(([x, f]) => cam(x, f, 0));
      if (pts.some((p) => !p) || offscreen(pts, width, height)) continue;
      const depth = d3.mean(pts, (p) => p[2]);
      const d = kind === "line" ? "M" + pts.map((p) => fmt(p[0]) + "," + fmt(p[1])).join("L") : polygon(pts);
      parts.push({ depth, ground: true, svg: kind === "line" ? `<path d="${d}" fill="none" stroke="${pal.fog(color.stroke, depth)}" stroke-width="${color.width}" stroke-linecap="round"/>` : `<path d="${d}" fill="${pal.fog(color, depth)}"/>` });
    }
  };
  flatShapes(city.parks, pal.park, "area");
  flatShapes(city.water, pal.water, "area");
  flatShapes(city.roads.filter((r) => !r.major).map((r) => r.line), { stroke: pal.road, width: 1 }, "line");
  flatShapes(city.roads.filter((r) => r.major).map((r) => r.line), { stroke: pal.major, width: 1.6 }, "line");

  // Step 3. One building = its walls (far first) then its roof, emitted as one
  // unit so the painter's sort below works per building, not per face.
  for (const b of city.buildings) {
    const ground = flat(b.ring);
    const top = b.h * EXAGGERATION;
    const base = ground.map(([x, f]) => cam(x, f, 0));
    const roof = ground.map(([x, f]) => cam(x, f, top));
    if (base.some((p) => !p) || roof.some((p) => !p)) continue;
    if (offscreen(roof, width, height) && offscreen(base, width, height)) continue;
    const walls = [];
    for (let i = 0; i < ground.length - 1; i++) {
      // Light from the west-southwest: a wall's shade is its normal against it.
      const [x0, f0] = ground[i];
      const [x1, f1] = ground[i + 1];
      const len = Math.hypot(x1 - x0, f1 - f0) || 1;
      const lit = Math.max(0, ((f1 - f0) * -0.8 + (x1 - x0) * -0.6) / len);
      const depth = (base[i][2] + base[i + 1][2]) / 2;
      walls.push({
        depth,
        d: polygon([base[i], base[i + 1], roof[i + 1], roof[i]]),
        color: pal.fog(d3.interpolateRgb(pal.wallDark, pal.wallLit)(lit), depth),
      });
    }
    walls.sort((a, b) => b.depth - a.depth);
    const depth = d3.max(base, (p) => p[2]);
    const roofDepth = d3.mean(roof, (p) => p[2]);
    parts.push({
      depth,
      svg:
        walls.map((w) => `<path d="${w.d}" fill="${w.color}"/>`).join("") +
        `<path d="${polygon(roof)}" fill="${pal.fog(pal.roof, roofDepth)}"/>`,
    });
  }

  // Step 3, painter's algorithm: everything on the ground first, then buildings
  // far to near. A building's sort key is its farthest base corner, which gets
  // long footprints wrong at some angles.
  // DECISION: sort per building, by farthest base corner. Checked at 2× card
  // size on the four sampled cities (Manhattan towers, Eixample blocks, Gothic
  // alleys, Budapest centre) at PITCH_DEG 50: no visible overlap errors. Rejected:
  // sorting per wall — more work, and no visible error for it to fix at 248×96.
  // A steeper pitch or a larger card reopens this.
  parts.sort((a, b) => (a.ground === b.ground ? b.depth - a.depth : a.ground ? -1 : 1));

  return (
    `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}">` +
    `<rect width="${width}" height="${height}" fill="${pal.ground}"/>` +
    parts.map((p) => p.svg).join("") +
    `</svg>`
  );
}

// Measured with `cargo run --release --example weather_map_tracer`, 2026-10-23,
// card 248×96 in a 272 px sidebar at 140 dpi, default-dark, 20 runs each:
//
//                      suburb  manhattan  eixample  gothic
//   SVG size            220 KB    692 KB   1119 KB  2515 KB
//   build (once)         58 ms    198 ms    368 ms   605 ms
//   eval, data URI     1.5 ms    3.6 ms    7.9 ms   19 ms    ← every tick
//   eval, short key    0.05 ms   0.13 ms   0.09 ms  0.17 ms  ← every tick
//   raster, data URI    12 ms     34 ms     50 ms   101 ms   ← every repaint
//   raster, bitmap     0.7 ms    0.4 ms    0.6 ms   0.4 ms   ← every repaint
//   raster, no scene   0.6 ms    0.4 ms    0.4 ms   0.5 ms
//
// Four runs moved every number by up to 2× with machine load (gothic build:
// 407–670 ms); none of the decisions below sits near a threshold that moves.
//
// DECISION: the scene must not travel as a data URI. It costs twice on every
// tick: the string goes through `json_stringify` and serde on its way out of
// QuickJS (eval column), and takumi re-parses and re-rasterizes the SVG on every
// repaint (`preload_layout_images` skips `data:` sources, src/render/mod.rs).
// The sidebar repaints at drag rate under the volume slider, so 34–101 ms per
// repaint is a frozen slider. A pre-drawn bitmap is indistinguishable from no
// scene at all. Rejected: `<img src={dataUri}>`, which is what this file did first.
//
// DECISION: the scene reaches the renderer as a short key naming pixels tauler
// drew once — a Picture, src/render/picture.rs. Per tick that is 0.05–0.17 ms,
// stream handoff of the geometry included (layout-stream.jsx).
// PROVISIONAL (Q3): `picture(key, size, build)` is the recommended shape.
//
// DECISION: the build runs once per scene key, inside `build` — tauler calls it
// only for a key it does not hold. The module is evaluated once per load and
// `render()` on every tick (`JsxEvaluator::eval`, src/jsx.rs), so module state
// would survive between renders too, but a key held on the Rust side also
// survives the raster.
//
// ASK (Q2): the build blocks the render loop once per scene key: 58–605 ms in
// the sampled cities at 500 m radius (worst: dense old town). That happens at
// load, on a theme switch and when the location moves — never during a drag.
// Recommend: accept it, and cap it with a radius of 350 m (half the area of 500 m).
//
// PROVISIONAL (Q3): until Picture exists, the tracer keeps the data URI so it
// renders at all; the numbers above are why it cannot ship that way.
const memo = new Map();

export function citySceneUri(city, width, height, colors) {
  const key = `${city.center}|${width}x${height}|${colors.background}|${colors.foreground}`;
  let uri = memo.get(key);
  if (!uri) {
    uri = "data:image/svg+xml;utf8," + encodeURIComponent(citySceneSvg(city, width, height, colors));
    memo.set(key, uri);
  }
  return uri;
}

// SPEC (P1, needs Picture):
//   export default function CityScene({ city, width, height }) {
//     if (!city) return null;                                    // P2
//     const key = sceneKey(city.center, width, height, ctx.theme.colors);
//     return <img src={picture(key, { width, height },
//                              () => citySceneSvg(city, width, height, ctx.theme.colors))}
//                 style={{ position: "absolute", top: 0, left: 0, width, height }} />;
//   }
export default function CityScene({ city, width, height }) {
  return (
    <img
      src={citySceneUri(city, width, height, ctx.theme.colors)}
      style={{ position: "absolute", top: 0, left: 0, width, height }}
    />
  );
}
