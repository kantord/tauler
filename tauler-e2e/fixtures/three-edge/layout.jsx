// Scenario: a sidebar, then a top bar and a bottom bar that span only the space
// beside it. Declaration order is what decides that, so this fixture is the one
// that breaks if order stops being the API.

function Status() {
  return (
    <Module bin="/usr/local/share/e2e/bin/e2e-module-status">
      {(data) => (
        <container tw="flex flex-col gap-1 rounded-lg border px-3 py-2">
          <text tw="text-[10px] text-foreground opacity-60">STATUS</text>
          <text tw="text-[15px] text-foreground">{data?.time ?? "--:--"}</text>
          <text tw="text-[11px] text-foreground opacity-70">{data?.host ?? "no data"}</text>
        </container>
      )}
    </Module>
  );
}

export default function render() {
  return (
    <root>
      <I3Layout module="/usr/local/bin/tauler-i3">
        <Panel id="sidebar" anchor="left" size={272}>
          <container tw="flex flex-col h-full w-full gap-4 px-4 py-4 bg-background">
            <text tw="text-[18px] text-foreground">tauler</text>
            <Status />
          </container>
        </Panel>
        <Panel id="topbar" anchor="top" size={26}>
          <container tw="flex flex-row h-full w-full items-center px-3 bg-background">
            <text tw="text-[12px] text-foreground">workspace 1</text>
          </container>
        </Panel>
        <Panel id="bottombar" anchor="bottom" size={26}>
          <container tw="flex flex-row h-full w-full items-center px-3 bg-background">
            <text tw="text-[12px] text-foreground opacity-70">tauler-e2e</text>
          </container>
        </Panel>
      </I3Layout>
    </root>
  );
}
