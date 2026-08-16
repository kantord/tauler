// The larger blocks: lists and tables, which is where a real bar's node count
// actually comes from.

import { KeyValueRow, TagStrip } from "./tiles.jsx";
import { rows } from "./cards.jsx";

const CARD = "flex flex-col gap-[4px] px-[10px] py-[6px] rounded-xl bg-slate-900/60";

export function ProcessList() {
  const d = useJSONStream("/bin/sh", "echo processes");
  const list = d?.rows ?? rows(5, (i) => ({ name: "proc" + i, cpu: i * 3, mem: i * 11 }));
  return (
    <div class={CARD}>
      <span class="text-[10px] text-slate-400">processes</span>
      {list.slice(0, 5).map((r) => (
        <div class="flex flex-row justify-between w-full">
          <span class="text-[10px] text-slate-300">{r.name}</span>
          <div class="flex flex-row gap-[6px]">
            <span class="text-[10px] text-slate-500">{r.cpu}</span>
            <span class="text-[10px] text-slate-500">{r.mem}</span>
          </div>
        </div>
      ))}
    </div>
  );
}

export function SessionList() {
  const d = useJSONStream("/bin/sh", "echo sessions");
  const items = d?.sessions ?? rows(4, (i) => ({ name: "s" + i, age: i + "m", attached: i < 2, tags: ["a" + i, "b" + i] }));
  return (
    <div class={CARD}>
      <span class="text-[10px] text-slate-400">sessions</span>
      {items.slice(0, 4).map((s) => (
        <div class="flex flex-col gap-[1px]">
          <KeyValueRow name={s.name} detail={s.age} muted={!s.attached} />
          <TagStrip tags={s.tags ?? []} active={s.tags?.[0]} />
        </div>
      ))}
    </div>
  );
}

export function AgendaList() {
  const d = useJSONStream("/bin/sh", "echo agenda");
  const entries = d?.entries ?? rows(5, (i) => ({ at: i + ":00", title: "item " + i, soon: i < 2 }));
  return (
    <div class={CARD}>
      <span class="text-[10px] text-slate-400">agenda</span>
      {entries.slice(0, 5).map((e) => (
        <div class="flex flex-row gap-[6px]">
          <span class="text-[10px] text-slate-500">{e.at}</span>
          <span class={e.soon ? "text-[10px] text-sky-300" : "text-[10px] text-slate-300"}>
            {e.title}
          </span>
        </div>
      ))}
    </div>
  );
}

export function WorkspaceStrip({ workspaces }) {
  return (
    <div class="flex flex-row gap-[3px]">
      {(workspaces ?? []).map((w) => (
        <div
          class={
            w.focused
              ? "flex flex-row items-center justify-center w-[22px] h-[20px] rounded-2xl bg-sky-500"
              : "flex flex-row items-center justify-center w-[22px] h-[20px] rounded-2xl bg-slate-700"
          }
        >
          <span class="text-[10px] text-slate-100">{w.name}</span>
        </div>
      ))}
    </div>
  );
}
