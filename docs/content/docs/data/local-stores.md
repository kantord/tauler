---
title: Encapsulate state in a component
description: Give each copy of a component its own remembered value.
template: article
---

Declare a store inside a component, and every place the component is written gets its
own. A store declared inside a function is a **local store**.

```jsx
import { store } from "tauler";

function Disclosure({ title, children }) {
  const s = store({ open: false });
  return (
    <div>
      <div on_click={() => { s.open = !s.open; }}>{title}</div>
      {s.open && children}
    </div>
  );
}

export default () => (
  <root>
    <panel id="side" anchor="left" width={240} height={ctx.screen_height}>
      <div class="flex flex-col">
        <Disclosure key="mail" title="Mail"><div>3 unread</div></Disclosure>
        <Disclosure key="calendar" title="Calendar"><div>Nothing today</div></Disclosure>
      </div>
    </panel>
  </root>
);
```

## How it works

A local store is read and written like a [shared store](/docs/data/shared-stores/), and
a write from a handler runs the layout again in the same way. What differs is whose it
is. Tauler names a local store by where its component is written in the tree, so two
`<Disclosure>` elements hold two values. A `key` says which thing an element is: the
same key keeps what was there, and a new key is a new thing that starts over. A store
belongs to the component whose code writes `<Disclosure … />`, not to the element it ends
up inside, and it lasts as long as that component keeps being written.

## Common tasks

### Keep a store for each row of a list

Give each row a `key`. A row keeps its store when the list is reordered or another row
appears in front of it:

```jsx
{rows.map((row) => <Row key={row.id} row={row} />)}
```

### Write a reusable function that declares a store

Any function can declare a store, not only a component. Assign each call to its own
`const`, and each call has its own store:

```jsx
// lib/toggle.js
import { store } from "tauler";

export function toggle() {
  const s = store({ open: false });
  return { open: s.open, flip: () => { s.open = !s.open; } };
}

// components/Menu.jsx
import { toggle } from "../lib/toggle.js";

export function Menu() {
  const menu = toggle();
  const help = toggle();
  return <div on_click={menu.flip}>{menu.open ? "close" : "menu"}</div>;
}
```

### Turn a reusable function into a shared store

Call it through a `const` at the top level of a file. `sidebar` is then one store for
everything that imports it:

```jsx
export const sidebar = toggle();
```

### Keep a value when the component goes away

A local store is gone when its component is not written. Keep what must outlive that in
a store at the top level of a file:

```jsx
const drafts = store({ text: "" });

function Drawer() {
  return <div>{drafts.text}</div>;
}
```

### Start a component over

Change its `key`. The component with the new key starts from its default:

```jsx
<Editor key={"doc/" + doc.id} doc={doc} />
```

## Rules

**When one component writes another more than once, give each a `key`.** A `key` is a
string or a number, and the component also receives it as a prop. Without keys the same
declaration is reached twice, and the run throws.

**A row whose `key` changes loses its store.** A new key is a new thing. A
[`<D3Figure>`](/docs/components/#d3figure) follows the same rule: a new `key` is a new
drawing.

**A component that is not written on a run loses its stores at once.** When a drawer
closes or a row leaves the list, what was kept beneath it is gone, and it starts from its
default when it comes back.

**The declaration must be the whole value of a `const`.** An `if` or a loop around it is
fine.

**The function it is in must have a name, and so must every function around that one.**
`function Row() {…}` or `const Row = () => {…}`. A handler or a `.map()` callback written
inline cannot declare a store, and neither can an `async` function. These fail the load
with the file and the line.

**A component with a store in it, or beneath it, must be declared at the top level of its
file.** One defined inside another function throws when it runs.

**One store is declared once per run.** If the same declaration is reached twice (two of
one component with no `key`, a function called twice without a `const` of its own, a call
in the body of a loop) the run throws and the error names the store. Make the repeated
part a component with a `key`.

**Do not destructure the call of a function that declares a store.**
`const { open, flip } = toggle();` throws, at the top level of a file too. Assign the
call to a plain `const` first, then destructure that:
`const menu = toggle(); const { open, flip } = menu;`. A function that declares no store
can be destructured as usual.

**Declare while a component runs.** A function that declares a store throws when a
handler calls it. Declare the store in the component and use it in the handler.

**Hooks and draw code cannot declare a store.** A Unit's hooks and a `<D3Figure>`'s
`draw` can read a store their component declared.

**Renaming starts over what was named after it.** That covers the component, the
function that declares the store, the `const`, and the file. Writing the component
inside a different component does the same.

**A local store does not survive a restart of Tauler.** For that,
[write a program](/docs/data/programs/) that saves the state and use it as a
[service](/docs/data/services/).

## Reference

### `store(defaults)` inside a function

| argument | type | description |
|---|---|---|
| `defaults` | object | The value each copy starts with. JSON only. |

Returns the store for this place in the tree. The same function as for a
[shared store](/docs/data/shared-stores/#reference), imported from `"tauler"`.

### `key`

| prop | type | description |
|---|---|---|
| `key` | string or number | Which thing this element is, among the elements one component writes. |

### Behaviour

- **Before the first value.** A local store has its default the first time its component
  is written.
- **When it updates.** A write from a handler runs the layout again, once.
- **On errors.** A handler or a run that throws changes no store.
- **On a reload.** It keeps its value when the layout file is saved, as long as the
  component is written by the first run after the reload. A reload starts every program
  again, so that first run has no stream values yet: a component that is only written
  once a stream has a value starts over.

## Related

- [Share state across the layout](/docs/data/shared-stores/): reading, writing and what
  causes a run.
- [`<D3Figure>`](/docs/components/#change-the-key-when-the-drawing-should-change): what
  `key` means for a drawing.
- [Components](/docs/components/#control-components): a reusable control that holds its
  own value declares a local store.
