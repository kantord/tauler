// Bar chart: focused hours per day this week. A data join of bars, value
// labels on each, today in the accent colour, the week's average as a dashed
// rule.
import { contentKey } from "./keys.js";
import { chartTheme, themeKey } from "./theme.js";

export default function BarChart({ days, today, width, height }) {
  // No data, no chart: `d3.mean([])` is undefined and the average label
  // would throw on `.toFixed`.
  if (!days?.length) return null;
  const t = chartTheme(ctx.theme);
  return (
    <D3Figure
      key={`bar/${themeKey(ctx.theme)}/${contentKey(days)}/${today}`}
      width={width}
      height={height}
      draw={(svg, d3) => {
        const m = { top: 12, right: 6, bottom: 14, left: 6 };
        const x = d3.scaleBand().domain(days.map((d) => d.day)).range([m.left, width - m.right]).padding(0.28);
        const y = d3.scaleLinear().domain([0, d3.max(days, (d) => d.hours)]).nice().range([height - m.bottom, m.top]);
        const avg = d3.mean(days, (d) => d.hours);
        const rest = t.mix(t.background, t.foreground, 0.32);

        svg.append("g").selectAll("rect").data(days).join("rect")
          .attr("x", (d) => x(d.day)).attr("width", x.bandwidth())
          .attr("y", (d) => y(d.hours)).attr("height", (d) => y(0) - y(d.hours))
          .attr("rx", 2)
          .attr("fill", (d) => (d.day === today ? t.series[0] : rest));

        svg.append("g").selectAll("text").data(days).join("text")
          .attr("x", (d) => x(d.day) + x.bandwidth() / 2).attr("y", (d) => y(d.hours) - 3)
          .attr("text-anchor", "middle").attr("font-size", 7)
          .attr("fill", (d) => (d.day === today ? t.series[0] : t.axis))
          .text((d) => d.hours.toFixed(1));

        svg.append("line")
          .attr("x1", m.left).attr("x2", width - m.right).attr("y1", y(avg)).attr("y2", y(avg))
          .attr("stroke", t.series[3]).attr("stroke-width", 0.8).attr("stroke-dasharray", "3 2");
        svg.append("text").attr("x", width - m.right).attr("y", y(avg) - 2)
          .attr("text-anchor", "end").attr("font-size", 6.5).attr("fill", t.series[3])
          .text(`avg ${avg.toFixed(1)} h`);

        svg.append("g")
          .attr("transform", `translate(0,${height - m.bottom})`)
          .call(d3.axisBottom(x).tickSize(0).tickPadding(4))
          .call((g) => g.select(".domain").attr("stroke", t.grid))
          .call((g) => g.selectAll(".tick text").attr("fill", t.axis).attr("font-size", 7));
      }}
    />
  );
}
