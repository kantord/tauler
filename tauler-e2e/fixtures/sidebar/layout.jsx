// Scenario: one left-anchored sidebar.
//
// Expectations for this fixture live in tauler-e2e/tests/scenarios.rs, written
// out by hand. Deriving them from the same arithmetic the implementation uses
// would assert only that the code equals itself.

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
      </I3Layout>
    </root>
  );
}
