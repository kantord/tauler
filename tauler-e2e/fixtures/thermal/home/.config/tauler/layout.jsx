// Thermal — one heat field, cropped per window.
//
// The brief's version tints each window's *interior* by the CPU of the process
// inside it, over kitty's remote control for terminals and an eww overlay for
// everything else. Neither half is reachable here, and the reasons are worth
// more than the feature was:
//
//   · i3's `client.*` colours are per *state*, not per window. There is no IPC
//     command that gives one container its own border colour. The brief's own
//     heat daemon has a comment admitting this and then falls through to an
//     overlay window anyway.
//   · tauler paints panels, not clients. A panel large enough to cover a window
//     would hide it: each panel is rasterized into its own opaque buffer, and
//     with no compositor there is no alpha to see through (see the layout-file
//     docs on `root-bg`).
//
// So this build does the thing the brief calls the effect it wants most, and
// reaches it by a route the brief did not consider. The <wallpaper> is one
// continuous heat field across the whole screen; the terminals are urxvt with
// -tr, so each reads _XROOTPMAP_ID and paints its own crop of that field as its
// background. Adjacent windows line up into a single image, across the 20px
// gaps, with no compositor, no per-window IPC, and nothing polling /proc.
//
// What is left over — the measurement callout on focus — is a free-floating
// panel whose geometry comes from a live module reading _NET_ACTIVE_WINDOW.
// That is the one part of §3.3 that survives, and it is the interesting one:
// it makes a panel's *position* data-driven, which nothing else in this repo
// does.
//
// Geometry, which tauler-e2e/tests/scenarios.rs states by hand:
//
//   readout  1920×58 at 0,0     — anchored top, reserved
//   scale      76×1022 at 1844,58 — anchored right, reserved
//   measure    live             — free-floating, above, over the focused window

const TAULER_I3 = "/usr/local/bin/tauler-i3";
const SCENE = "~/.local/bin/thermal-scene";
const GEOMETRY = "~/.local/bin/thermal-geometry";

// Temperature order, cold to peak. Used for the scale, the ramp and the
// callout, so the three cannot drift apart.
const RAMP = ["#2E244A", "#5C3076", "#A83E60", "#D6702C", "#F4D660"];
const RAMP_CSS = `linear-gradient(90deg, ${RAMP.join(", ")})`;

// ── The field ───────────────────────────────────────────────────────────────
//
// Four heat blobs over a cold base, plus a scanline. Written as one
// `background-image` list on the wallpaper root: takumi composites the layers
// in CSS order, so the scanline goes first in the list (topmost) and the base
// last.
//
// The blob centres are in per cent, so the field is resolution-independent —
// the brief's Pillow field would have to be regenerated for a second monitor.
const FIELD = {
  backgroundColor: "#120F1C",
  backgroundImage: [
    "repeating-linear-gradient(0deg, rgba(0,0,0,0.20) 0px, rgba(0,0,0,0.20) 1px, transparent 1px, transparent 3px)",
    "radial-gradient(circle 300px at 22% 72%, rgba(244,214,96,0.92) 0%, rgba(214,112,44,0.72) 38%, rgba(168,62,96,0.34) 66%, transparent 100%)",
    "radial-gradient(circle 480px at 68% 30%, rgba(214,112,44,0.70) 0%, rgba(168,62,96,0.46) 42%, transparent 100%)",
    "radial-gradient(circle 640px at 92% 88%, rgba(168,62,96,0.50) 0%, rgba(92,48,118,0.42) 50%, transparent 100%)",
    "radial-gradient(circle 700px at 8% 8%, rgba(92,48,118,0.60) 0%, transparent 100%)",
    "linear-gradient(160deg, #2E244A 0%, #1B1530 55%, #120F1C 100%)",
  ].join(", "),
};

// ── Readout ─────────────────────────────────────────────────────────────────

function Reading({ label, value, tone }) {
  return (
    <container tw="flex flex-col justify-center">
      <text tw="text-[8px] text-muted-foreground" style={{ letterSpacing: "1.6px" }}>
        {label}
      </text>
      <text tw="text-[14px]" style={{ color: tone ?? "#F1E7E2" }}>
        {value}
      </text>
    </container>
  );
}

function Divider() {
  return <container tw="w-[1px] h-[26px] bg-border" />;
}

// The horizontal ramp under the readout, with the scene's own min and max
// written at the ends. Twelve ticks over the gradient, because a bare gradient
// reads as decoration and a ticked one reads as a scale.
function RampBar({ min, max }) {
  const ticks = [];
  for (let i = 0; i < 12; i++) {
    ticks.push(i);
  }
  return (
    <container tw="flex flex-col justify-center gap-[3px]">
      <container
        style={{
          display: "flex",
          flexDirection: "row",
          alignItems: "flex-end",
          width: "260px",
          height: "12px",
          backgroundImage: RAMP_CSS,
        }}
      >
        {ticks.map(() => (
          <container
            style={{
              width: "20px",
              height: "4px",
              borderLeft: "1px solid rgba(18,15,28,0.55)",
            }}
          />
        ))}
      </container>
      <container tw="flex flex-row justify-between w-[260px]">
        <text tw="text-[8px] text-muted-foreground">{min}</text>
        <text tw="text-[8px] text-muted-foreground">{max}</text>
      </container>
    </container>
  );
}

// ── Scale ───────────────────────────────────────────────────────────────────
//
// The right edge: the same ramp, vertical, peak at the top, with the reading
// written beside each stop.

function ScaleStop({ label, colour }) {
  return (
    <container tw="flex flex-row items-center gap-[5px] w-full">
      <container style={{ width: "9px", height: "1px", backgroundColor: colour }} />
      <text tw="text-[8px] text-muted-foreground">{label}</text>
    </container>
  );
}

function Scale({ scene }) {
  const stops = scene?.stops ?? [];
  return (
    <container
      tw="flex flex-row h-full w-full pt-[16px] pb-[16px] pl-[10px] pr-[8px] bg-card"
      style={{ borderLeft: "1px solid rgba(244,214,96,0.18)" }}
    >
      <container
        style={{
          width: "10px",
          height: "100%",
          // Reversed: the scale reads top-down from peak to cold, so the ramp
          // has to run the other way from the horizontal one.
          backgroundImage: `linear-gradient(180deg, ${RAMP.slice().reverse().join(", ")})`,
        }}
      />
      <container tw="flex flex-col justify-between grow pl-[6px]">
        {stops.map((s, i) => (
          <ScaleStop label={s} colour={RAMP[RAMP.length - 1 - i] ?? "#F4D660"} />
        ))}
      </container>
    </container>
  );
}

// ── Measurement callout ─────────────────────────────────────────────────────
//
// A panel whose x/y/width come from a subprocess. The module polls
// _NET_ACTIVE_WINDOW and `xwininfo`, because tauler-i3 publishes window titles
// and nothing about geometry — see the report.
//
// It stays one long-lived panel rather than opening and closing on focus: the
// brief worries about a map/unmap flash without a compositor, and the way not
// to find out is not to unmap. When nothing is focused the module reports a
// zero rect and the panel parks itself off-screen instead of disappearing.

function Measure({ geom }) {
  const w = geom?.w ?? 0;
  if (!w) {
    return null;
  }
  const width = Math.min(w, 360);
  return (
    <panel id="measure" x={geom.x} y={geom.y} width={width} height={26} above={true}>
      <container
        tw="flex flex-row items-center h-full w-full pl-[8px] pr-[10px] gap-[7px]"
        style={{ backgroundColor: "#F4D660" }}
      >
        <container
          style={{ width: "8px", height: "8px", backgroundColor: "#120F1C" }}
        />
        <text tw="text-[11px]" style={{ color: "#120F1C" }}>
          {geom.label}
        </text>
      </container>
    </panel>
  );
}

// ── Root ────────────────────────────────────────────────────────────────────

export default function render() {
  return (
    <root>
      <wallpaper id="desktop">
        <container tw="flex flex-col w-full h-full" style={{ ...FIELD, position: "relative" }}>
          <container
            style={{
              position: "absolute",
              bottom: "34px",
              left: "48px",
              display: "flex",
              flexDirection: "column",
            }}
          >
            <text tw="text-[13px]" style={{ color: "rgba(241,231,226,0.30)", letterSpacing: "5px" }}>
              FIELD 01 · CONTINUOUS
            </text>
            <text tw="text-[10px]" style={{ color: "rgba(241,231,226,0.20)", letterSpacing: "2px" }}>
              every window below is a crop of this image
            </text>
          </container>
        </container>
      </wallpaper>

      <I3Layout module={TAULER_I3}>
        <Panel id="readout" anchor="top" size={58}>
          <container style={{ position: "relative", width: "100%", height: "100%" }}>
            {/* The readout sits on its own crop of the field, so the bar is
                part of the same image as everything under it. */}
            <image
              src="root-bg"
              style={{ position: "absolute", top: 0, left: 0, width: "100%", height: "100%" }}
            />
            <container
              tw="flex flex-row items-center h-full w-full pl-[16px] pr-[16px] gap-[16px] bg-card"
              style={{ position: "relative", borderBottom: "1px solid rgba(244,214,96,0.18)" }}
            >
              <container tw="flex flex-row items-center gap-[8px]">
                <container style={{ width: "10px", height: "10px", backgroundColor: "#F4D660" }} />
                <text tw="text-[13px] text-primary" style={{ letterSpacing: "3px" }}>
                  IR
                </text>
              </container>

              <Divider />

              <Module bin={TAULER_I3}>
                {(data) => {
                  const wss = data?.workspaces ?? [];
                  const focused = wss.filter((w) => w.focused)[0];
                  const title = (focused?.focused_windows ?? [])[0] ?? "—";
                  return (
                    <container tw="flex flex-row items-center gap-[10px]">
                      <Reading label="SUBJECT" value={title} />
                      <Reading label="WS" value={focused?.name ?? "—"} tone="#F4D660" />
                    </container>
                  );
                }}
              </Module>

              <container tw="flex grow" />

              <Module bin={SCENE}>
                {(scene) => (
                  <container tw="flex flex-row items-center gap-[16px]">
                    <RampBar min={scene?.min ?? "--"} max={scene?.max ?? "--"} />
                    <Divider />
                    <Reading label="MIN" value={scene?.min ?? "--"} tone="#5C3076" />
                    <Reading label="MAX" value={scene?.max ?? "--"} tone="#F4D660" />
                    <Reading label="E" value={scene?.emissivity ?? "--"} />
                    <Divider />
                    <Reading label="TIME" value={scene?.time ?? "--:--"} />
                  </container>
                )}
              </Module>
            </container>
          </container>
        </Panel>

        <Panel id="scale" anchor="right" size={76}>
          <Module bin={SCENE}>{(scene) => <Scale scene={scene} />}</Module>
        </Panel>
      </I3Layout>

      {/* A <panel> produced from inside a <Module> callback, at root level.
          Nothing else in the repo does this, and it is the whole reason the
          callout can track a window. */}
      <Module bin={GEOMETRY}>{(geom) => <Measure geom={geom} />}</Module>
    </root>
  );
}
