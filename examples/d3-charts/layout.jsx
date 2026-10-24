// Tracer layout: a sidebar of three d3 charts, each a <D3View>, at the real
// sidebar width (272 px). `examples/d3_charts_tracer.rs` evaluates and
// rasterizes it across themes.
//
// The data is fixed — a tracer measures drawing, not streams — but shaped like
// the real thing: ten minutes of 5 s samples, a week of hours, and tauler's own
// dependency graph. `.data/` is gitignored: `./gen-crates.sh` writes
// crates.js, and D3View.jsx says how to build the d3 + linkedom bundle.
import { d3 } from "./.data/d3dom.js";
import crates from "./.data/crates.js";
import LineChart from "./LineChart.jsx";
import BarChart from "./BarChart.jsx";
import NetworkGraph from "./NetworkGraph.jsx";

const W = 248;
const START = Date.UTC(2026, 9, 24, 14, 0);
const rand = d3.randomLcg(0.42);
const samples = d3.range(120).map((i) => ({
  t: START + i * 5000,
  cpu: Math.max(2, Math.min(98, 35 + 22 * Math.sin(i / 9) + 14 * Math.sin(i / 3.1) + 10 * (rand() - 0.5))),
  mem: 58 + 6 * Math.sin(i / 40) + 2 * (rand() - 0.5),
}));
const days = [
  { day: "Mon", hours: 5.2 }, { day: "Tue", hours: 6.8 }, { day: "Wed", hours: 4.1 },
  { day: "Thu", hours: 7.4 }, { day: "Fri", hours: 3.6 }, { day: "Sat", hours: 1.2 }, { day: "Sun", hours: 0.4 },
];

function Section({ title, children }) {
  return (
    <div class="flex flex-col gap-[4px] w-full">
      <span class="text-[10px] text-muted-foreground">{title}</span>
      {children}
    </div>
  );
}

export default function render() {
  return (
    <root>
      <I3Layout>
        <Panel id="sidebar" anchor="left" size={272}>
          <div class="flex flex-col gap-[14px] w-full h-full bg-background p-[12px]">
            <Section title="System · last 10 min">
              <LineChart samples={samples} width={W} height={100} />
            </Section>
            <Section title="Focus · this week">
              <BarChart days={days} today="Thu" width={W} height={84} />
            </Section>
            <Section title="tauler · crate graph">
              <NetworkGraph graph={crates} width={W} height={170} />
            </Section>
          </div>
        </Panel>
      </I3Layout>
    </root>
  );
}
