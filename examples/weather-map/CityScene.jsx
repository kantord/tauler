// CityScene: a few city blocks around a point, drawn as a tilted 3D view with
// building heights exaggerated, meant to sit behind a card's text.
//
// DECISION (Q7, answered): this is an example that exercises <D3View>, not a
// shipped widget; it may never merge into tauler. OSM's ODbL credit lives here,
// in the file: map data © OpenStreetMap contributors.
// DECISION (Q6, answered): if it ships, it ships in the dotfiles-tauler Package
// in pieces — the fetcher, the scene, the card — next to <Weather>.
//
// TERM: City scene — the drawing this file makes: ground, water, parks,
// roads and extruded buildings around one point, seen from above at an angle.
// It sits behind a card's text; nothing on the card is read from it.
// Avoid: backdrop (CONTEXT.md reserves it as an _Avoid_ for Wallpaper and
// tauler:root-bg), map (suggests panning, zoom, interactivity), terrain (no
// elevation model is involved), background (already a theme token).
// DECISION (Q4, answered): the name.
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
//   P5 the browser renderer (ADR 0026) — not in the first step, in the first
//      merge (Q5, answered; see src/d3_view.rs).
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
// DECISION (Q1, answered in part): the goal is a feeling of space, not a
// faithful projection — at 500 m the earth's real curvature drops the ground by
// 2 cm, so anything that reads as "3D" here is a deliberate exaggeration. The
// base is a perspective camera pitched over the city, heights ×EXAGGERATION,
// far geometry fading into the background colour (atmospheric perspective).
// DECISION (Q1, answered): the shipped look is `planet` — fake curvature that
// brings a horizon and sky into the card. Rendered beside `shadows` (barely
// visible on dark themes) and `miniature` (planet + shadows + tilt-shift), it was
// the only cue that added something at 248×96. Shadows and tilt-shift were
// removed with the shaded renderer; `flat` stays for comparison
// (TRACER_LOOK=flat).
//
// Pipeline, all of it once per (geometry, size, theme) — see `memo` below:
//   1. lon/lat → metres on a ground plane: d3.geoMercator scaled to 1 unit = 1 m
//   2. metres → camera space: pitch, distance, perspective divide (`project`)
//   3. buildings → wall quads + roof, painter's algorithm (far first)
//   4. everything → one SVG string in the illustration style, theme colours
import * as d3 from "./.data/d3.js";

// The look's numbers are taste, tuned by eye on the four sampled cities.
// Heights are exaggerated on a log curve, not by a constant: a 10 m house
// draws ×2.5, a 30 m block ×2, a 150 m tower ×1. A constant ×3 let Manhattan's
// towers fill the card while a village still looked flat.
const EXAGGERATION = 3; // trees, and the scale of the curve below
const HEIGHT_KNEE_M = 25;
const drawnHeight = (h) => EXAGGERATION * HEIGHT_KNEE_M * Math.log1p(h / HEIGHT_KNEE_M);
const PITCH_DEG = 50; // camera angle below horizontal; 90 = straight down
const DISTANCE_M = 650; // camera distance for a full city block; see `frame`
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

// The look: `planet` ships (Q1); `flat` is kept for comparison.
//   curvature — fake planet radius in metres: the ground falls away with
//               distance like a tiny planet, so a horizon and sky appear
//               (0 = flat; the real earth's 6371 km would drop 2 cm at 500 m)
export const LOOKS = {
  flat: { curvature: 0 },
  planet: { curvature: 1100 },
};

// Step 2. Camera behind the centre point (to the south), looking north and down.
function camera(width, height, look = LOOKS.planet, distance = DISTANCE_M) {
  const pitch = (PITCH_DEG * Math.PI) / 180;
  const sin = Math.sin(pitch);
  const cos = Math.cos(pitch);
  const scale = (FOCAL * width) / 2;
  // Returns [screenX, screenY, depth], or null behind the near plane.
  // Curvature: drop every point by d²/2R, d its ground distance from the camera,
  // less the centre's own drop so the centre point stays where it was.
  const camF = -distance * cos;
  // Curvature scales with the camera distance, so a close village bends as much
  // as a far city does.
  const radius = (look.curvature * distance) / DISTANCE_M;
  const drop = (x, f) =>
    look.curvature ? (x * x + (f - camF) ** 2 - camF * camF) / (2 * radius) : 0;
  return (x, f, z0) => {
    const z = z0 - drop(x, f);
    const depth = f * cos - z * sin + distance;
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

// The illustration style: flat fills, two wall tones, ink outlines — a vector
// drawing, not a shaded render. Every colour is a theme token or a mix of two,
// so the scene follows the user's theme, light or dark.
//
// DECISION: the scene is drawn at full contrast and knows nothing about the text
// that will sit on it. Legibility belongs to the layers above the `<img>` in the
// card (a veil, CSS `filter`), which takumi supports: brightness, contrast,
// saturate, grayscale, opacity, blur. Rejected: fog and dimming baked into the
// SVG (the first version) — every card that wanted a different balance would
// have needed a different drawing.
//
// THEME-GAP: the theme has no map colours; they are mixed from the tokens it has.
function palette(themeColors, mode) {
  const c = Object.fromEntries(Object.entries(themeColors).map(([k, v]) => [k, toRgb(v)]));
  const mix = (a, b, t) => d3.interpolateRgb(a, b)(t);
  const bg = c.background;
  const fg = c.foreground;
  const dark = mode !== "light";
  // Roofs: mostly neutral, one in four in an accent hue, so the theme shows
  // without every block shouting. Every accent at once was a fruit salad.
  const neutral = [mix(bg, fg, dark ? 0.42 : 0.28), mix(bg, fg, dark ? 0.52 : 0.38), mix(bg, fg, dark ? 0.34 : 0.2)];
  const accents = [c.primary, c["ansi-cyan"], c["ansi-yellow"], c["ansi-red"]]
    .filter(Boolean)
    .map((h) => mix(bg, h, dark ? 0.55 : 0.45));
  const hues = [...neutral, ...neutral, ...neutral, ...accents];
  return {
    sky: mix(bg, c["ansi-blue"] ?? fg, dark ? 0.22 : 0.18),
    ground: mix(bg, fg, dark ? 0.12 : 0.1),
    road: mix(bg, fg, dark ? 0.3 : 0.02),
    major: mix(bg, c["ansi-yellow"] ?? c.primary ?? fg, dark ? 0.7 : 0.55),
    water: mix(bg, c["ansi-blue"] ?? fg, dark ? 0.55 : 0.45),
    park: mix(bg, c["ansi-green"] ?? fg, dark ? 0.4 : 0.35),
    tree: mix(bg, c["ansi-green"] ?? fg, dark ? 0.7 : 0.6),
    ink: dark ? mix(bg, "rgb(0,0,0)", 0.55) : mix(fg, bg, 0.2),
    roofs: hues,
    lit: (roof) => mix(roof, bg, 0.22),
    shade: (roof) => mix(roof, dark ? "rgb(0,0,0)" : fg, dark ? 0.45 : 0.3),
  };
}

// Framing: the camera looks at where the buildings are, not at the requested
// point, and moves in when they are compact. A geocoded village centre often
// lands beside the village (Saint-Clar, Montemagno Monferrato), and a fixed
// 650 m camera left a village a smudge on empty ground.
// Centre: the median building centroid. Distance: 2.4 × the radius holding
// 75% of the buildings around it, clamped to 220–650 m. With fewer than 15
// buildings there is nothing to frame and the requested point stays.
function frame(city) {
  const base = groundPlane(city.center);
  const centroids = city.buildings.map((b) => d3.polygonCentroid(b.ring.map(base)));
  if (centroids.length < 15) return { toGround: base, distance: DISTANCE_M };
  const cx = d3.median(centroids, (c) => c[0]);
  const cf = d3.median(centroids, (c) => c[1]);
  const spread = d3.quantile(centroids.map((c) => Math.hypot(c[0] - cx, c[1] - cf)).sort(d3.ascending), 0.75);
  const distance = Math.max(220, Math.min(DISTANCE_M, 2.4 * spread));
  return { toGround: (p) => { const [x, f] = base(p); return [x - cx, f - cf]; }, distance };
}

// A stable small integer from a footprint, so a building keeps its colour.
function hashRing(ring) {
  const [lon, lat] = ring[0];
  return Math.abs(Math.round(lon * 1e6) * 31 + Math.round(lat * 1e6)) >>> 0;
}

export function citySceneSvg(city, width, height, colors, look = LOOKS.planet, mode = "dark") {
  const { toGround, distance } = frame(city);
  const cam = camera(width, height, look, distance);
  const pxPerM = (FOCAL * width) / 2; // ÷ depth gives pixels per metre there
  const pal = palette(colors, mode);
  const flat = (ring) => ring.map((p) => toGround(p));
  const ground = []; // drawn first, in order: areas, road casings, road fills
  const solids = []; // buildings and trees, painter-sorted far to near

  const project = (pts, z = 0) => pts.map(([x, f]) => cam(x, f, z));
  const visible = (pts) => !pts.some((p) => !p) && !offscreen(pts, width, height);
  const line = (pts) => "M" + pts.map((p) => fmt(p[0]) + "," + fmt(p[1])).join("L");
  // Ink thins and fades with distance, the way a pen drawing leaves the
  // background lighter. Constant ink turned dense far blocks (London, Soho)
  // into a tangle of lines.
  const inkAt = (depth, w = 0.5) => {
    const k = Math.min(1, (0.65 * distance) / depth);
    return `stroke="${pal.ink}" stroke-width="${fmt(Math.max(0.15, w * k) * 100 / 100)}" stroke-opacity="${(0.3 + 0.7 * k).toFixed(2)}" stroke-linejoin="round"`;
  };
  const area = (pts) => Math.abs(d3.polygonArea(pts.map((p) => [p[0], p[1]])));

  if (look.curvature) {
    const disc = d3.range(0, 2 * Math.PI, Math.PI / 48)
      .map((a) => cam(1.46 * distance * Math.cos(a), 1.46 * distance * Math.sin(a), 0))
      .filter(Boolean);
    ground.push(`<path d="${polygon(disc)}" fill="${pal.ground}" stroke="${pal.ink}" stroke-width="0.8"/>`);
  }
  for (const [rings, fill] of [[city.parks, pal.park], [city.water, pal.water]]) {
    for (const ring of rings) {
      const pts = project(flat(ring));
      if (visible(pts)) ground.push(`<path d="${polygon(pts)}" fill="${fill}" stroke="${pal.ink}" stroke-width="0.4" stroke-linejoin="round"/>`);
    }
  }
  // Roads: an ink casing under a lighter fill, the classic cartographic stroke.
  // Widths are metres at the road's mean depth, so near streets are wider.
  const roads = [];
  for (const r of city.roads) {
    const pts = project(flat(r.line));
    if (!visible(pts)) continue;
    const w = ((r.major ? 14 : 8) * pxPerM) / d3.mean(pts, (p) => p[2]);
    roads.push({ d: line(pts), w: Math.max(0.6, w), major: r.major });
  }
  roads.sort((a, b) => a.major - b.major);
  for (const r of roads) ground.push(`<path d="${r.d}" fill="none" stroke="${pal.ink}" stroke-width="${fmt(r.w + 1)}" stroke-linecap="round" stroke-linejoin="round"/>`);
  for (const r of roads) ground.push(`<path d="${r.d}" fill="none" stroke="${r.major ? pal.major : pal.road}" stroke-width="${fmt(r.w)}" stroke-linecap="round" stroke-linejoin="round"/>`);

  // Trees: a few circles scattered in each park, sized in metres like the roads.
  for (const ring of city.parks) {
    const poly = flat(ring);
    const [[x0, f0], [x1, f1]] = [d3.extent(poly, (p) => p[0]), d3.extent(poly, (p) => p[1])].map((e, i, a) => (i === 0 ? [a[0][0], a[1][0]] : [a[0][1], a[1][1]]));
    const rand = d3.randomLcg(hashRing(ring) / 2 ** 32);
    const n = Math.min(40, Math.round(((x1 - x0) * (f1 - f0)) / 250));
    for (let i = 0; i < n; i++) {
      const p = [x0 + rand() * (x1 - x0), f0 + rand() * (f1 - f0)];
      if (!d3.polygonContains(poly, p)) continue;
      const top = cam(p[0], p[1], 9 * EXAGGERATION * 0.5);
      const base = cam(p[0], p[1], 0);
      if (!top || !base || offscreen([top], width, height)) continue;
      const r = (5 * pxPerM) / top[2];
      solids.push({ depth: base[2], svg: `<circle cx="${fmt(top[0])}" cy="${fmt(top[1])}" r="${fmt(r)}" fill="${pal.tree}" ${inkAt(top[2], 0.4)}/>` });
    }
  }

  // Buildings: walls in two flat tones by which way they face, then the roof,
  // all outlined in ink.
  for (const b of city.buildings) {
    const gr = flat(b.ring);
    const top = drawnHeight(b.h);
    const base = project(gr);
    const roof = project(gr, top);
    if (base.some((p) => !p) || roof.some((p) => !p)) continue;
    if (offscreen(roof, width, height) && offscreen(base, width, height)) continue;
    // Below ~2 px² a building is texture, not a shape: dropping it keeps far
    // blocks calm and the SVG smaller.
    const hull0 = d3.polygonHull([...base, ...roof].map((p) => [p[0], p[1]]));
    if (!hull0 || area(hull0) < 2) continue;
    const roofColor = pal.roofs[hashRing(b.ring) % pal.roofs.length];
    const walls = [];
    for (let i = 0; i < gr.length - 1; i++) {
      const [x0, f0] = gr[i];
      const [x1, f1] = gr[i + 1];
      // Light from the west-southwest; a wall either faces it or does not.
      const facing = (f1 - f0) * -0.8 + (x1 - x0) * -0.6 > 0;
      walls.push({
        depth: (base[i][2] + base[i + 1][2]) / 2,
        d: polygon([base[i], base[i + 1], roof[i + 1], roof[i]]),
        fill: facing ? pal.lit(roofColor) : pal.shade(roofColor),
      });
    }
    walls.sort((a, b) => b.depth - a.depth);
    // Ink on the roof and the silhouette only: outlining every wall striped
    // round towers with dozens of vertical lines. Walls get a hairline in their
    // own colour so neighbouring faces don't show a seam.
    const ink = inkAt(d3.mean(base, (p) => p[2]));
    const hull = hull0;
    solids.push({
      depth: d3.max(base, (p) => p[2]),
      svg:
        walls.map((w) => `<path d="${w.d}" fill="${w.fill}" stroke="${w.fill}" stroke-width="0.3"/>`).join("") +
        `<path d="${polygon(roof)}" fill="${roofColor}" ${ink}/>` +
        (hull ? `<path d="${polygon(hull)}" fill="none" ${ink}/>` : ""),
    });
  }

  // DECISION: sort per building, by farthest base corner. Checked at 2× card
  // size on the four sampled cities (Manhattan towers, Eixample blocks, Gothic
  // alleys, Budapest centre) at PITCH_DEG 50: no visible overlap errors. Rejected:
  // sorting per wall — more work, and no visible error for it to fix at 248×96.
  // A steeper pitch or a larger card reopens this.
  solids.sort((a, b) => b.depth - a.depth);

  return (
    `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}">` +
    `<rect width="${width}" height="${height}" fill="${look.curvature ? pal.sky : pal.ground}"/>` +
    ground.join("") +
    solids.map((p) => p.svg).join("") +
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
// Those numbers are the first, shaded renderer. The illustration style draws
// more per building (road casings, wall hairlines, a silhouette): measured
// 2026-10-24, build 84 ms (Alcover) to 792 ms (London, Soho), SVG 0.4–4.2 MB,
// per-tick eval as a data URI 2.6–31 ms. Every decision below holds, by a
// wider margin: the data URI is worse, and the build is off the loop anyway.
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
//
// DECISION (Q2/Q3, answered): the scene is a <D3View> (src/d3_view.rs), drawn
// in the draw runtime off the render loop. The 35–670 ms build no longer
// freezes anything; until it finishes the card shows the previous scene or
// none (P2/P3 for free). Rejected: building inside the render call.
//
// The tracer cannot exercise <D3View> yet, so it keeps the data URI to render
// at all; the numbers above are why that cannot ship.
const memo = new Map();

export function citySceneUri(city, width, height, colors, look = LOOKS.planet, mode = "dark") {
  const key = `${city.center}|${width}x${height}|${colors.background}|${colors.foreground}|${JSON.stringify(look)}|${mode}`;
  let uri = memo.get(key);
  if (!uri) {
    uri = "data:image/svg+xml;utf8," + encodeURIComponent(citySceneSvg(city, width, height, colors, look, mode));
    memo.set(key, uri);
  }
  return uri;
}

// SPEC (P1, needs <D3View>):
//   export default function CityScene({ city, width, height, look }) {
//     if (!city) return null;                                    // P2
//     return <D3View key={sceneKey(city.center, width, height, ctx.theme.name, look)}
//                    width={width} height={height}
//                    draw={(svg) => drawCityScene(svg, city, ctx.theme.colors, look)} />;
//   }
// where drawCityScene appends the parts below with d3 instead of joining strings.
export default function CityScene({ city, width, height, look }) {
  return (
    <img
      src={citySceneUri(city, width, height, ctx.theme.colors, look, ctx.theme.mode)}
      style={{ position: "absolute", top: 0, left: 0, width, height }}
    />
  );
}
