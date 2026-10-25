#!/usr/bin/env bash
# Write .data/crates.js: tauler's workspace crates and their direct dependencies
# (normal ones — no dev or build), as {nodes: [{id, group}], links: [{source, target}]}
# for the network graph example. Run from anywhere inside the repository.
set -euo pipefail
here=$(dirname "$0")
mkdir -p "$here/.data"
tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT
cargo metadata --format-version 1 --no-deps | node -e '
  let s = ""; process.stdin.on("data", d => s += d).on("end", () => {
    const m = JSON.parse(s);
    const workspace = new Set(m.packages.map(p => p.name));
    const nodes = [], links = [], seen = new Set(), linked = new Set();
    const add = (id, group) => { if (!seen.has(id)) { seen.add(id); nodes.push({ id, group }); } };
    for (const p of m.packages) {
      add(p.name, "workspace");
      for (const d of p.dependencies) {
        if (d.kind) continue; // "dev" or "build"; normal dependencies have none
        add(d.name, workspace.has(d.name) ? "workspace" : "external");
        // A dependency declared under several `[target.cfg(...)]` tables is
        // listed once per table; one edge is enough.
        if (linked.has(p.name + "\0" + d.name)) continue;
        linked.add(p.name + "\0" + d.name);
        links.push({ source: p.name, target: d.name });
      }
    }
    process.stdout.write("export default " + JSON.stringify({ nodes, links }) + ";\n");
    console.error(`${nodes.length} nodes, ${links.length} links`);
  });' > "$tmp"
mv "$tmp" "$here/.data/crates.js"
