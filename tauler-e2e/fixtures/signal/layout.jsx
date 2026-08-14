// Signal — a flag per workspace.
//
// Each workspace flies a small piece of generative geometric art, derived from
// the workspace's own contents: hash the sorted window names into a seed, and
// let the seed pick the division and the two colours. Same set of windows, same
// flag, every time — so you learn to recognise your workspaces by their flag,
// and a new window visibly changes one.
//
// The brief builds this with a Pillow script writing PNGs into /run/user, an
// i3ipc daemon regenerating them on every window event, a content-hash in the
// filename to defeat the bar's image cache, and a symlink the bar points at. It
// then spends three questions on whether the bar notices the file changed.
//
// None of that exists here. A flag is five nested <container>s, the seed is
// computed in the layout file from the data tauler-i3 already publishes, and
// "does the bar notice" is not a question you can ask: every tick re-renders
// everything from the current data (docs/adr/0007). There is no cache to bust
// because there is no file.
//
// Geometry, which tauler-e2e/tests/scenarios.rs states by hand:
//
//   hoist  1920×92 at 0,0  — anchored top, reserved

const TAULER_I3 = "/usr/local/bin/tauler-i3";
const SIGNALS = "/fixtures/signal/bin/signals";

const FLAG_W = 96;
const FLAG_H = 64;

const INK = {
  navy: "#10182C",
  bone: "#E9E7DC",
  oxide: "#B04A3A",
  signal: "#DFB22E",
  sea: "#3A74B0",
};
const KEYS = ["bone", "oxide", "signal", "sea"];

// FNV-1a, 32-bit. `Math.imul` rather than `*`: the multiply overflows 2^53 and
// silently loses the low bits otherwise, which turns a hash into a smear —
// every workspace would fly nearly the same flag and the whole idea would look
// like it worked.
function hash(str) {
  let acc = 2166136261;
  for (let i = 0; i < str.length; i++) {
    acc = Math.imul(acc ^ str.charCodeAt(i), 16777619) >>> 0;
  }
  return acc;
}

// The seed is the workspace number plus its window names, sorted. Sorted
// because the flag must not change when you move focus between two windows.
//
// tauler-i3 publishes window *titles*, not classes. That is not what the brief
// asked for and it matters: a title changes when you cd, open a file or load a
// page, so a title-seeded flag reshuffles under you while a class-seeded one
// would not. The fixture's clients hold static titles, which hides it here —
// see the report.
function seedFor(ws) {
  const names = (ws.focused_windows ?? []).slice().sort();
  return `${ws.name}|${names.join(",")}`;
}

// ── The flag ────────────────────────────────────────────────────────────────
//
// Five divisions, all pure flex. No absolute positioning, because a container
// that is only ever two boxes wide does not need any — and the layout-file docs
// warn that several absolutely-positioned siblings is a bug family.

function Half({ colour, horizontal }) {
  return (
    <container
      style={{
        display: "flex",
        width: horizontal ? "100%" : "50%",
        height: horizontal ? "50%" : "100%",
        backgroundColor: colour,
      }}
    />
  );
}

function Quarter({ colour }) {
  return (
    <container
      style={{ display: "flex", width: "50%", height: "100%", backgroundColor: colour }}
    />
  );
}

function Field({ div, a, b }) {
  if (div === 0) {
    return (
      <container style={{ display: "flex", flexDirection: "row", width: "100%", height: "100%", backgroundColor: a }}>
        <Half colour={b} horizontal={false} />
      </container>
    );
  }
  if (div === 1) {
    return (
      <container style={{ display: "flex", flexDirection: "column", width: "100%", height: "100%", backgroundColor: a }}>
        <Half colour={b} horizontal={true} />
      </container>
    );
  }
  if (div === 2) {
    return (
      <container style={{ display: "flex", flexDirection: "column", width: "100%", height: "100%", backgroundColor: a }}>
        <container style={{ display: "flex", flexDirection: "row", width: "100%", height: "50%" }}>
          <Quarter colour={b} />
        </container>
        <container style={{ display: "flex", flexDirection: "row", width: "100%", height: "50%", justifyContent: "flex-end" }}>
          <Quarter colour={b} />
        </container>
      </container>
    );
  }
  if (div === 3) {
    // The hoist triangle. `clip-path: polygon(...)` is the whole division —
    // the brief's generator drew this with Pillow's `d.polygon` because GTK3
    // CSS has no way to cut a shape out of a box.
    return (
      <container style={{ display: "flex", width: "100%", height: "100%", backgroundColor: a }}>
        <container
          style={{
            width: "100%",
            height: "100%",
            backgroundColor: b,
            clipPath: "polygon(0% 0%, 100% 0%, 0% 100%)",
          }}
        />
      </container>
    );
  }
  return (
    <container
      style={{
        display: "flex",
        width: "100%",
        height: "100%",
        backgroundColor: a,
        paddingTop: "6px",
        paddingBottom: "6px",
        paddingLeft: "8px",
        paddingRight: "8px",
      }}
    >
      <container style={{ width: "100%", height: "100%", backgroundColor: b }} />
    </container>
  );
}

// `seed`, and never `h`. `h` is the JSX factory: optative-script registers it as
// a global and every element in this file compiles to a call to it. A local
// `const h = hash(...)` shadows it, so the very next `<Field />` becomes
// "Error: not a function" — reported against the JSX line, not the declaration
// that broke it, and only in the scope that declared it. Renaming is the whole
// fix; see the report for how long that took to find.
function Flag({ ws }) {
  const seed = hash(seedFor(ws));
  const div = seed % 5;
  let a = INK[KEYS[(seed >>> 3) % 4]];
  let b = INK[KEYS[(seed >>> 11) % 4]];
  if (a === b) b = INK.navy;

  return (
    <container
      style={{
        display: "flex",
        flexDirection: "column",
        width: `${FLAG_W}px`,
        height: `${FLAG_H}px`,
        position: "relative",
        // Focus is a hoist rope, not a colour change: the flag itself must stay
        // the workspace's identity or the whole conceit breaks.
        opacity: ws.focused ? 1 : 0.6,
        boxShadow: ws.focused
          ? "inset 0 0 0 2px #E9E7DC"
          : "inset 0 0 0 1px rgba(233,231,220,0.25)",
      }}
    >
      <Field div={div} a={a} b={b} />
    </container>
  );
}

// The number rides under the flag rather than on it: at 96×64 a glyph over a
// quartered field is unreadable against two of the four palettes.
function Hoisted({ ws }) {
  return (
    <container tw="flex flex-col items-center gap-[4px]">
      <Flag ws={ws} />
      <text
        tw={`text-[10px] ${ws.focused ? "text-foreground" : "text-muted-foreground"}`}
      >
        {ws.name}
      </text>
    </container>
  );
}

// ── Signals ─────────────────────────────────────────────────────────────────

const SIGNAL_INK = {
  B: { bg: "#B04A3A", fg: "#E9E7DC" },
  O: { bg: "#DFB22E", fg: "#10182C" },
  P: { bg: "#3A74B0", fg: "#E9E7DC" },
};

function Sig({ code, faded }) {
  const ink = SIGNAL_INK[code] ?? { bg: "#1A2038", fg: "#7F8AA6" };
  return (
    <container
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        width: faded ? "20px" : "30px",
        height: faded ? "20px" : "30px",
        backgroundColor: ink.bg,
        opacity: faded ? 0.35 : 1,
      }}
    >
      <text
        style={{ fontSize: faded ? "11px" : "16px", fontWeight: 700, color: ink.fg }}
      >
        {code}
      </text>
    </container>
  );
}

export default function render() {
  return (
    <root>
      {/* Navy ground with a huge low-contrast hoist triangle and a dot field.
          The triangle is the same `clip-path` the flags use, at 1080px. */}
      <wallpaper id="desktop">
        <container
          tw="flex flex-col w-full h-full"
          style={{
            backgroundColor: "#10182C",
            position: "relative",
            backgroundImage:
              "radial-gradient(circle at 2px 2px, rgba(233,231,220,0.05) 0px, rgba(233,231,220,0.05) 1.2px, transparent 1.4px)",
            backgroundSize: "14px 14px",
            backgroundRepeat: "repeat",
          }}
        >
          <container
            style={{
              position: "absolute",
              top: 0,
              left: 0,
              width: "100%",
              height: "100%",
              backgroundColor: "rgba(58,116,176,0.10)",
              clipPath: "polygon(0% 0%, 100% 100%, 0% 100%)",
            }}
          />
          <container
            style={{
              position: "absolute",
              bottom: "44px",
              right: "60px",
              display: "flex",
              flexDirection: "column",
              alignItems: "flex-end",
            }}
          >
            <text
              style={{
                fontSize: "84px",
                fontWeight: 800,
                letterSpacing: "14px",
                color: "rgba(233,231,220,0.10)",
              }}
            >
              SIGNAL
            </text>
          </container>
        </container>
      </wallpaper>

      <I3Layout module={TAULER_I3}>
        <Panel id="hoist" anchor="top" size={92}>
          <container style={{ position: "relative", width: "100%", height: "100%" }}>
            <image
              src="root-bg"
              style={{ position: "absolute", top: 0, left: 0, width: "100%", height: "100%" }}
            />
            <container
              tw="flex flex-row items-center w-full h-full pl-[18px] pr-[18px] gap-[14px] bg-card"
              style={{ position: "relative", borderBottom: "1px solid rgba(233,231,220,0.15)" }}
            >
              <Module bin={TAULER_I3}>
                {(data) => (
                  <container tw="flex flex-row items-end gap-[12px]">
                    {(data?.workspaces ?? []).map((ws) => (
                      <Hoisted ws={ws} />
                    ))}
                  </container>
                )}
              </Module>

              <container tw="flex grow" />

              <Module bin={SIGNALS}>
                {(data) => (
                  <container tw="flex flex-row items-center gap-[18px]">
                    {/* The hoist: the last three signals, struck but still
                        flying. History is free here — it is a second array in
                        the same JSON line. */}
                    <container tw="flex flex-col items-end gap-[3px]">
                      <text tw="text-[8px] text-muted-foreground" style={{ letterSpacing: "2px" }}>
                        STRUCK
                      </text>
                      <container tw="flex flex-row gap-[3px]">
                        {(data?.history ?? []).map((code) => (
                          <Sig code={code} faded={true} />
                        ))}
                      </container>
                    </container>

                    <container tw="flex flex-row gap-[5px]">
                      {(data?.flying ?? []).map((code) => (
                        <Sig code={code} faded={false} />
                      ))}
                    </container>

                    <container tw="w-[1px] h-[34px] bg-border" />

                    <container tw="flex flex-col items-end">
                      <text tw="text-[20px] text-foreground">{data?.time ?? "--:--"}</text>
                      <text tw="text-[9px] text-muted-foreground" style={{ letterSpacing: "2px" }}>
                        {data?.date ?? ""}
                      </text>
                    </container>
                  </container>
                )}
              </Module>
            </container>
          </container>
        </Panel>
      </I3Layout>
    </root>
  );
}
