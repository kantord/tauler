// A synthetic bar, written to match the *shape* of a real one rather than to
// look like anything in particular: roughly 400 nodes, 20-odd levels deep, four
// panels and a wallpaper, ~19 stream reads and a few modules.
//
// Those numbers come from measuring a real desktop's layout, and they are the
// only thing this file is trying to reproduce. Nothing here is copied from it.
//
// How close it lands, measured against that layout with empty streams:
//
//   eval        4.71ms   vs 4.07ms   (+16%)
//   layout JSON 21.5KB   vs 19.8KB   (+9%)
//   streams     20       vs 19
//   nodes       593      vs 405      (deeper fan-out, same order)
//
// Eval cost and JSON size are the two that matter, since they are what the
// stages downstream are handed. Node count is a means to them, not a target.
//
// It exists so `benches/pipeline.rs` has something representative to evaluate.
// A toy layout evaluates in microseconds and would make every measurement taken
// against it a lie.

import {
  LoadCard,
  MemoryCard,
  DiskCard,
  NetCard,
  QueueCard,
  AlertCard,
  ClockCard,
  ForecastCard,
} from "./components/cards.jsx";
import { ProcessList, SessionList, AgendaList, WorkspaceStrip } from "./components/panels.jsx";
import { MetricTile, SparkRow, TagStrip, KeyValueRow } from "./components/tiles.jsx";

const SH = "/bin/sh";
const COL = "flex flex-col gap-[6px] p-[8px]";
const ROW = "flex flex-row items-center gap-[8px] px-[8px]";

function TopStrip() {
  const modes = useJSONStream(SH, "echo modes");
  const battery = useJSONStream(SH, "echo battery");
  const audio = useJSONStream(SH, "echo audio");
  return (
    <div class={ROW}>
      <TagStrip tags={modes?.available ?? []} active={modes?.current} />
      <MetricTile
        label="batt"
        value={battery?.pct ?? 0}
        unit="%"
        level={100 - (battery?.pct ?? 0)}
      />
      <MetricTile label="vol" value={audio?.volume ?? 0} unit="%" level={audio?.volume ?? 0} />
      <SparkRow points={audio?.levels ?? []} />
    </div>
  );
}

function LeftColumn() {
  const links = useJSONStream(SH, "echo links");
  return (
    <div class={COL}>
      <ClockCard />
      <ForecastCard />
      <AgendaList />
      <div class="flex flex-col gap-[2px]">
        {(links?.items ?? []).slice(0, 5).map((l) => (
          <KeyValueRow name={l.label} detail={l.hint} muted={!l.enabled} />
        ))}
      </div>
    </div>
  );
}

function RightColumn() {
  return (
    <div class={COL}>
      <LoadCard />
      <MemoryCard />
      <DiskCard />
      <NetCard />
    </div>
  );
}

function BottomStrip() {
  const build = useJSONStream(SH, "echo build");
  const vcs = useJSONStream(SH, "echo vcs");
  return (
    <div class={ROW}>
      <KeyValueRow name={vcs?.branch ?? "-"} detail={vcs?.dirty ? "dirty" : "clean"} />
      <TagStrip tags={build?.stages ?? []} active={build?.current} />
      <SparkRow points={build?.durations ?? []} />
      <img src="tauler:root-bg" style={{ width: 16, height: 16 }} />
    </div>
  );
}

export default function render() {
  return (
    <root>
      <wallpaper id="bg">
    <div class="flex w-full h-full bg-slate-950" />
  </wallpaper>

  <I3Layout>
    <Panel id="topbar" anchor="top" size={34}>
      <div class="flex flex-row justify-between w-full h-full bg-slate-900">
        <Module bin={SH} args={["-c", "echo workspaces"]}>
          {(data) => <WorkspaceStrip workspaces={data?.workspaces ?? []} />}
        </Module>
        <TopStrip />
      </div>
    </Panel>

    <Panel id="left" anchor="left" size={220}>
      <div class="flex flex-col w-full h-full bg-slate-900">
        <LeftColumn />
        <SessionList />
      </div>
    </Panel>

    <Panel id="right" anchor="right" size={220}>
      <div class="flex flex-col w-full h-full bg-slate-900">
        <RightColumn />
        <Module bin={SH} args={["-c", "echo notify"]}>
          {(data) => <AlertCard />}
        </Module>
      </div>
    </Panel>

    <Panel id="bottom" anchor="bottom" size={30}>
      <div class="flex flex-row w-full h-full bg-slate-900">
        <BottomStrip />
        <Module bin={SH} args={["-c", "echo tasks"]}>
          {(data) => <QueueCard />}
        </Module>
        <ProcessList />
      </div>
    </Panel>
  </I3Layout>
    </root>
  );
}
