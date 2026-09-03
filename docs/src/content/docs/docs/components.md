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
| **Display** | its props, as pixels | props | `<Card>`, `<Badge>`, `<Progress>`, `<Table>`, `<Icon>` |
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
`<Icon>`.

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
