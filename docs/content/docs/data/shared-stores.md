---
title: Share state across the layout
description: Keep a value between runs of the layout, and read or change it from any file.
template: article
---

Declare an object at the top level of a file, and Tauler keeps it between runs of the
layout: whether a drawer is open, which tab is selected, the value of a slider that no
program owns. An object kept this way is a **store**, and one declared at the top level
is shared by every file that imports it.

```jsx
import { store } from "tauler";

const desk = store({ open: false });

export default () => (
  <root>
    <panel id="bar" anchor="top" width={ctx.screen_width} height={32}>
      <div on_click={() => { desk.open = !desk.open; }}>
        {desk.open ? "close" : "open"}
      </div>
    </panel>
  </root>
);
```

## How it works

Everything else in a layout is built again from the current stream values on every run.
A store is where you put what has to be remembered. You read it like any object and
write it with a plain assignment, and a write from a handler makes the layout run again
so the screen shows the new value. Tauler finds a store again by the file that declares
it and the name of its `const`, so you never write a key for it, and two files can both
call theirs `state`.

## Common tasks

### Use one store from several files

Export it. Every file that imports it gets the same one:

```jsx
// state.js
import { store } from "tauler";
export const filters = store({ range: "1h" });

// components/RangePicker.jsx
import { filters } from "../state.js";
export const RangePicker = () => (
  <div on_click={() => { filters.range = "24h"; }}>{filters.range}</div>
);
```

### Change a nested value

Change it in place:

```jsx
desk.pinned.push("mail");
delete desk.note;
```

### Hold the value of a control

`on_change` writes the store, and the next run draws it:

```jsx
import { store } from "tauler";
import { Slider } from "@ui/slider";

const chart = store({ zoom: 50 });

const Zoom = () => (
  <Slider value={chart.zoom} on_change={(v) => { chart.zoom = v; }} />
);
```

### Remember something the data showed

A write made while the layout runs is kept and shows on the next run. This remembers
which workspace was focused last:

```jsx
const seen = store({ last: null });

function LastFocused({ workspaces }) {
  const focused = workspaces.find((ws) => ws.focused);
  if (focused) seen.last = focused.name;
  return <div>{seen.last}</div>;
}
```

### Read a store from a Unit or a figure

A [Unit](/docs/units/#driving-a-unit-from-your-layout)'s hooks and a
[`<D3Figure>`](/docs/components/#change-the-key-when-the-drawing-should-change)'s `draw`
read the same stores your layout does:

```jsx
<D3Figure key={"zoom/" + chart.zoom} width={240} height={80}
          draw={(svg) => svg.append("text").attr("y", 20).text(chart.zoom)} />
```

## Rules

**A store does not survive a restart of Tauler.** It lives in Tauler's memory. For state
that must outlast a restart, [write a program](/docs/data/programs/) that saves it and
use it as a [service](/docs/data/services/).

**`store` is an import, not a global.** A file that calls it without
`import { store } from "tauler"` fails to load, and the error says what to import.

**The call must be the whole value of a `const`.** `store(...)` in a `let`, as an array
element, or returned from a function fails the load, with the file and the line. So does
a second argument. The name of the `const` is how Tauler finds the store again.

**The default is an object, and what it holds must be JSON.** Numbers, strings,
booleans, `null`, arrays and objects.

**A write from a handler causes one run.** Several handlers that write at once still
cause one, and a handler that leaves every value as it was causes none. A handler that
only writes a store does not have to return events.

**A write made while the layout runs causes no run.** It is kept, and shows on the next
run that something else causes. That is the place for bookkeeping that follows the data.

**A handler or a run that throws leaves every store as it was before it started.**

**Hooks and draw code cannot write a store.** An assignment in a Unit's hook or in a
`<D3Figure>`'s `draw` throws. A write does not run either of them sooner: a Unit sees
the new value at its next Sweep, and a figure is drawn again when its `key` changes.

**Renaming the `const`, or moving it to another file, starts the store from its default
again.** The file and the name are its identity. A store you delete is forgotten.

## Reference

### `store(defaults)`

| argument | type | description |
|---|---|---|
| `defaults` | object | The value the store starts with. JSON only. |

Returns the store: an object you read and assign to. Imported with
`import { store } from "tauler"`. Declared at the top level of a file it is one shared
store; declared inside a function it is a
[local store](/docs/data/local-stores/). A file can declare several stores and export
them.

### Behaviour

- **Before the first value.** A store has its default from the first run.
- **When it updates.** A write from a handler runs the layout again, once. A write
  during a run is kept without causing one.
- **On errors.** A handler or a run that throws changes no store.
- **On a reload.** A store keeps its value when the layout file is saved. A key you add
  to the default appears with its default value, and the keys you already had keep
  theirs. Nothing survives a restart of Tauler.

## Related

- [Encapsulate state in a component](/docs/data/local-stores/): a store for each copy of
  a component.
- [Do something on a click](/docs/interaction/clicks/): the function form of a handler,
  which is where a store is written.
- [Offload state to an external program](/docs/data/services/): for state that another
  program owns or that must outlast Tauler.
- [Control components](/docs/components/#control-components): a control holds no value,
  so a store or a service does.
