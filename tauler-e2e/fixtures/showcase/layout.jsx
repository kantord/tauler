// Scenario: what a tauler desktop is supposed to look like when someone has
// actually finished it — one floating bar over a wallpaper tauler painted
// itself, with the terminals reading that same wallpaper through
// _XROOTPMAP_ID.
//
// This is a Scenario like any other, not a marketing render: its expected gaps
// and panel geometry are written out by hand in tauler-e2e/tests/scenarios.rs,
// and it fails the same way sidebar and three-edge do. See docs/adr/0015.
//
// Geometry, since three numbers here have to agree with that file:
//
//   panel   1920×58 at 0,0        — reserved, so i3 tiles below it
//   inset   12px                  — wallpaper, via root-bg, on all four sides
//   card    1896×34 at 12,12      — the bar you actually see
//
// 12 is also i3's inner gap (see i3.config), so the space around the bar and
// the space between windows are the same distance. That is most of what makes
// the arrangement look deliberate rather than assembled.

// <root>, <Panel>, <I3Layout> and <Module> are globals; the component library
// is not — each component is an ES module, and <Icon> without this line is a
// runtime "Icon is not defined", not a render with a missing glyph.
import { Icon } from "@ui/icon";

const TAULER_I3 = "/usr/local/bin/tauler-i3";
const STATUS = "/fixtures/showcase/bin/status";

// One workspace pill.
//
// Fixed width and `justify-center`, never horizontal padding. A padded flex
// container does not size to its text: `px-[10px]` around a single "2" produces
// a 40px box with 11px of space on the left and 23px on the right, and the same
// 40px box for "22" — the text node inside a padded container measures 20px
// wide whatever it contains. The glyph then sits visibly left of centre. Giving
// the pill its own width sidesteps the measurement entirely, and uniform pills
// are what a workspace strip wants anyway.
const PILL = "flex flex-row items-center justify-center w-[24px] h-[22px] rounded-2xl";

function Workspace({ ws }) {
  if (ws.focused) {
    return (
      <container tw={`${PILL} bg-primary`}>
        <text tw="text-[12px] text-primary-foreground">{ws.name}</text>
      </container>
    );
  }
  // Occupied but unfocused reads brighter than empty, so the strip shows where
  // the work is, not just where the cursor is.
  const tone = (ws.focused_windows ?? []).length
    ? "text-foreground"
    : "text-muted-foreground";
  return (
    <container tw={`${PILL} bg-secondary`}>
      <text tw={`text-[12px] ${tone}`}>{ws.name}</text>
    </container>
  );
}

// Icon plus value. No borders and no chip backgrounds: at this size they turn a
// status readout into a row of buttons.
function Stat({ icon, value, tone }) {
  return (
    <container tw="flex flex-row items-center gap-[6px]">
      <Icon name={icon} tw={`text-[13px] ${tone}`} />
      <text tw="text-[12px] text-foreground">{value}</text>
    </container>
  );
}

function Divider() {
  return <container tw="w-[1px] h-[14px] bg-border" />;
}

export default function render() {
  return (
    <root>
      {/* The image is authored at exactly 1920×1080, so it needs no fitting —
          a wallpaper always covers its output exactly and the sizes already
          agree. Anything else would be `object-fit` on this same node.

          PNG, and not by preference: takumi-core builds the `image` crate with
          only `png` and `ico` enabled, so a JPEG here decodes to nothing. The
          failure is silent — `preload_layout_images` drops the error — and
          presents as a black desktop with no log line to explain it. */}
      {/* `id` is required: omitting it fails the whole root parse, not just
          this node. */}
      <wallpaper id="desktop">
        <image
          src="/fixtures/showcase/wallpaper.png"
          style={{ width: "100%", height: "100%" }}
        />
      </wallpaper>

      <I3Layout module={TAULER_I3}>
        <Panel id="bar" anchor="top" size={58}>
          {/* The sanctioned root-bg shape: one absolutely-positioned <image>
              under content that stays `relative`. An <image> node rather than
              backgroundImage — see the layout-file docs, the background-image
              path redoes per-pixel setup and costs ~3× as much. */}
          <container style={{ position: "relative", width: "100%", height: "100%" }}>
            <image
              src="root-bg"
              style={{
                position: "absolute",
                top: 0,
                left: 0,
                width: "100%",
                height: "100%",
              }}
            />
            <container tw="flex h-full w-full p-[12px]" style={{ position: "relative" }}>
              <container tw="flex flex-row h-full w-full items-center justify-between rounded-2xl border border-border bg-card pl-[10px] pr-[10px]">
                {/* Left: real workspaces, and the real title of the focused
                    window. The same subprocess <I3Layout> registers above — one
                    bin, one process, union of props. */}
                <Module bin={TAULER_I3}>
                  {(data) => {
                    const wss = data?.workspaces ?? [];
                    const focused = wss.filter((w) => w.focused)[0];
                    const title = (focused?.focused_windows ?? [])[0];
                    return (
                      <container tw="flex flex-row items-center gap-[10px]">
                        <Icon name="md-layers_triple" tw="text-[15px] text-primary" />
                        <Divider />
                        <container tw="flex flex-row items-center gap-[4px]">
                          {wss.map((ws) => (
                            <Workspace ws={ws} />
                          ))}
                        </container>
                        {title ? (
                          <text tw="text-[12px] text-muted-foreground">{title}</text>
                        ) : null}
                      </container>
                    );
                  }}
                </Module>

                {/* Right: frozen numbers. Structure is real here, cosmetics are
                    not — a live clock would make every pull request's
                    screenshot differ for no reason.
                    The readout sits on its own inset surface so the bar reads as
                    two groups rather than one long row of loose text. */}
                <Module bin={STATUS}>
                  {(data) => (
                    <container tw="flex flex-row items-center gap-[10px]">
                      <container tw="flex flex-row items-center gap-[14px] rounded-2xl bg-secondary h-[26px] pl-[12px] pr-[12px]">
                        {/* The glyph names are the wrong way round: md-memory
                            draws a processor die and md-chip draws a RAM
                            stick. Matched to what they look like, not to what
                            they are called. */}
                        <Stat icon="md-memory" value={data?.cpu ?? "--"} tone="text-[#c4a7e7]" />
                        <Stat icon="md-chip" value={data?.mem ?? "--"} tone="text-[#9ccfd8]" />
                        <Stat icon="md-volume_high" value={data?.vol ?? "--"} tone="text-[#f6c177]" />
                        <Stat icon="md-battery_70" value={data?.bat ?? "--"} tone="text-[#ea9a97]" />
                      </container>
                      <container tw="flex flex-row items-center gap-[6px]">
                        <Icon name="md-clock_outline" tw="text-[13px] text-primary" />
                        <text tw="text-[13px] text-foreground">{data?.time ?? "--:--"}</text>
                      </container>
                    </container>
                  )}
                </Module>
              </container>
            </container>
          </container>
        </Panel>
      </I3Layout>
    </root>
  );
}
