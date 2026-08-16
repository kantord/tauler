// Small leaf components. Each is deliberately shaped like something a real bar
// would contain: read a value, branch on it, build a class string, emit a few
// nested elements. None of them do anything clever — the point is the volume of
// ordinary work, since that is what the eval cost turns out to be made of.

const ROW = "flex flex-row items-center gap-[6px]";
const COL = "flex flex-col gap-[2px]";

function tone(level) {
  if (level > 85) return "text-red-400";
  if (level > 60) return "text-amber-300";
  return "text-slate-200";
}

export function MetricTile({ label, value, unit, level }) {
  return (
    <div class={COL + " px-[8px] py-[4px] rounded-lg bg-slate-800/40"}>
      <div class="flex flex-col">
       <div class="flex flex-col">
      <div class={ROW}>
        <span class="text-[10px] text-slate-400">{label}</span>
        <span class={"text-[11px] " + tone(level)}>
          {value}
          {unit}
        </span>
      </div>
      <div class="flex flex-row h-[3px] w-[64px] rounded bg-slate-700">
        <div
          class="h-[3px] rounded bg-sky-400"
          style={{ width: Math.max(2, Math.min(64, level)) }}
        />
      </div>
       </div>
      </div>
    </div>
  );
}

export function SparkRow({ points }) {
  const bars = (points || []).slice(0, 12);
  return (
    <div class="flex flex-row items-end gap-[1px] h-[16px]">
      {bars.map((p, i) => (
        <div
          class={i === bars.length - 1 ? "w-[3px] bg-sky-300" : "w-[3px] bg-slate-500"}
          style={{ height: Math.max(1, Math.min(16, Math.round(p / 6))) }}
        />
      ))}
    </div>
  );
}

export function TagStrip({ tags, active }) {
  return (
    <div class={ROW}>
      {(tags || []).map((t) => (
        <div
          class={
            t === active
              ? "px-[6px] py-[1px] rounded-2xl bg-sky-500 text-slate-950 text-[10px]"
              : "px-[6px] py-[1px] rounded-2xl bg-slate-700 text-slate-300 text-[10px]"
          }
        >
          {t}
        </div>
      ))}
    </div>
  );
}

export function KeyValueRow({ name, detail, muted }) {
  return (
    <div class="flex flex-row justify-between w-full">
      <span class={muted ? "text-[10px] text-slate-500" : "text-[10px] text-slate-300"}>
        {name}
      </span>
      <span class="text-[10px] text-slate-400">{detail}</span>
    </div>
  );
}
