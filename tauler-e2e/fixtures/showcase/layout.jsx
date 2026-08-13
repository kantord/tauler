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

// One workspace pill. Focused is a filled lozenge; everything else is a dim
// glyph, so the eye finds the current workspace without reading anything.
function Workspace({ ws }) {
  if (ws.focused) {
    return (
      <container tw="flex flex-row items-center rounded-2xl bg-primary px-[10px] h-[22px]">
        <text tw="text-[12px] text-primary-foreground">{ws.name}</text>
      </container>
    );
  }
  return (
    <container tw="flex flex-row items-center rounded-2xl px-[10px] h-[22px]">
      <text tw="text-[12px] text-muted-foreground">{ws.name}</text>
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
              <container tw="flex flex-row h-full w-full items-center justify-between rounded-2xl border border-border bg-card pl-[8px] pr-[16px]">
                {/* Left: real workspaces. The same subprocess <I3Layout>
                    registers above — one bin, one process, union of props. */}
                <Module bin={TAULER_I3}>
                  {(data) => (
                    <container tw="flex flex-row items-center gap-[2px]">
                      {(data?.workspaces ?? []).map((ws) => (
                        <Workspace ws={ws} />
                      ))}
                    </container>
                  )}
                </Module>

                {/* Right: frozen numbers. Structure is real here, cosmetics are
                    not — a live clock would make every pull request's
                    screenshot differ for no reason. */}
                <Module bin={STATUS}>
                  {(data) => (
                    <container tw="flex flex-row items-center gap-[14px]">
                      <Stat icon="md-chip" value={data?.cpu ?? "--"} tone="text-[#c4a7e7]" />
                      <Stat icon="md-memory" value={data?.mem ?? "--"} tone="text-[#9ccfd8]" />
                      <Stat icon="md-volume_high" value={data?.vol ?? "--"} tone="text-[#f6c177]" />
                      <Stat icon="md-battery_70" value={data?.bat ?? "--"} tone="text-[#ea9a97]" />
                      <Divider />
                      <text tw="text-[13px] text-foreground">{data?.time ?? "--:--"}</text>
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
