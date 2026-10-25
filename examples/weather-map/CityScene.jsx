// CityScene: a few city blocks around a point, drawn as a tilted 3D view with
// building heights exaggerated, meant to sit behind a card's text.
//
// An example that exercises <D3Figure>, not a shipped widget.
// Map data © OpenStreetMap contributors (ODbL).
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
//   P1 happy — the card shows the scene for its location.
//      Needs: coordinates, cached geometry, a scene built off the per-tick
//      path, pixels drawn once.
//   P2 no scene — first run (Overpass takes ~40 s), offline, Overpass refusing.
//      The card renders without a scene, no gap, no error.
//   P3 location moves — new coordinates → new geometry → new scene; the old one
//      stays up until the new one is ready.
//   P4 theme switch — same geometry, new colours; no refetch.
//   P5 the browser renderer (ADR 0026) — not in the first step, in the first
//      merge (Q5, answered; see src/d3_figure/mod.rs).
//
// SPEC (P2): with no City geometry yet, CityScene renders nothing and the card
// keeps its normal background.
// SPEC (P3/P4): a scene is keyed by (geometry, size, every theme colour, look,
// mode); any other change reuses the existing one. "Geometry" is the geometry
// itself, not its centre: a refetch at the same point is a new scene.
//
// DECISION (Q2/Q3, answered): the scene is a <D3Figure>, drawn in the draw
// runtime off the render loop and shown as a Picture. Why not a data URI in
// `<img src>`, and why not build it in the render call, is measured in
// docs/adr/0044 — this file's first version did both. Until a drawing is ready
// the card shows the previous scene or none (P2/P3 for free).
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
// Pipeline, all of it once per key — in `draw`, which tauler runs once per key:
//   1. lon/lat → metres on a ground plane: d3.geoMercator scaled to 1 unit = 1 m
//   2. metres → camera space: pitch, distance, perspective divide (`project`)
//   3. buildings → wall quads + roof, painter's algorithm (far first)
//   4. everything → appended to the figure's <svg> with d3, in the
//      illustration style, theme colours
//
// `"d3"` is resolved by tauler itself (downloaded and pinned on first use).
import * as d3 from "d3";

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


// Steps 1–3: everything the scene shows, as shapes in screen space and in the
// order they are painted. No SVG here; `drawCityScene` turns it into elements.
function sceneShapes(city, width, height, look) {
  const { toGround, distance } = frame(city);
  const cam = camera(width, height, look, distance);
  const pxPerM = (FOCAL * width) / 2; // ÷ depth gives pixels per metre there
  const flat = (ring) => ring.map((p) => toGround(p));
  const project = (pts, z = 0) => pts.map(([x, f]) => cam(x, f, z));
  const visible = (pts) => !pts.some((p) => !p) && !offscreen(pts, width, height);
  // Ink thins and fades with distance, the way a pen drawing leaves the
  // background lighter. Constant ink turned the far blocks of the densest
  // samples into a tangle of lines.
  const inkAt = (depth, w = 0.5) => {
    const k = Math.min(1, (0.65 * distance) / depth);
    return { width: fmt(Math.max(0.15, w * k)), opacity: (0.3 + 0.7 * k).toFixed(2) };
  };
  const area = (pts) => Math.abs(d3.polygonArea(pts.map((p) => [p[0], p[1]])));

  const disc = look.curvature
    ? d3.range(0, 2 * Math.PI, Math.PI / 48)
        .map((a) => cam(1.46 * distance * Math.cos(a), 1.46 * distance * Math.sin(a), 0))
        .filter(Boolean)
    : null;

  const areas = []; // parks, then water
  for (const [rings, kind] of [[city.parks, "park"], [city.water, "water"]]) {
    for (const ring of rings) {
      const pts = project(flat(ring));
      if (visible(pts)) areas.push({ d: polygon(pts), kind });
    }
  }

  // Roads: widths are metres at the road's mean depth, so near streets are
  // wider. Minor first, so major roads cross over them.
  const roads = [];
  for (const r of city.roads) {
    const pts = project(flat(r.line));
    if (!visible(pts)) continue;
    const w = ((r.major ? 14 : 8) * pxPerM) / d3.mean(pts, (p) => p[2]);
    roads.push({ d: "M" + pts.map((p) => fmt(p[0]) + "," + fmt(p[1])).join("L"), w: Math.max(0.6, w), major: r.major });
  }
  roads.sort((a, b) => a.major - b.major);

  const solids = []; // trees and buildings, painter-sorted far to near

  // Trees: a few circles scattered in each park, sized in metres like the roads.
  for (const ring of city.parks) {
    const poly = flat(ring);
    const [x0, x1] = d3.extent(poly, (p) => p[0]);
    const [f0, f1] = d3.extent(poly, (p) => p[1]);
    const rand = d3.randomLcg(hashRing(ring) / 2 ** 32);
    const n = Math.min(40, Math.round(((x1 - x0) * (f1 - f0)) / 250));
    for (let i = 0; i < n; i++) {
      const p = [x0 + rand() * (x1 - x0), f0 + rand() * (f1 - f0)];
      if (!d3.polygonContains(poly, p)) continue;
      const top = cam(p[0], p[1], 9 * EXAGGERATION * 0.5);
      const base = cam(p[0], p[1], 0);
      if (!top || !base || offscreen([top], width, height)) continue;
      solids.push({
        tree: { cx: fmt(top[0]), cy: fmt(top[1]), r: fmt((5 * pxPerM) / top[2]) },
        depth: base[2],
        ink: inkAt(top[2], 0.4),
      });
    }
  }

  // Buildings: walls in two flat tones by which way they face, then the roof.
  for (const b of city.buildings) {
    const gr = flat(b.ring);
    const base = project(gr);
    const roof = project(gr, drawnHeight(b.h));
    if (base.some((p) => !p) || roof.some((p) => !p)) continue;
    if (offscreen(roof, width, height) && offscreen(base, width, height)) continue;
    // Below ~2 px² a building is texture, not a shape: dropping it keeps far
    // blocks calm and the SVG smaller.
    const hull = d3.polygonHull([...base, ...roof].map((p) => [p[0], p[1]]));
    if (!hull || area(hull) < 2) continue;
    const walls = [];
    for (let i = 0; i < gr.length - 1; i++) {
      const [x0, f0] = gr[i];
      const [x1, f1] = gr[i + 1];
      walls.push({
        depth: (base[i][2] + base[i + 1][2]) / 2,
        d: polygon([base[i], base[i + 1], roof[i + 1], roof[i]]),
        // Light from the west-southwest; a wall either faces it or does not.
        lit: (f1 - f0) * -0.8 + (x1 - x0) * -0.6 > 0,
      });
    }
    walls.sort((a, b) => b.depth - a.depth);
    solids.push({
      building: { walls, roof: polygon(roof), hull: polygon(hull), tone: hashRing(b.ring) },
      depth: d3.max(base, (p) => p[2]),
      ink: inkAt(d3.mean(base, (p) => p[2])),
    });
  }

  // DECISION: sort per building, by farthest base corner. Checked at 2× card
  // size on every sampled place (towers, grid blocks, old-town alleys, a
  // suburb, villages) at PITCH_DEG 50: no visible overlap errors. Rejected:
  // sorting per wall — more work, and no visible error for it to fix at 248×96.
  // A steeper pitch or a larger card reopens this.
  solids.sort((a, b) => b.depth - a.depth);

  return { disc: disc && polygon(disc), areas, roads, solids };
}

// Step 4: the shapes as elements of the figure's <svg>, in the illustration
// style. Every colour comes from `palette`.
export function drawCityScene(svg, city, width, height, colors, look = LOOKS.planet, mode = "dark") {
  const pal = palette(colors, mode);
  const { disc, areas, roads, solids } = sceneShapes(city, width, height, look);
  const inked = (sel) =>
    sel.attr("stroke", pal.ink)
      .attr("stroke-width", (s) => s.ink.width)
      .attr("stroke-opacity", (s) => s.ink.opacity)
      .attr("stroke-linejoin", "round");

  svg.append("rect").attr("width", width).attr("height", height)
    .attr("fill", look.curvature ? pal.sky : pal.ground);
  if (disc) {
    svg.append("path").attr("d", disc).attr("fill", pal.ground)
      .attr("stroke", pal.ink).attr("stroke-width", 0.8);
  }
  svg.append("g").attr("stroke", pal.ink).attr("stroke-width", 0.4).attr("stroke-linejoin", "round")
    .selectAll("path").data(areas).join("path")
    .attr("d", (a) => a.d).attr("fill", (a) => pal[a.kind]);

  // Roads: an ink casing under a lighter fill, the classic cartographic stroke.
  const road = svg.append("g").attr("fill", "none")
    .attr("stroke-linecap", "round").attr("stroke-linejoin", "round");
  road.append("g").attr("stroke", pal.ink)
    .selectAll("path").data(roads).join("path")
    .attr("d", (r) => r.d).attr("stroke-width", (r) => fmt(r.w + 1));
  road.append("g")
    .selectAll("path").data(roads).join("path")
    .attr("d", (r) => r.d).attr("stroke-width", (r) => fmt(r.w))
    .attr("stroke", (r) => (r.major ? pal.major : pal.road));

  // Trees and buildings share one painter order, so each is its own group.
  const solid = svg.append("g").selectAll("g").data(solids).join("g");
  solid.filter((s) => s.tree).append("circle")
    .attr("cx", (s) => s.tree.cx).attr("cy", (s) => s.tree.cy).attr("r", (s) => s.tree.r)
    .attr("fill", pal.tree)
    .call(inked);
  const building = solid.filter((s) => s.building);
  const roofColor = (s) => pal.roofs[s.building.tone % pal.roofs.length];
  // Ink on the roof and the silhouette only: outlining every wall striped
  // round towers with dozens of vertical lines. Walls get a hairline in their
  // own colour so neighbouring faces don't show a seam.
  building.selectAll("path")
    .data((s) => s.building.walls.map((w) => ({ d: w.d, fill: (w.lit ? pal.lit : pal.shade)(roofColor(s)) })))
    .join("path")
    .attr("d", (w) => w.d).attr("fill", (w) => w.fill)
    .attr("stroke", (w) => w.fill).attr("stroke-width", 0.3);
  building.append("path").attr("d", (s) => s.building.roof).attr("fill", roofColor).call(inked);
  building.append("path").attr("d", (s) => s.building.hull).attr("fill", "none").call(inked);
}

// FNV-1a over a value's JSON: the scene's key is made of these.
function hashOf(value) {
  const text = JSON.stringify(value);
  let hash = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) {
    hash ^= text.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash.toString(36);
}

// The geometry's part of the key: its content, not its identity or its centre.
// The render runtime and the draw runtime each load the geometry and must agree
// on the key, and a refetch at the same point is a new scene. Hashed once per
// geometry object (0.1–0.6 MB of JSON), not once per tick.
const geometryKeys = new WeakMap();

function geometryKey(city) {
  let key = geometryKeys.get(city);
  if (key === undefined) geometryKeys.set(city, (key = hashOf(city)));
  return key;
}

export default function CityScene({ city, width, height, look = LOOKS.planet }) {
  if (!city) return null; // P2
  const { colors, mode } = ctx.theme;
  return (
    <D3Figure
      key={`city/${geometryKey(city)}/${width}x${height}/${hashOf([colors, look, mode])}`}
      width={width}
      height={height}
      draw={(svg) => drawCityScene(svg, city, width, height, colors, look, mode)}
    />
  );
}
