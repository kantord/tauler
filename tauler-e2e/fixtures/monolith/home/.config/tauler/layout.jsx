// Monolith III — engraved surfaces.
//
// Exploratory scenario for the "three rices" handoff brief. The brief targets
// eww/GTK3 and spends most of its questions on which paint primitives survive
// there; tauler rasterizes with takumi, so the answers are different enough
// that the design changes shape. Everything textured below is CSS on a
// <container>. Nothing is a pre-rendered PNG, and the brief's plate-gen.py has
// no equivalent here at all.
//
// Geometry, which tauler-e2e/tests/scenarios.rs states by hand:
//
//   rail      60×1080 at 0,0     — anchored left, reserved
//   slip     400×132  at 1480,40 — free-floating, above; the galley notification
//   launcher 680×360  at 620,300 — free-floating, above; the "rofi" plate
//
// The two floats reserve nothing: only <Panel> inside <I3Layout> eats an edge.
//
// There is not one icon glyph in this file. The brief forbids them and nothing
// here wanted one.

const TAULER_I3 = "/usr/local/bin/tauler-i3";
const STATUS = "~/.local/bin/monolith-status";

// ── Plates ──────────────────────────────────────────────────────────────────
//
// Four textures, each a `style` object rather than a class, because none of
// them is expressible in the `tw` subset and all of them are the point.
//
// The alphas follow the brief's legibility rule: texture never exceeds ~0.2
// under body text, full strength only in margins, meters and empty plate.

// RULED. 1px gold rule every 6px. `repeating-linear-gradient` with two hard
// stops per period — the same declaration the brief writes for GTK3, and it
// needed no translation.
const RULED = {
  backgroundColor: "#1B1924",
  backgroundImage:
    "repeating-linear-gradient(0deg, rgba(216,178,94,0.16) 0px, rgba(216,178,94,0.16) 1px, transparent 1px, transparent 6px)",
};

// HALFTONE. The brief asks whether a repeating radial dot field is reachable.
// It is, but not with `repeating-radial-gradient` — that draws concentric
// rings, not a grid. A single `radial-gradient` tile plus `background-size`
// and `background-repeat` is the dot field, and it is one declaration.
const HALFTONE = {
  backgroundColor: "#2B2836",
  backgroundImage:
    "radial-gradient(circle at 3px 3px, rgba(216,178,94,0.30) 0px, rgba(216,178,94,0.30) 1.4px, transparent 1.6px)",
  backgroundSize: "9px 9px",
  backgroundRepeat: "repeat",
};

// GUILLOCHÉ. Concentric rings off a centre outside the box, which is what
// `repeating-radial-gradient` is actually good for.
const GUILLOCHE = (cx, cy, alpha) => ({
  backgroundImage: `repeating-radial-gradient(circle at ${cx} ${cy}, transparent 0px, transparent 11px, rgba(216,178,94,${alpha}) 11px, rgba(216,178,94,${alpha}) 12px)`,
});

// EMBOSS. A lit top edge and a shadowed bottom one, both inside the box. The
// brief's fallback for this was a 9-slice PNG; `box-shadow: inset` is enough.
const EMBOSS = {
  boxShadow:
    "inset 0 1px 0 rgba(216,178,94,0.22), inset 0 -1px 0 rgba(16,15,22,0.9)",
};

// MOIRÉ. Two rule fields one degree either side of vertical, stacked in one
// `background-image` list. The beat comes from the sampling, so it is real
// interference rather than a picture of interference — and it changes with the
// meter's height, which a pre-rendered PNG pair could not do.
const MOIRE = {
  backgroundImage:
    "repeating-linear-gradient(87deg, rgba(216,178,94,0.75) 0px, rgba(216,178,94,0.75) 1px, transparent 1px, transparent 6px)," +
    "repeating-linear-gradient(93deg, rgba(216,178,94,0.75) 0px, rgba(216,178,94,0.75) 1px, transparent 1px, transparent 6px)",
};

// ── Type ────────────────────────────────────────────────────────────────────
//
// `fonts.primary` in config.yaml puts Nimbus Roman in the sans-serif slot, so
// the serif is the default and the mono has to be asked for. That is backwards
// from how the brief writes it and it is the only way to get two faces.
const MONO = { fontFamily: "monospace" };

const ROMAN = ["", "I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X"];

function roman(name) {
  const n = parseInt(name, 10);
  return ROMAN[n] ?? name;
}

// ── Rail ────────────────────────────────────────────────────────────────────

function Folio({ ws }) {
  const focused = ws.focused;
  return (
    <container
      tw="flex flex-row items-center justify-center w-full h-[34px]"
      style={focused ? { backgroundColor: "#D8B25E" } : {}}
    >
      <text
        tw={`text-[19px] ${focused ? "text-primary-foreground" : "text-muted-foreground"}`}
      >
        {roman(ws.name)}
      </text>
    </container>
  );
}

// A load meter that reads as an engraved gauge rather than a progress bar: the
// moiré fills from the bottom and a plate-coloured mask eats it from the top.
function Moire({ load }) {
  const height = Math.round(150 * (1 - load));
  return (
    // The `overflow: hidden` wrapper is not decoration. A rotated
    // `repeating-linear-gradient` paints outside its own border box in takumi
    // 0.20 — the 87°/93° pair below bled across the full 60px rail and 390px
    // down it, straight over the load figure and the clock. Clipping it in a
    // parent is the fix, and it costs nothing.
    <container
      style={{
        display: "flex",
        width: "34px",
        height: "150px",
        overflow: "hidden",
      }}
    >
      <container
        tw="flex flex-col w-full h-full border border-border"
        style={MOIRE}
      >
        <container
          style={{ width: "100%", height: `${height}px`, backgroundColor: "#1B1924" }}
        />
      </container>
    </container>
  );
}

function Rail({ ws, status }) {
  const load = status?.load ?? 0;
  return (
    <container
      tw="flex flex-col items-center w-full h-full pt-[14px] pb-[14px]"
      style={{ ...RULED, borderRight: "1px solid rgba(216,178,94,0.22)" }}
    >
      <text tw="text-[26px] text-primary">M</text>

      <container tw="flex flex-col items-center w-full mt-[18px]" style={EMBOSS}>
        {ws.map((w) => (
          <Folio ws={w} />
        ))}
      </container>

      <container tw="flex flex-col grow" />

      <Moire load={load} />
      <text tw="text-[9px] text-muted-foreground mt-[6px]" style={MONO}>
        {`${Math.round(load * 100)}%`}
      </text>

      <container tw="flex flex-col items-center mt-[14px]">
        <text tw="text-[17px] text-foreground">{status?.hh ?? "--"}</text>
        <text tw="text-[17px] text-muted-foreground">{status?.mm ?? "--"}</text>
      </container>
    </container>
  );
}

// ── Galley slip ─────────────────────────────────────────────────────────────
//
// The brief's hardest surface: dunst paints a flat fill and nothing else, so
// the perforated edge had to become an icon PNG or a second always-on-top
// window drawing over dunst's geometry. Here the notification *is* a panel, so
// the perforation is a background tile like any other and the whole question
// dissolves. Half-circles punched in the paper, showing the plate behind.
const PERFORATION = {
  backgroundColor: "#E9E4DA",
  backgroundImage:
    "radial-gradient(circle at 0px 6px, #221F2B 0px, #221F2B 3.4px, transparent 3.8px)",
  backgroundSize: "14px 12px",
  backgroundRepeat: "repeat-y",
};

function Slip() {
  return (
    <container tw="flex flex-row w-full h-full" style={{ backgroundColor: "#E9E4DA" }}>
      <container tw="flex w-[14px] h-full" style={PERFORATION} />
      <container tw="flex flex-col grow pl-[18px] pr-[18px] pt-[14px] pb-[14px]">
        <container tw="flex flex-row items-center justify-between">
          <text tw="text-[10px]" style={{ ...MONO, color: "#6C6478", letterSpacing: "1.5px" }}>
            GALLEY
          </text>
          <text tw="text-[10px]" style={{ ...MONO, color: "#6C6478" }}>
            09:41
          </text>
        </container>
        <container
          tw="w-full h-[1px] mt-[8px] mb-[10px]"
          style={{ backgroundColor: "#B9B2A4" }}
        />
        <text tw="text-[17px]" style={{ color: "#221F2B" }}>
          Plate cycle complete
        </text>
        <text tw="text-[11px] mt-[4px]" style={{ ...MONO, color: "#6C6478" }}>
          4 surfaces reseeded from palette hash
        </text>
      </container>
    </container>
  );
}

// ── Launcher ────────────────────────────────────────────────────────────────
//
// Standing in for rofi, whose .rasi cannot take a per-element background image.
// Here every row is an ordinary node, so "ruled plate under the list, halftone
// under the header" is not a question anyone has to ask.

function Entry({ mark, name, hint, selected }) {
  return (
    <container
      tw="flex flex-row items-center w-full h-[38px] pl-[16px] pr-[16px]"
      style={selected ? { backgroundColor: "rgba(216,178,94,0.16)", ...EMBOSS } : {}}
    >
      <text tw="text-[15px] text-primary w-[38px]">{mark}</text>
      <text tw={`text-[15px] ${selected ? "text-foreground" : "text-muted-foreground"}`}>
        {name}
      </text>
      <container tw="flex grow" />
      <text tw="text-[10px] text-muted-foreground" style={MONO}>
        {hint}
      </text>
    </container>
  );
}

function Launcher() {
  return (
    <container
      tw="flex flex-col w-full h-full border border-border"
      style={{ backgroundColor: "#221F2B" }}
    >
      <container
        tw="flex flex-row items-center w-full h-[64px] pl-[20px] pr-[20px]"
        style={{ ...HALFTONE, ...EMBOSS }}
      >
        <text tw="text-[13px] text-primary" style={{ ...MONO, letterSpacing: "3px" }}>
          RUN
        </text>
        <container
          tw="w-[1px] h-[20px] ml-[16px] mr-[16px]"
          style={{ backgroundColor: "rgba(216,178,94,0.35)" }}
        />
        <text tw="text-[19px] text-foreground">plate</text>
        <container tw="w-[1px] h-[21px] ml-[3px]" style={{ backgroundColor: "#D8B25E" }} />
      </container>

      <container tw="flex flex-col w-full grow pt-[8px]" style={RULED}>
        <Entry mark="I" name="plate-cycle" hint="mod+p" selected={true} />
        <Entry mark="II" name="plate-regen" hint="mod+shift+w" />
        <Entry mark="III" name="folio-index" hint="" />
        <Entry mark="IV" name="galley-replay" hint="" />
      </container>

      <container
        tw="flex flex-row items-center justify-between w-full h-[30px] pl-[20px] pr-[20px]"
        style={{ backgroundColor: "#1B1924", ...EMBOSS }}
      >
        <text tw="text-[10px] text-muted-foreground" style={MONO}>
          4 entries
        </text>
        <text tw="text-[10px] text-muted-foreground" style={MONO}>
          seed 8f3c1a
        </text>
      </container>
    </container>
  );
}

// ── Root ────────────────────────────────────────────────────────────────────

export default function render() {
  return (
    <root>
      {/* The whole wallpaper is CSS. Base plate, a guilloché rosette off the
          top-right, a halftone field fading in from the left, and one wide
          ruled band. The brief's Pillow generator drew exactly these four and
          wrote them to a PNG; here they are four declarations and they resize
          with the output. */}
      <wallpaper id="desktop">
        <container
          tw="flex flex-col w-full h-full"
          style={{ backgroundColor: "#221F2B", position: "relative" }}
        >
          <container
            style={{
              position: "absolute",
              top: 0,
              left: 0,
              width: "100%",
              height: "100%",
              ...GUILLOCHE("78%", "18%", 0.1),
            }}
          />
          <container
            style={{
              position: "absolute",
              top: 0,
              left: 0,
              width: "46%",
              height: "100%",
              backgroundImage:
                "radial-gradient(circle at 3px 3px, rgba(216,178,94,0.13) 0px, rgba(216,178,94,0.13) 1.4px, transparent 1.6px)",
              backgroundSize: "10px 10px",
              backgroundRepeat: "repeat",
            }}
          />
          <container
            style={{
              position: "absolute",
              top: "62%",
              left: 0,
              width: "100%",
              height: "180px",
              backgroundImage:
                "repeating-linear-gradient(0deg, rgba(216,178,94,0.09) 0px, rgba(216,178,94,0.09) 1px, transparent 1px, transparent 7px)",
            }}
          />
          <container
            style={{
              position: "absolute",
              bottom: "40px",
              right: "56px",
              display: "flex",
              flexDirection: "column",
              alignItems: "flex-end",
            }}
          >
            <text tw="text-[92px]" style={{ color: "rgba(216,178,94,0.18)" }}>
              MONOLITH
            </text>
            <text
              tw="text-[13px]"
              style={{ ...MONO, color: "rgba(233,228,218,0.20)", letterSpacing: "6px" }}
            >
              THIRD PLATE
            </text>
          </container>
        </container>
      </wallpaper>

      <I3Layout module={TAULER_I3}>
        <Panel id="rail" anchor="left" size={60}>
          <Module bin={TAULER_I3}>
            {(i3data) => (
              <Module bin={STATUS}>
                {(status) => (
                  <Rail ws={i3data?.workspaces ?? []} status={status} />
                )}
              </Module>
            )}
          </Module>
        </Panel>
      </I3Layout>

      {/* Free-floating and stacked above the clients. Neither reserves space —
          that is what makes them notification-and-launcher rather than bars. */}
      <panel id="slip" x={1480} y={40} width={400} height={132} above={true}>
        <Slip />
      </panel>

      <panel id="launcher" x={620} y={300} width={680} height={360} above={true}>
        <Launcher />
      </panel>
    </root>
  );
}
