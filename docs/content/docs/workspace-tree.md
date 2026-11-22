---
title: Build a collapsible workspace tree
description: A sidebar for i3 or sway that groups workspaces named project:task into a tree whose groups open and close.
---

This page builds a left sidebar that lists workspaces as a tree. Workspaces named
`tauler:fix-gaps`, `tauler:review-12` and `docs:landing` appear under two headings,
`tauler` and `docs`, and a click on a heading opens or closes its group. It takes four
steps and about fifty lines.

It needs i3 or sway, [`tauler-i3`](/docs/modules/tauler-i3/) installed, and a
[layout file](/docs/layout-file/). Workspaces are named in your i3 or sway config or with
`rename workspace`. A name without a colon goes into a group called `other`.

## Split the names into groups

`tauler-i3` reports each workspace as `{ name, focused, urgent, focused_windows }`. Put
the helpers and a `Tree` component above `render` in your layout file. `Tree` groups the
workspaces by the text before the first colon and keeps the order i3 reports them in.

```jsx
function projectOf(name) {
  const i = name.indexOf(":");
  return i < 0 ? "other" : name.slice(0, i);
}

function taskOf(name) {
  const i = name.indexOf(":");
  return i < 0 ? name : name.slice(i + 1);
}

function Group({ project, workspaces, events }) {
  return (
    <div class="flex flex-col">
      <div class="px-2 py-1 text-foreground">{project}</div>
      {workspaces.map((ws) => (
        <div
          key={ws.name}
          class={"px-2 py-1 pl-6 " + (ws.focused ? "bg-accent text-foreground" : "text-foreground/60")}
          on_click={[events.switchWorkspace({ workspace: ws.name })]}
        >
          {taskOf(ws.name)}
        </div>
      ))}
    </div>
  );
}

function Tree({ workspaces, events }) {
  const groups = new Map();
  for (const ws of workspaces ?? []) {
    const project = projectOf(ws.name);
    if (!groups.has(project)) groups.set(project, []);
    groups.get(project).push(ws);
  }
  return (
    <div class="flex flex-col">
      {[...groups].map(([project, list]) => (
        <Group key={project} project={project} workspaces={list} events={events} />
      ))}
    </div>
  );
}
```

Each task row sends `switchWorkspace` with the full name when clicked. `workspaces` is
`undefined` until `tauler-i3` writes its first line, so `Tree` falls back to an empty
list. The `key` on each `Group` matters in the next step.

## Put it in a panel

Reserve a left panel with [`<I3Layout>`](/docs/layout/#i3layout) and feed `Tree` from
the module:

```jsx
export default function render() {
  return (
    <root>
      <I3Layout module="~/.cargo/bin/tauler-i3">
        <Panel id="sidebar" anchor="left" size={240}>
          <div class="flex flex-col h-full w-full py-2 bg-background">
            <Module bin="~/.cargo/bin/tauler-i3">
              {(data, events) => <Tree workspaces={data?.workspaces} events={events} />}
            </Module>
          </div>
        </Panel>
      </I3Layout>
    </root>
  );
}
```

Save the file. The sidebar shows every group open, and clicking a task switches to that
workspace.

## Make a group collapse

Whether a group is open is something to remember between ticks, so it goes in a
[Store](/docs/data/#store). A Store declared inside a component belongs to that
component, and each `<Group key=...>` gets its own. Replace `Group` with:

```jsx
import { store } from "tauler";

function Group({ project, workspaces, events }) {
  const s = store({ open: true });
  return (
    <div class="flex flex-col">
      <div
        class="flex flex-row gap-2 px-2 py-1 text-foreground"
        on_click={() => { s.open = !s.open; }}
      >
        <span>{s.open ? "-" : "+"}</span>
        <span>{project}</span>
        <span class="text-foreground/40">{workspaces.length}</span>
      </div>
      {s.open && workspaces.map((ws) => (
        <div
          key={ws.name}
          class={"px-2 py-1 pl-6 " + (ws.focused ? "bg-accent text-foreground" : "text-foreground/60")}
          on_click={[events.switchWorkspace({ workspace: ws.name })]}
        >
          {taskOf(ws.name)}
        </div>
      ))}
    </div>
  );
}
```

Put the `import` at the top of the file, after any frontmatter. A click on a heading
writes `s.open`, which causes one tick, and the group's rows disappear or come back. A
handler that only writes a Store returns no intents. The `key` on `<Group>` ties each
Store to its project name, so a group keeps its state when another group appears above
it.

## Show what a closed group hides

A closed group hides the focused workspace and any urgent one. Dim the heading unless it
holds the focused workspace, and add a mark when it holds an urgent one. Add two lines
before `return`, and change the heading to use them:

```jsx
const urgent = workspaces.some((ws) => ws.urgent);
const hasFocus = workspaces.some((ws) => ws.focused);
```

```jsx
<div
  class={"flex flex-row gap-2 px-2 py-1 " + (hasFocus ? "text-foreground" : "text-foreground/60")}
  on_click={() => { s.open = !s.open; }}
>
  <span>{s.open ? "-" : "+"}</span>
  <span>{project}</span>
  <span class="text-foreground/40">{workspaces.length}</span>
  {urgent && <span class="text-destructive">!</span>}
</div>
```

## Limits

- The tree lists the workspaces of the primary output only, because that is what
  `tauler-i3` reports. See [The init event and outputs](/docs/modules/tauler-i3/#the-init-event-and-outputs).
- The list refreshes when i3 or sway reports a workspace or window event, and at least
  once a second.
- Open and closed state lasts until Tauler restarts. It survives saving the layout file,
  but not a group that has no workspaces: when the last `tauler:*` workspace closes, the
  group disappears and starts open again when one returns.
- A tree taller than the panel is cut off. Tauler has no scrolling here beyond
  [`<ScrollArea>`](/docs/components/#control-components), which this page does not use.
- Only the first colon splits a name, so `a:b:c` is task `b:c` in group `a`.
- This page was checked against Tauler's own renderer and click handling, not on a
  running i3 or sway session.

## Screenshots

`tauler-screenshot` runs no streams, so it cannot draw the sidebar from live data. To
preview the tree, render a file that passes `Tree` a fixed list. See
[Screenshots](/docs/develop/screenshots/).
