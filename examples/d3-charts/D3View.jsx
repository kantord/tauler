// Stand-in for the planned <D3View> node (src/d3_view.rs), so these examples can
// be written against its API before it exists:
//
//   <D3View key="cpu/1712" width={248} height={110} draw={(svg, d3) => { ... }} />
//
// What it does differently from the real thing: `draw` runs right here, inside
// the render call, and the result travels as an SVG data URI. Both are exactly
// what src/d3_view.rs moves off the render loop; the author's code does not
// change when it does.
//
// `.data/` is gitignored. Regenerate the bundle (d3 7 + linkedom 0.18, both ISC):
//   printf 'export * as d3 from "d3";\nexport { parseHTML } from "linkedom";\n' > entry.js
//   npm i d3@7 linkedom@0.18 esbuild && npx esbuild entry.js --bundle --format=esm \
//     --minify --outfile=examples/d3-charts/.data/d3dom.js
import { d3, parseHTML } from "./.data/d3dom.js";

const { document } = parseHTML("<!doctype html><html><body></body></html>");

// Timers that never fire. d3-timer schedules a frame as soon as anything starts
// one — `d3.forceSimulation()` does so on construction, before `.stop()` can
// be called — and QuickJS has no `setTimeout`, so the network graph threw
// "setTimeout is not defined". A drawing is rasterized once, so nothing should
// ever wait for a frame: a simulation is run to rest with `.tick()`, and a
// transition simply never plays.
for (const name of ["setTimeout", "setInterval", "requestAnimationFrame"]) {
  globalThis[name] ??= () => 0;
}
for (const name of ["clearTimeout", "clearInterval", "cancelAnimationFrame"]) {
  globalThis[name] ??= () => {};
}
const drawn = new Map();

export function drawSvg(width, height, draw) {
  const svg = d3
    .select(document.body)
    .append("svg")
    .attr("xmlns", "http://www.w3.org/2000/svg")
    .attr("width", width)
    .attr("height", height)
    .attr("viewBox", `0 0 ${width} ${height}`)
    .attr("font-family", "sans-serif");
  draw(svg, d3);
  const markup = svg.node().outerHTML;
  svg.remove();
  return markup;
}

export default function D3View({ key, width, height, draw }) {
  let uri = drawn.get(key);
  if (!uri) {
    uri = "data:image/svg+xml;utf8," + encodeURIComponent(drawSvg(width, height, draw));
    drawn.set(key, uri);
  }
  return <img src={uri} style={{ width, height }} />;
}
