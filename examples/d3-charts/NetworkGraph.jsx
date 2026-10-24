// Network graph: tauler's own crates and what they depend on, laid out by
// d3-force. Workspace crates are larger, in the accent colour and labelled;
// external crates are small neutral dots. The simulation runs to rest inside
// `draw` — there is no animation in a drawing that is rasterized once.
//
// The simulation is the expensive part: 300 ticks of this 60-node graph took
// ~700 ms in QuickJS against 60 ms in Node (tracer, 2026-10-24) — the line and
// bar charts draw in 4.3 and 1.8 ms. So it cools faster (ALPHA_DECAY: 135
// ticks instead of d3's default 300; ~390 ms, Node 23 ms), and the settled
// layout is kept per graph: a theme switch redraws without simulating again.
import D3View from "./D3View.jsx";
import { chartTheme, themeKey } from "./theme.js";

const ALPHA_DECAY = 0.05;
const settled = new Map();

function layout(graph, width, height, d3) {
  const key = `${graph.nodes.length}/${graph.links.length}/${width}x${height}`;
  if (settled.has(key)) return settled.get(key);
  // d3-force mutates its input; work on copies so the data stays plain.
  const nodes = graph.nodes.map((n) => ({ ...n }));
  const links = graph.links.map((l) => ({ ...l }));
  const inner = (n) => n.group === "workspace";
  const degree = d3.rollup(links.flatMap((l) => [l.source, l.target]), (v) => v.length, (id) => id);
  const r = (n) => (inner(n) ? 3.2 : 1.6) + Math.sqrt(degree.get(n.id) ?? 0) * 0.5;
  const sim = d3.forceSimulation(nodes)
    .alphaDecay(ALPHA_DECAY)
    .force("link", d3.forceLink(links).id((n) => n.id).distance((l) => (inner(l.target) ? 26 : 16)).strength(0.6))
    .force("charge", d3.forceManyBody().strength((n) => (inner(n) ? -60 : -14)))
    .force("x", d3.forceX(width / 2).strength(0.06))
    // The card is wide and short: pull harder vertically.
    .force("y", d3.forceY(height / 2).strength(0.16))
    // Labelled nodes keep a wider berth, so their names have room.
    .force("collide", d3.forceCollide((n) => r(n) + (inner(n) ? 9 : 1)))
    .stop();
  while (sim.alpha() > sim.alphaMin()) sim.tick();
  const result = { nodes, links, inner, r };
  settled.set(key, result);
  return result;
}

export default function NetworkGraph({ graph, width, height }) {
  const t = chartTheme(ctx.theme);
  return (
    <D3View
      key={`net/${themeKey(ctx.theme)}/${graph.nodes.length}/${graph.links.length}`}
      width={width}
      height={height}
      draw={(svg, d3) => {
        const { nodes, links, inner, r } = layout(graph, width, height, d3);

        // Fit whatever shape the layout settled into to the card.
        const pad = 10;
        const fx = d3.scaleLinear().domain(d3.extent(nodes, (n) => n.x)).range([pad, width - pad]);
        const fy = d3.scaleLinear().domain(d3.extent(nodes, (n) => n.y)).range([pad, height - pad]);

        svg.append("g").attr("stroke", t.grid).attr("stroke-width", 0.6)
          .selectAll("line").data(links).join("line")
          .attr("x1", (l) => fx(l.source.x)).attr("y1", (l) => fy(l.source.y))
          .attr("x2", (l) => fx(l.target.x)).attr("y2", (l) => fy(l.target.y))
          .attr("stroke", (l) => (inner(l.target) ? t.mix(t.background, t.series[0], 0.55) : t.grid));

        svg.append("g").selectAll("circle").data(nodes).join("circle")
          .attr("cx", (n) => fx(n.x)).attr("cy", (n) => fy(n.y)).attr("r", r)
          .attr("fill", (n) => (inner(n) ? t.series[0] : t.mix(t.background, t.foreground, 0.4)))
          .attr("stroke", t.background).attr("stroke-width", 0.6);

        svg.append("g").selectAll("text").data(nodes.filter(inner)).join("text")
          .attr("x", (n) => fx(n.x)).attr("y", (n) => fy(n.y) - r(n) - 1.5)
          .attr("text-anchor", "middle").attr("font-size", 6.5).attr("font-weight", 600)
          .attr("fill", t.foreground).attr("paint-order", "stroke")
          .attr("stroke", t.background).attr("stroke-width", 2)
          .text((n) => n.id.replace(/^tauler-/, "") || n.id);
      }}
    />
  );
}
