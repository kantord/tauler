---
title: Components
description: The three kinds of component, what tells them apart, and which of the shipped ones is which.
---

A component is a plain JavaScript function that takes props and returns a tree. There is
nothing to register — JSX turns `<Card />` into a function call already. Tauler ships a set
of them as `@ui/*` modules, and the [component reference](/docs/component-reference/) lists
every one with a rendered example.

```jsx
import { Card, CardContent } from "@ui/card";
import { Progress } from "@ui/progress";

<Card>
  <CardContent>
    <Progress value={load} />
  </CardContent>
</Card>
```

What separates components is their **shape**, not where they come from. There are three
kinds:

| kind | draws | takes | ships as |
|---|---|---|---|
| **Data** | nothing | a function as its child, and calls it with data | `<Module>` |
| **Display** | its props, as pixels | props | `<Card>`, `<Badge>`, `<Progress>`, `<Table>`, `<Icon>`, `<D3Figure>` |
| **Control** | a value, as pixels — and emits intents when you touch it | a `value` and an `on_change` | `<Slider>`, `<Knob>`, `<ScrollArea>` |

Exactly one applies to a component. Where two seem to, the earlier row wins: a component
that hands data to a render prop is a Data component however much frame it draws around the
result, and a component with an `on_change` is a Control whatever else it displays.

## Data components

A Data component renders no pixels. Its child is a function, and the component's whole job
is to call that function with data and return whatever comes back.

`<Module>` is the one that ships. It owns a subprocess, reads JSON from it, and hands the
latest value — plus a proxy for talking back — to its child:

```jsx
<Module bin="~/.cargo/bin/tauler-i3">
  {(data, events) => (
    <WorkspaceList workspaces={data?.workspaces} events={events} />
  )}
</Module>
```

The kind is defined by the shape, not by the subprocess. A function that only reshapes what
it is given is a Data component too:

```jsx
function Peak({ samples, children }) {
  return children(Math.max(0, ...samples));
}

<Peak samples={load}>
  {peak => <Progress value={peak * 10} />}
</Peak>
```

Because every stream value is computed at the top of the render function, a Data component
is never the only way to reach a value — it is a way to scope one. The
[data page](/docs/data/#module) covers `<Module>` and everything it does with the
subprocess behind it.

## Display components

A Display component renders the props it is given and nothing else. Most of what ships is
one: `<Card>` and its parts, `<Badge>`, `<Progress>`, `<Table>` and `<DataTable>`,
`<Icon>`. To draw from data yourself, use a [`<D3Figure>`](#d3figure).

A wrapper that only decorates its children, like `<Card>`, is a Display component too —
wrapping is not a kind of its own. Underneath, every Display component is HTML elements,
so `class` reaches it the same way it reaches a `<div>`; see
[Elements and styling](/docs/elements/).

```jsx
import { Badge } from "@ui/badge";

<Badge variant={failed ? "destructive" : "secondary"}>
  <span>{failed ? "failing" : "passing"}</span>
</Badge>
```

A Display component has no way to react to a click of its own. For that, put an `on_click`
on an element inside it — or reach for a Control.

## Control components

A Control — `<Slider>`, `<Knob>` and `<ScrollArea>` are the three that ship — is a Display
component that also emits intents when you press or drag it. It does not remember what you
did. There is no `useState` behind it and no value store in the runtime: the value it shows
is the value you pass in, read fresh from a stream on every tick, and a press only changes
what you see once the process that owns the value has said so.

```jsx
import { Slider } from "@ui/slider";

<Module bin="~/.cargo/bin/tauler-audio">
  {(data, events) => (
    <Slider
      value={data?.volume ?? 0}
      step={5}
      on_change={v => events.setVolume({ volume: v })}
    />
  )}
</Module>
```

The round trip is the whole design. `on_change` builds an intent, the intent reaches
`tauler-audio` over stdin, `tauler-audio` changes the volume and emits the new one, and the
next tick renders it. Nothing is optimistic, and there is never a second answer to "what is
the volume".

That also means a control needs somebody to talk to. A slider that only filtered a chart on
screen would still need a Module, a stream or `globals` behind it, because it has nowhere
of its own to keep the number.

`on_change` takes the value the pointer is over and returns intents — one, or an array of
them, the same shape every [event handler](/docs/data/#event-handlers) takes. `<Slider>`
does the arithmetic from `min`, `max` and `step`, so a module receives a number in its own
units and never sees a coordinate. Omit `on_change` and a Control still renders — it is
simply not interactive.

### Position or displacement

`<Slider>` and `<Knob>` read the pointer differently, and the difference is worth knowing
before you write a third.

`<Slider>` reads a **position**: what value is under the pointer. That needs a scale, which
is what `min` and `max` are, and it means pressing a slider jumps it to where you pressed —
which is what a slider should do.

`<Knob>` reads a **displacement**: how far the pointer has come since the press. That needs
no scale at all, which is why it has no `min` and no `max`, and it means pressing a knob
anywhere turns it by nothing. It follows your hand instead of jumping to it.

```jsx
import { Knob } from "@ui/knob";

<Module bin="~/.cargo/bin/tauler-audio">
  {(data, events) => (
    <Knob
      value={data?.balance ?? 0}
      step={5}
      on_change={deg => events.setBalance({ deg })}
    />
  )}
</Module>
```

`<Knob>` reports an angle in degrees — 0 points up, and it grows clockwise. The angle wraps
into 0–360, so turning past the top comes round rather than running off the end. It does not
count whole turns; there is no scale for them to mean anything on.

### Dragging

`<Slider>` is draggable: press and sweep, and the value follows. It works the way a slider
on a web page works — pressing **captures** the pointer, so every movement until you let go
goes to that slider whatever you slide over, and dragging off the panel and back on resumes
where the pointer is.

Nothing else drags unless you ask. Any element can, with `on_drag` — which is how you write
a Control of your own:

```jsx
<div on_drag={p => [lights.setLevel({ level: Math.round(p.x / p.width * 100) })]} />
```

`on_drag` fires on the press as well as on every movement after it, so a plain click with no
movement still does something and a control needs only one handler. Leave it off for
buttons — a workspace switcher that answered a drag would switch to every workspace the
pointer crossed.

The handler receives the pointer's position **relative to the element**, in CSS pixels:

| field | meaning |
|---|---|
| `p.x`, `p.y` | offset from the element's top-left. **Negative** above or left of it, and past `width`/`height` beyond it — clamp if you want clamping |
| `p.press_x`, `p.press_y` | where the button went down, in the same coordinates. Subtract to get how far the drag has come; on the press itself it equals `p.x`, `p.y` |
| `p.width`, `p.height` | the element's own size |
| `p.buttons` | bitmask of held buttons, `1` for primary |

There is no speed and no per-event delta — those two points are the whole story, so the same
gesture always gives the same result however fast you made it.

Nothing is dispatched when a movement produces the intents that were just sent, so a drag
costs one message per distinct value, not one per pixel. Giving a control a `step` is what
keeps that number small.

Drag is X11-only for now. On Wayland and macOS no movement is reported, so a slider still
works as click-to-set — and a knob, which has nothing to read but the movement, does
nothing at all there.

## D3Figure

`<D3Figure>` is a Display component you draw yourself, with [d3](https://d3js.org). Its
`draw` function receives an empty `<svg>` of the figure's size and d3 itself, and writes
d3 the way d3's own documentation does: `svg.append(...)`, data joins, scales, shapes,
axes, force layouts.

```jsx
function LoadChart({ samples }) {
  if (!samples.length) return null;
  const width = 240, height = 80;
  return (
    <D3Figure
      key={"load/" + samples.at(-1).t}
      width={width}
      height={height}
      draw={(svg, d3) => {
        const x = d3.scaleTime()
          .domain(d3.extent(samples, d => new Date(d.t * 1000)))
          .range([24, width - 8]);
        const y = d3.scaleLinear()
          .domain([0, d3.max(samples, d => d.v)]).nice()
          .range([height - 18, 4]);
        svg.append("path")
          .datum(samples)
          .attr("fill", "none")
          .attr("stroke", "steelblue")
          .attr("d", d3.line(d => x(new Date(d.t * 1000)), d => y(d.v)));
        svg.append("g")
          .attr("transform", `translate(0,${height - 18})`)
          .attr("color", "gray")
          .call(d3.axisBottom(x).ticks(3));
      }}
    />
  );
}
```

The samples come from a stream, the way any history does:

```jsx
const load = useJSONStream("/bin/sh", `
  while :; do
    printf '{"t":%s,"v":%s}\\n' "$(date +%s)" "$(cut -d' ' -f1 /proc/loadavg)"
    sleep 5
  done | tauler-accumulate -n 60
`) ?? [];

<LoadChart samples={load} />
```

`<D3Figure>` needs no import. It takes four props, all required:

| prop | type | description |
|---|---|---|
| `key` | string, number, boolean, or a JSON object or array | names this drawing; see below |
| `width` | number | width in logical pixels |
| `height` | number | height in logical pixels |
| `draw` | `(svg, d3, { name }) => void` | draws into `svg` |

`svg` is a d3 selection of an `<svg>` with that `width` and `height`, a `viewBox` of
`0 0 width height` and `font-family: sans-serif`. `d3` is d3 7.9.0.

`name` is the name tauler gives this drawing. It is different for each key and size, so use
it for ids inside the drawing, such as a clip-path's. A page has a single DOM, so two
figures that both use a fixed id such as `"clip"` can take each other's.

`width` and `height` must be numbers greater than 0. A string such as `"248"` fails with an
error that names them.

### Change the key when the drawing should change

A figure whose key does not change is not drawn again until the layout file is saved or
its `width` or `height` changes: tauler does not run `draw` to see whether its output
changed. Give the figure a new key when what it shows changes,
and the simplest way to get one is to derive it from the data, as `LoadChart` does from its
newest sample. A figure with no `key` fails with an error that names it.

A key must follow from what the render sees: the data, the streams and the globals. A key
built from `Math.random()`, `Date.now()` or state kept
at module scope is not drawn on the desktop, and tauler logs a warning.

A key can also be an object or an array. tauler names it by a hash of its JSON, so you can
pass the figure's inputs directly:

```jsx
<D3Figure key={{ samples, theme }} width={240} height={80} draw={...} />
```

Key order matters, because `JSON.stringify` writes keys in the order they were added:
`{ a: 1, b: 2 }` and `{ b: 2, a: 1 }` are two drawings. A key that holds a function,
`undefined`, a symbol, `NaN`, `Infinity` or `-Infinity`, a `BigInt`, a cycle, a `Date`, an
array hole, or an object that is not plain (a `Map`, a `Set`, a class instance) fails with an
error that names where in the key it is. JSON would drop or change such a value, so two
different keys could name the same drawing.

An object key costs one `JSON.stringify` per tick, on the desktop twice: once in the bar and
once where the figure is drawn. For 500 rows of 3 fields, that is about 0.8 ms per figure.
For large data, a string key you make yourself, such as the newest timestamp, is cheaper.

The key is the only thing tauler compares. A `draw` that reads anything not in the key still
draws a stale picture when that thing changes.

### What a slow drawing looks like

On the desktop, `draw` runs off the bar's own loop. A slow drawing shows up a little later;
the bar does not freeze while it is drawn. Until a figure's drawing arrives, the figure
shows its previous drawing, or nothing.

A `draw` that runs longer than 5 seconds is stopped. A `draw` that throws leaves the
previous drawing in place and logs a warning once.

### Pass the data in

`draw` has no network, no filesystem and no shell. `d3.json` and `d3.csv` cannot fetch
anything. Read the data in the layout file — from a [stream](/docs/data/) — and pass it to
the figure.

Timers never fire, so nothing animates. Transitions do not play. Run a force simulation to
rest with `.tick()` instead of letting it run:

```jsx
const simulation = d3.forceSimulation(nodes)
  .force("link", d3.forceLink(links).id(d => d.id))
  .force("charge", d3.forceManyBody())
  .force("center", d3.forceCenter(width / 2, height / 2))
  .stop();
simulation.tick(300);
```

### Text cannot be measured

On the desktop, `getBBox`, `getComputedTextLength` and `getBoundingClientRect` measure
nothing. Place labels from your scales, not from the size of the text.

### The first figure downloads d3

The first figure on a machine downloads d3 and linkedom — the projects' own published
files, checked against pinned hashes — into `~/.cache/tauler/d3`. Until the download lands,
figures draw nothing. Every later run uses the cached copy.

On a machine with no network, put the same two files in a directory and point
`TAULER_D3_DIR` at it: `d3.min.js` from the `dist/` folder of npm's `d3@7.9.0` package, and
linkedom 0.18.14's `worker.js`, saved as `linkedom.js`. They are checked against the same
hashes, so any other version is refused.

### d3 outside a figure

A layout file can import d3 for its maths — scales, formats — outside any figure:

```jsx
import * as d3 from "d3";

const percent = d3.format(".0%");
```

Named imports work too. The first such import triggers the same download, and the layout
loads once it lands, like a [Package](/docs/packages/).

### In a browser

On this site's live examples, `draw` runs against the page's real DOM and the drawing stays
in the page as an inline `<svg>`. Text measurement works there. `import "d3"` at the top of
a layout file does not work in a browser yet.

## What has no kind

Three things in a layout file look like components and are not one of the three:

- **Shell nodes** — `<root>`, `<panel>`, `<wallpaper>`. They are lowercase, describe
  structure rather than content, and never reach the rasterizer. See
  [The layout file](/docs/layout-file/#nodes).
- **`<I3Layout>`** produces shell nodes — the panels it lays out — rather than pixels or
  data, so none of the kinds applies. See [Screen layout](/docs/layout/).
- **Units.** `unit()` returns a component, but using one declares something that should be
  true in the world — a window's workspace, a light's state — and draws nothing. See
  [Units](/docs/units/).
