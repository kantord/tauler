// Card-sized components, each reading one stream. A card is the unit that makes
// a bar expensive: a stream read, a shape check, a map over a handful of rows,
// and a dozen elements out the other side.

import { MetricTile, SparkRow, TagStrip, KeyValueRow } from "./tiles.jsx";

const CARD = "flex flex-col gap-[4px] px-[10px] py-[6px] rounded-xl bg-slate-900/60";
const HEAD = "flex flex-row items-center justify-between";

// Rows the cards fall back to when a stream has produced nothing yet. A real
// bar renders its skeleton in that state rather than collapsing to nothing,
// which is why the measured tree is the same size either way.
export function rows(n, make) {
  const out = [];
  for (let i = 0; i < n; i++) out.push(make(i));
  return out;
}

function series(seed, n) {
  const out = [];
  let x = seed;
  for (let i = 0; i < n; i++) {
    x = (x * 37 + 11) % 100;
    out.push(x);
  }
  return out;
}

export function LoadCard() {
  const d = useJSONStream("/bin/sh", "echo load");
  const one = d?.one ?? 0;
  return (
    <div class={CARD}>
      <div class={HEAD}>
        <span class="text-[10px] text-slate-400">load</span>
        <span class="text-[11px] text-slate-200">{one}</span>
      </div>
      <SparkRow points={d?.history ?? series(one + 3, 12)} />
      <MetricTile label="1m" value={one} unit="" level={one} />
      <MetricTile label="5m" value={d?.five ?? 0} unit="" level={d?.five ?? 0} />
    </div>
  );
}

export function MemoryCard() {
  const d = useJSONStream("/bin/sh", "echo memory");
  const used = d?.used_pct ?? 0;
  return (
    <div class={CARD}>
      <div class={HEAD}>
        <span class="text-[10px] text-slate-400">memory</span>
        <span class="text-[11px] text-slate-200">{used}%</span>
      </div>
      <MetricTile label="rss" value={d?.rss_gb ?? 0} unit="G" level={used} />
      <KeyValueRow name="swap" detail={(d?.swap_gb ?? 0) + "G"} muted={used < 50} />
      <KeyValueRow name="cache" detail={(d?.cache_gb ?? 0) + "G"} muted />
    </div>
  );
}

export function DiskCard() {
  const d = useJSONStream("/bin/sh", "echo disk");
  const mounts = d?.mounts ?? rows(3, (i) => ({ path: "/vol" + i, used_pct: 20 + i * 13 }));
  return (
    <div class={CARD}>
      <span class="text-[10px] text-slate-400">disk</span>
      {mounts.slice(0, 3).map((m) => (
        <MetricTile label={m.path} value={m.used_pct} unit="%" level={m.used_pct} />
      ))}
    </div>
  );
}

export function NetCard() {
  const d = useJSONStream("/bin/sh", "echo net");
  const ifaces = d?.interfaces ?? rows(3, (i) => ({ name: "if" + i, rx_kbs: i * 40, tx_kbs: i * 7, up: i !== 2 }));
  return (
    <div class={CARD}>
      <div class={HEAD}>
        <span class="text-[10px] text-slate-400">net</span>
        <TagStrip tags={ifaces.map((i) => i.name)} active={d?.primary} />
      </div>
      {ifaces.slice(0, 3).map((i) => (
        <KeyValueRow name={i.name} detail={i.rx_kbs + " / " + i.tx_kbs} muted={!i.up} />
      ))}
      <SparkRow points={d?.history ?? series(7, 12)} />
    </div>
  );
}

export function QueueCard() {
  const d = useJSONStream("/bin/sh", "echo queue");
  const items = d?.items ?? rows(5, (i) => ({ title: "job-" + i, state: i % 2 ? "run" : "done" }));
  return (
    <div class={CARD}>
      <div class={HEAD}>
        <span class="text-[10px] text-slate-400">queue</span>
        <span class="text-[10px] text-slate-500">{items.length}</span>
      </div>
      {items.slice(0, 5).map((it) => (
        <KeyValueRow name={it.title} detail={it.state} muted={it.state === "done"} />
      ))}
    </div>
  );
}

export function AlertCard() {
  const d = useJSONStream("/bin/sh", "echo alerts");
  const alerts = d?.alerts ?? rows(4, (i) => ({ severity: i ? "low" : "high", message: "check " + i }));
  if (alerts.length === 0) {
    return (
      <div class={CARD}>
        <span class="text-[10px] text-slate-500">no alerts</span>
      </div>
    );
  }
  return (
    <div class={CARD}>
      {alerts.slice(0, 4).map((a) => (
        <div class="flex flex-row items-center gap-[6px]">
          <div
            class={
              a.severity === "high"
                ? "w-[6px] h-[6px] rounded-2xl bg-red-400"
                : "w-[6px] h-[6px] rounded-2xl bg-amber-300"
            }
          />
          <span class="text-[10px] text-slate-300">{a.message}</span>
        </div>
      ))}
    </div>
  );
}

export function ClockCard() {
  const t = useJSONStream("/bin/sh", "echo clock");
  return (
    <div class={CARD}>
      <span class="text-[14px] text-slate-100">{t?.time ?? "--:--"}</span>
      <KeyValueRow name={t?.weekday ?? ""} detail={t?.date ?? ""} muted />
    </div>
  );
}

export function ForecastCard() {
  const d = useJSONStream("/bin/sh", "echo forecast");
  const days = d?.days ?? rows(5, (i) => ({ label: "d" + i, high: 20 + i, low: 10 + i }));
  return (
    <div class={CARD}>
      <div class="flex flex-row gap-[6px]">
        {days.slice(0, 5).map((day) => (
          <div class="flex flex-col items-center gap-[1px]">
            <span class="text-[9px] text-slate-500">{day.label}</span>
            <span class="text-[10px] text-slate-200">{day.high}</span>
            <span class="text-[9px] text-slate-500">{day.low}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
