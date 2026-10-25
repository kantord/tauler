// Line chart: CPU and memory over the last ten minutes, the kind of history
// `tauler-accumulate` produces (ADR 0014). Area under CPU, gridlines, a time
// axis, and the latest reading called out at the right edge.
import { contentKey } from "./keys.js";
import { chartTheme, themeKey } from "./theme.js";

export default function LineChart({ samples, width, height }) {
  if (!samples?.length) return null;
  const t = chartTheme(ctx.theme);
  const last = samples[samples.length - 1];
  return (
    <D3Figure
      key={`line/${themeKey(ctx.theme)}/${contentKey(samples)}`}
      width={width}
      height={height}
      draw={(svg, d3) => {
        const m = { top: 14, right: 30, bottom: 16, left: 24 };
        const x = d3.scaleTime().domain(d3.extent(samples, (d) => d.t)).range([m.left, width - m.right]);
        const y = d3.scaleLinear().domain([0, 100]).range([height - m.bottom, m.top]);

        const fill = svg.append("defs").append("linearGradient")
          .attr("id", "cpu-fill").attr("x1", 0).attr("y1", 0).attr("x2", 0).attr("y2", 1);
        fill.append("stop").attr("offset", 0).attr("stop-color", t.series[1]).attr("stop-opacity", 0.45);
        fill.append("stop").attr("offset", 1).attr("stop-color", t.series[1]).attr("stop-opacity", 0);

        svg.append("g")
          .attr("transform", `translate(${m.left},0)`)
          .call(d3.axisLeft(y).ticks(3).tickSize(-(width - m.left - m.right)).tickFormat((v) => `${v}%`))
          .call((g) => g.select(".domain").remove())
          .call((g) => g.selectAll(".tick line").attr("stroke", t.grid))
          .call((g) => g.selectAll(".tick text").attr("fill", t.axis).attr("font-size", 7));
        svg.append("g")
          .attr("transform", `translate(0,${height - m.bottom})`)
          .call(d3.axisBottom(x).ticks(4).tickSize(0).tickPadding(5).tickFormat(d3.timeFormat("%H:%M")))
          .call((g) => g.select(".domain").attr("stroke", t.axis))
          .call((g) => g.selectAll(".tick text").attr("fill", t.axis).attr("font-size", 7));

        const curve = d3.curveMonotoneX;
        svg.append("path").datum(samples).attr("fill", "url(#cpu-fill)")
          .attr("d", d3.area().x((d) => x(d.t)).y0(y(0)).y1((d) => y(d.cpu)).curve(curve));
        const series = [
          { key: "cpu", label: "cpu", color: t.series[1], width: 1.6 },
          { key: "mem", label: "mem", color: t.series[2], width: 1.2 },
        ];
        svg.selectAll(".series").data(series).join("path")
          .attr("fill", "none").attr("stroke", (s) => s.color).attr("stroke-width", (s) => s.width)
          .attr("d", (s) => d3.line().x((d) => x(d.t)).y((d) => y(d[s.key])).curve(curve)(samples));

        // Latest reading: a dot on each line, the value beside it.
        // Labels closer than 9 px are pushed apart, the higher one up.
        const ys = series.map((s) => y(last[s.key]));
        if (Math.abs(ys[0] - ys[1]) < 9) {
          const mid = (ys[0] + ys[1]) / 2;
          const up = ys[0] < ys[1] ? 0 : 1;
          ys[up] = mid - 4.5;
          ys[1 - up] = mid + 4.5;
        }
        const end = svg.selectAll(".end").data(series).join("g")
          .attr("transform", (s) => `translate(${x(last.t)},${y(last[s.key])})`);
        end.append("circle").attr("r", 2.2).attr("fill", (s) => s.color);
        end.append("text").attr("x", 4).attr("y", (s, i) => ys[i] - y(last[s.key]))
          .attr("dy", "0.32em").attr("font-size", 7.5)
          .attr("fill", (s) => s.color).text((s) => `${Math.round(last[s.key])}%`);

        const legend = svg.append("g").attr("transform", `translate(${m.left + 2},8)`);
        legend.selectAll("text").data(series).join("text")
          .attr("x", (_, i) => i * 28).attr("font-size", 7.5).attr("font-weight", 600)
          .attr("fill", (s) => s.color).text((s) => s.label);
      }}
    />
  );
}
