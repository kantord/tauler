---
title: Do something on a click
description: Put a handler on an element so that a click sends an event or changes a value.
template: article
---

Put `on_click` on an element, and a click on it does one of two things: it sends events
to other programs, or it changes a value the layout keeps. The value of `on_click` is a
**handler**.

```jsx
import { store } from "tauler";

const desk = store({ clicks: 0 });

export default () => (
  <root>
    <panel id="bar" anchor="top" width={ctx.screen_width} height={32}>
      <div on_click={() => { desk.clicks += 1; }}>clicked {desk.clicks} times</div>
    </panel>
  </root>
);
```

## How it works

A handler has two forms. An array is a list of events, built ahead of time with
[`useDispatch`](/docs/data/dispatch/), and a click sends them as they stand. A function
is called when the click happens: it can write a [store](/docs/data/shared-stores/), and
it can return a list of events to send. On a click, Tauler finds the topmost element
painted over that point that carries a handler, and runs that one handler. The click
itself changes nothing on screen: the layout runs again when a store was written, or
when a program prints a new line.

## Common tasks

### Send an event

Give an array of events:

```jsx
const [i3, dispatch] = useService("~/.cargo/bin/tauler-i3");

<div on_click={[dispatch.switchWorkspace({ workspace: "1: web" })]}>web</div>
```

### Send events to several programs

One gesture can address several programs, one event each:

```jsx
on_click={[
  i3.switchWorkspace({ workspace: ws.name }),
  notify.dismiss({ id: n.id }),
]}
```

### Change a value the layout keeps

Give a function that writes a store. It returns nothing:

```jsx
<div on_click={() => { desk.open = !desk.open; }}>menu</div>
```

### Decide what to send when the click happens

Give a function that returns the list. It receives the pointer:

```jsx
<div on_click={(p) => [player.seek({ fraction: p.x / p.width })]} />
```

### Make a click reach a screen reader

Add `role="button"`, and a label when the text is not what a person should hear. See
[Accessibility](/docs/accessibility/):

```jsx
<div role="button" aria-label="Mute" on_click={[audio.mute()]}>🔇</div>
```

## Rules

**Put handlers on block-level elements.** `on_click` fires only on an element that has a
box of its own: a `<div>`, or any element that is a flex or grid item. A plain inline
element like a `<span>` in a run of text has no box, so a handler on it never fires.

```jsx
// Nothing happens: the span is inline, so it has no box to click.
<p>PRs: <span on_click={[gh.open({ repo })]}>{count}</span></p>

// Works: the div is a flex item, so it has one.
<div class="flex flex-row">
  <div on_click={[gh.open({ repo })]}>{count}</div>
</div>
```

You do not have to guess which case you are in. The first click on the surface logs a
warning that names the element as you wrote it:

```
WARN on_click on a node that is never painted on its own — inline elements
     cannot take clicks; move the handler to a block-level element
     node=<span id="pr-count" class="text-[11px]">
```

**The topmost painted element with a handler takes the click.** An element counts as
carrying a handler if it has `on_click` or `on_drag`, so an element that only drags
still covers a clickable one beneath it. Nothing passes the click on to the elements
below.

**Reach for the array unless you need the pointer or a store.** The array is data and
cannot fail when the click happens.

**A handler that throws does nothing.** It is logged once, no event is sent, and every
store is left as it was.

**Events are sent as written.** Each event goes to its own program as one line of JSON,
with nothing wrapped around it, so a program sees only the events it knows and never
learns that a click caused one. Events for different programs are delivered separately
and in no promised order. An event for a program the layout has not named is logged and
skipped, and the others are still sent.

**A wallpaper takes no clicks.** Handlers work in a `<panel>` only.

**Scroll wheel motion is not a click.** X11 reports it as button presses 4 to 7, and
those are dropped.

## Reference

### `on_click={handler}`

| form | type | what a click does |
|---|---|---|
| array | a list of events, each from `dispatch.name(data?)` | sends each event to its program |
| function | `(p) => events \| undefined` | calls the function, then sends the list it returns, if any |

The function receives the pointer, **relative to the element**, in CSS pixels:

| field | meaning |
|---|---|
| `p.x`, `p.y` | offset from the element's top-left |
| `p.press_x`, `p.press_y` | where the button went down; on a click, equal to `p.x`, `p.y` |
| `p.width`, `p.height` | the element's own size |
| `p.buttons` | bitmask of held buttons, `1` for primary |

### Behaviour

- **Before the first value.** A handler works from the first run.
- **When it updates.** A click causes a run of the layout only through what it changes: a
  store write causes one, and an event causes one when its program prints a new line.
- **On errors.** A handler that throws is logged once, sends nothing and writes nothing.
- **On a reload.** Handlers are built again on every run, so a saved layout file's
  handlers apply from its first run.

## Related

- [Send events to an external program](/docs/data/dispatch/): building the events a
  handler sends, and when they are delivered.
- [Share state across the layout](/docs/data/shared-stores/): what a write from a
  handler does.
- [Build a control that drags](/docs/interaction/drags/): `on_drag`, for a handler that
  follows the pointer.
- [Accessibility](/docs/accessibility/): which clicks a screen reader can make.
- [Elements and styling](/docs/elements/#clicks): which elements have a box.
