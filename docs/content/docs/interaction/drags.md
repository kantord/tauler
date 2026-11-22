---
title: Build a control that drags
description: Turn the pointer's position into a value while the button is held.
template: article
---

Put `on_drag` on an element, and it receives the pointer on the press and on every
movement until the button is released. An element that turns that into a value is a
**control**.

```jsx
import { store } from "tauler";

const lamp = store({ level: 50 });

export default () => (
  <root>
    <panel id="lamp" anchor="top" width={200} height={32}>
      <div class="h-full w-full" on_drag={(p) => {
        lamp.level = Math.max(0, Math.min(100, Math.round(p.x / p.width * 100)));
      }}>{lamp.level}%</div>
    </panel>
  </root>
);
```

## How it works

Pressing an element with `on_drag` captures the pointer: every movement until release
goes to that element, whatever the pointer slides over, the way a slider on a web page
works. Dragging off the panel and back on resumes where the pointer is. The handler
receives two points, where the pointer is and where the button went down, both relative
to the element. A control holds no value of its own. Its handler writes a
[store](/docs/data/shared-stores/) or dispatches to a
[service](/docs/data/services/), and the control draws whatever value comes back on the
next run.

## Common tasks

### Use a shipped control

`<Slider>`, `<Knob>` and `<ScrollArea>` do the arithmetic. `on_change` receives the value
and returns events or writes a store. See
[Control components](/docs/components/#control-components):

```jsx
import { Slider } from "@ui/slider";

<Slider value={audio?.volume ?? 0} step={5}
        on_change={(v) => dispatch.set({ value: v })} />
```

### Send a position to a program

Return a list of events from the handler:

```jsx
<div on_drag={(p) => [lights.setLevel({ level: Math.round(p.x / p.width * 100) })]} />
```

### Read how far the pointer has moved

Subtract the press point. This is what a knob does, and it needs no scale:

```jsx
<div on_drag={(p) => { dial.turn = p.x - p.press_x; }} />
```

### Keep the value inside a range

The position runs past the element's edges, so clamp it:

```jsx
const level = Math.max(0, Math.min(100, Math.round(p.x / p.width * 100)));
```

### Send fewer events

Round to a step. A movement that produces the events just sent sends nothing:

```jsx
<div on_drag={(p) => [lights.setLevel({ level: Math.round(p.x / p.width * 20) * 5 })]} />
```

## Rules

**A control never holds its value.** It shows the value you pass in, read on every run
from the store or service that owns it. A press changes what you see only once the
owner has the new value.

**`on_drag` fires on the press too.** A plain click with no movement still does
something, so a control needs only one handler. Leave `on_drag` off buttons: a workspace
switcher that answered a drag would switch to every workspace the pointer crossed.

**The position is not clamped.** `p.x` and `p.y` are negative above or left of the
element, and larger than `width` or `height` beyond it.

**There is no speed and no per-movement delta.** The two points are the whole story, so
the same gesture gives the same result however fast you made it.

**A drag sends one event per distinct value.** Nothing is dispatched when a movement
produces the events that were just sent. A `step` keeps that number small.

**A drag sends the newest position, not every position.** A slow program gets fewer
positions. See [the rules for dispatch](/docs/data/dispatch/#rules).

**Dragging works on X11 only for now.** On Wayland and macOS no movement is reported, so
a slider still works as click-to-set, and a knob, which reads only the movement, does
nothing there.

**The block-level and topmost-element rules for clicks apply.** See
[the rules for clicks](/docs/interaction/clicks/#rules). A screen reader cannot reach
`on_drag`.

## Reference

### `on_drag={handler}`

| form | type | what it does |
|---|---|---|
| function | `(p) => events \| undefined` | called on the press and on each movement; the list it returns, if any, is sent |
| array | a list of events | sent on the press; the same list is not sent again on movement |

The pointer `p`, **relative to the element**, in CSS pixels:

| field | meaning |
|---|---|
| `p.x`, `p.y` | offset from the element's top-left. **Negative** above or left of it, and past `width`/`height` beyond it |
| `p.press_x`, `p.press_y` | where the button went down, in the same coordinates. Subtract to get how far the drag has come; on the press itself it equals `p.x`, `p.y` |
| `p.width`, `p.height` | the element's own size |
| `p.buttons` | bitmask of held buttons, `1` for primary |

### Behaviour

- **Before the first value.** The handler runs on the press, before any movement.
- **When it updates.** The layout runs again when the handler wrote a store, or when the
  program it dispatched to prints a new line. The drag keeps going to the element that
  was pressed across those runs.
- **On errors.** A handler that throws is logged once and that movement does nothing.
- **On a reload.** Not checked: a reload in the middle of a drag.

## Related

- [Do something on a click](/docs/interaction/clicks/): the two forms of a handler.
- [Control components](/docs/components/#control-components): `<Slider>`, `<Knob>` and
  `<ScrollArea>`, and position against displacement.
- [Share state across the layout](/docs/data/shared-stores/) and
  [Offload state to an external program](/docs/data/services/): where a control's value
  lives.
