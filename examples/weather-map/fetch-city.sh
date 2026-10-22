#!/usr/bin/env bash
# Fetch one city block from OpenStreetMap and write it as a CityGeometry module.
#
#   ./fetch-city.sh <name> <lat> <lon> [radius_m]   →   .data/<name>.js
#
# Tracer tooling, not the shipped fetcher. The shipped one is the subprocess
# described in CityScene.jsx; this script exists so the tracer has real
# geometry from several kinds of city to measure against.
#
# DECISION: geometry comes from the Overpass API, fetched once per location and
# cached on disk. Measured on 2026-10-23, radius 500 m: 38–42 s per request,
# 0.4–1.8 MB of JSON, 437–1882 ways (Barcelona Gothic quarter is the worst case
# sampled). Rejected: fetching on every bar start — 40 s with no scene, and
# Overpass rate-limits per IP.
#
# DECISION: a building's height is `height`, else `building:levels` × 3 m, else
# 3 levels. Measured coverage of either tag: Manhattan 83%, Eixample 55%,
# Gothic 95%, central Budapest 26% — the fallback carries a quarter of some cities.
set -euo pipefail
name=$1 lat=$2 lon=$3 r=${4:-500}
here=$(dirname "$0")
mkdir -p "$here/.data"
q="[out:json][timeout:60];(way[building](around:$r,$lat,$lon);way[highway~\"^(motorway|trunk|primary|secondary|tertiary|residential|unclassified|pedestrian|living_street)$\"](around:$r,$lat,$lon);way[natural=water](around:$r,$lat,$lon);way[leisure=park](around:$r,$lat,$lon););out geom tags qt;"
curl -s --max-time 90 --data-urlencode "data=$q" https://overpass-api.de/api/interpreter |
  node -e '
    const [lat, lon] = process.argv.slice(1).map(Number);
    let s = ""; process.stdin.on("data", d => s += d).on("end", () => {
      const r6 = n => Math.round(n * 1e6) / 1e6;
      const pts = g => g.map(p => [r6(p.lon), r6(p.lat)]);
      const out = { center: [lon, lat], buildings: [], roads: [], water: [], parks: [] };
      for (const e of JSON.parse(s).elements) {
        if (!e.geometry || !e.tags) continue;
        const t = e.tags;
        if (t.building) {
          const h = parseFloat(t.height) || (parseFloat(t["building:levels"]) * 3) || 9;
          out.buildings.push({ h, ring: pts(e.geometry) });
        } else if (t.highway) {
          const major = /^(motorway|trunk|primary|secondary)$/.test(t.highway);
          out.roads.push({ major, line: pts(e.geometry) });
        } else if (t.natural === "water") out.water.push(pts(e.geometry));
        else if (t.leisure === "park") out.parks.push(pts(e.geometry));
      }
      process.stdout.write("export default " + JSON.stringify(out) + ";\n");
    });' "$lat" "$lon" > "$here/.data/$name.js"
echo "wrote $here/.data/$name.js ($(stat -c%s "$here/.data/$name.js") bytes)"
