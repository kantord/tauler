#!/usr/bin/env bash
# Fetch one city block from OpenStreetMap and write it as a CityGeometry module.
#
#   ./fetch-city.sh <name> <lat> <lon> [radius_m]   →   .data/<name>.js
#
# Tracer tooling, not the shipped fetcher. The shipped one is the subprocess
# described in CityScene.jsx; this script exists so the tracer has real
# geometry from several kinds of city to measure against.
#
# Geometry comes from the Overpass API, fetched once per location and cached on
# disk: at radius 500 m a request takes ~40 s and returns 0.4–1.8 MB of JSON,
# and Overpass rate-limits per IP, so fetching on every bar start is not an
# option.
#
# A building's height is `height`, else `building:levels` × 3 m, else 3 levels.
# Few buildings outside city centres carry either tag (Manhattan 84%, Venice
# 38%, suburbs and villages 0–19%), so the fallback draws most of every suburb.
#
# Arguments go to node after `--`: a southern latitude (-37.8) would otherwise
# be read as a node option.
#
# The main OSM API (`/api/0.6/map`) is the fallback when Overpass cannot be
# reached: same data, raw XML, a bounding box instead of a radius, at most
# 50 000 nodes — enough for a 500 m box outside the densest old towns. Only for
# a handful of tracer fetches; the OSM API's usage policy rules out a widget
# calling it.
set -euo pipefail
name=$1 lat=$2 lon=$3 r=${4:-500}
# `name` becomes a file name under .data/: nothing that could leave it.
[[ $name =~ ^[a-z0-9-]+$ ]] || { echo "name must be [a-z0-9-]+, got: $name" >&2; exit 2; }
for n in "$lat" "$lon" "$r"; do
  [[ $n =~ ^-?[0-9]+(\.[0-9]+)?$ ]] || { echo "not a number: $n" >&2; exit 2; }
done
here=$(dirname "$0")
mkdir -p "$here/.data"
raw=$(mktemp)
trap 'rm -f "$raw" "$raw.js"' EXIT
ua="tauler-weather-map-tracer"
q="[out:json][timeout:60];(way[building](around:$r,$lat,$lon);way[highway~\"^(motorway|trunk|primary|secondary|tertiary|residential|unclassified|pedestrian|living_street)$\"](around:$r,$lat,$lon);way[natural=water](around:$r,$lat,$lon);way[leisure=park](around:$r,$lat,$lon););out geom tags qt;"
# An Overpass reply counts only if it parses and carries no `remark`: a query
# that times out or runs out of memory still answers 200 with whatever it had
# gathered, and says so only in `remark` — cached as-is, that is a city with
# half its buildings missing for good.
overpass_ok() {
  node -e 'const j = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"));
           if (j.remark || !Array.isArray(j.elements)) { console.error("overpass: " + (j.remark ?? "no elements")); process.exit(1); }' -- "$raw" 2>&1
}
if ! { curl -sf --max-time 90 -A "$ua" --data-urlencode "data=$q" https://overpass-api.de/api/interpreter > "$raw" && overpass_ok; }; then
  bbox=$(node -e 'const [la,lo,r]=process.argv.slice(1).map(Number);const dla=r/111320,dlo=r/(111320*Math.cos(la*Math.PI/180));console.log([lo-dlo,la-dla,lo+dlo,la+dla].map(n=>n.toFixed(6)).join(","))' -- "$lat" "$lon" "$r")
  curl -sf --max-time 90 -A "$ua" "https://api.openstreetmap.org/api/0.6/map?bbox=$bbox" > "$raw"
fi
node -e '
  const fs = require("fs");
  const [file, lat, lon] = [process.argv[1], +process.argv[2], +process.argv[3]];
  const s = fs.readFileSync(file, "utf8");
  // Both sources become Overpass-shaped elements: {tags, geometry: [{lat, lon}]}.
  let elements;
  if (s.trimStart().startsWith("{")) elements = JSON.parse(s).elements;
  else {
    const nodes = new Map();
    for (const m of s.matchAll(/<node id="(\d+)"[^>]*?lat="([-\d.]+)" lon="([-\d.]+)"/g)) nodes.set(m[1], { lat: +m[2], lon: +m[3] });
    elements = [];
    for (const m of s.matchAll(/<way id="\d+"[^>]*>([\s\S]*?)<\/way>/g)) {
      const geometry = [...m[1].matchAll(/<nd ref="(\d+)"/g)].map(n => nodes.get(n[1])).filter(Boolean);
      const tags = Object.fromEntries([...m[1].matchAll(/<tag k="([^"]*)" v="([^"]*)"/g)].map(t => [t[1], t[2]]));
      elements.push({ tags, geometry });
    }
  }
  const r6 = n => Math.round(n * 1e6) / 1e6;
  const pts = g => g.map(p => [r6(p.lon), r6(p.lat)]);
  const minor = /^(tertiary|residential|unclassified|pedestrian|living_street)$/;
  const major = /^(motorway|trunk|primary|secondary)$/;
  const out = { center: [lon, lat], buildings: [], roads: [], water: [], parks: [] };
  for (const e of elements) {
    if (!e.geometry || e.geometry.length < 2 || !e.tags) continue;
    const t = e.tags;
    if (t.building) {
      const h = parseFloat(t.height) || (parseFloat(t["building:levels"]) * 3) || 9;
      out.buildings.push({ h, ring: pts(e.geometry) });
    } else if (major.test(t.highway ?? "") || minor.test(t.highway ?? "")) {
      out.roads.push({ major: major.test(t.highway), line: pts(e.geometry) });
    } else if (t.natural === "water") out.water.push(pts(e.geometry));
    else if (t.leisure === "park") out.parks.push(pts(e.geometry));
  }
  if (!out.buildings.length && !out.roads.length) { console.error("no geometry"); process.exit(1); }
  const tagged = elements.filter(e => e.tags?.building && (e.tags.height || e.tags["building:levels"])).length;
  console.error(`buildings ${out.buildings.length}, with height tags ${tagged} (${Math.round(100 * tagged / Math.max(1, out.buildings.length))}%), ways ${elements.length}`);
  process.stdout.write("export default " + JSON.stringify(out) + ";\n");
' -- "$raw" "$lat" "$lon" > "$raw.js"
mv "$raw.js" "$here/.data/$name.js"
echo "wrote $here/.data/$name.js ($(stat -c%s "$here/.data/$name.js") bytes)"
