// A left sidebar for AeroSpace on macOS.
//
// Workspaces come from `tauler-aerospace`, which also takes the clicks: a
// `switchWorkspace` intent runs `aerospace workspace <name>`.
//
// There is no <I3Layout> here. That component computes gaps and registers them
// with i3 at runtime; AeroSpace reads its gaps from aerospace.toml at load time
// and has no command to change them, so the reservation lives in
// ~/.config/aerospace/aerospace.toml and must match RAIL below by hand.

const AEROSPACE = "~/.cargo/bin/tauler-aerospace";

// Keep in step with `gaps.outer.left` in aerospace.toml.
const RAIL = 140;

// macOS keeps its menu bar at the top of every screen and will not give the
// space up, so the rail starts below it rather than fighting for the corner.
const MENU_BAR = 25;

// AeroSpace has no workspace label: a workspace *is* its name, and the only
// field it reports is that string. Naming them "web"/"code" in aerospace.toml
// would work but loses the digit the keybindings use, so the label is the
// bar's business and lives here.
const LABELS = {
  1: "main",
  2: "code",
  3: "web",
  4: "chat",
  5: "media",
};

const ROW = "flex flex-row items-center gap-[8px] w-full h-[26px] px-[8px] rounded-lg";
const BADGE = "flex flex-row items-center justify-center w-[18px]";

function Workspace({ ws, aero }) {
  const occupied = ws.focused_windows.length > 0;
  const row = ws.focused ? `${ROW} bg-primary` : occupied ? `${ROW} bg-secondary` : ROW;
  const text = ws.focused
    ? "text-primary-foreground"
    : occupied
      ? "text-foreground"
      : "text-muted-foreground";

  // The handler goes on the row's own div: a <span> is inline and has no box
  // to be clicked.
  return (
    <div class={row} on_click={[aero.switchWorkspace({ workspace: ws.name })]}>
      <div class={BADGE}>
        <span class={`text-[12px] ${text}`}>{ws.name}</span>
      </div>
      <span class={`text-[11px] ${text}`}>{LABELS[ws.name] ?? ws.apps[0] ?? ""}</span>
    </div>
  );
}

export default function render() {
  const aero = useEvents(AEROSPACE);

  return (
    <root>
      <panel
        id="rail"
        x={0}
        y={MENU_BAR}
        width={RAIL}
        height={ctx.screen_height - MENU_BAR}
        above={true}
      >
        <div class="flex flex-col h-full w-full gap-[6px] px-[8px] py-[10px] bg-background">
          <Module bin={AEROSPACE}>
            {(data) => {
              // AeroSpace declares every workspace in the config, so the full
              // list is 30-odd mostly-empty entries. Showing the occupied ones
              // plus wherever you are keeps the rail the length of the work.
              const all = data?.workspaces ?? [];
              const shown = all.filter((w) => w.focused || w.focused_windows.length);
              return (
                <div class="flex flex-col gap-[4px] w-full">
                  {shown.map((ws) => (
                    <Workspace ws={ws} aero={aero} />
                  ))}
                </div>
              );
            }}
          </Module>
        </div>
      </panel>
    </root>
  );
}
