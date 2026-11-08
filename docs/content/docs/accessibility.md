---
title: Accessibility
description: What a screen reader sees in a Panel, which props name and activate elements, and what is not supported.
---

On Linux, Tauler shows each Panel to assistive technology — a screen reader such as Orca —
through the platform accessibility API, and lets the reader click buttons. A Panel is drawn
as pixels, so the tree the reader sees is built from your layout file, not from the pixels.
What you write in the layout is what it gets.

## Know the scope

- **Linux only.** On macOS Tauler exposes nothing to assistive technology.
- **Panels only.** Wallpapers have no window and Dom surfaces are markup the browser
  handles, so neither appears in the tree.
- **Nothing is built until a reader attaches.** With no assistive technology running,
  Tauler does no accessibility work.

## Read what the reader gets

Each Panel is one window in the tree, named by the elements inside it. Every element in the
layout becomes a node, nested the way the elements are nested, with its on-screen box as
its bounds. These tags are left out because they produce nothing on screen: `head`, `meta`,
`link`, `style`, `script`, and `br`. A subtree containing an inline `<svg>` is left out too.

A node gets a role and a name:

| Element | Role |
| --- | --- |
| `role="button"` | Button |
| any other `role` value | Generic container |
| `<img>` | Image |
| Has a name, no `role` | Paragraph |
| Anything else | Generic container |

The name is the first of these that exists:

1. the `aria-label` prop,
2. the `title` prop,
3. all the text inside the element, joined together.

Text takes no node of its own. It becomes part of the name of the elements around it, so a
`<div>` holding `{used}%` is a paragraph named by what that text renders as. An `<img>` has
no text, so give it `aria-label` or `title`; `alt` is not read.

Only `role="button"` is understood. Other `role` values do not fail, but they make the node
a generic container, which is also what you get with no `role` at all.

## Make a button

A node is activatable when it has `role="button"` **and** `on_click`. Give it a label when
its text is not what a person should hear:

```jsx
<div
  role="button"
  aria-label="Mute"
  on_click={[{ channel: "~/.cargo/bin/my-audio", event: { type: "mute" } }]}
  class="px-3 py-1 rounded bg-surface text-foreground"
>
  🔇
</div>
```

The reader lists it as a push button named "Mute". Activating it fires the element's
`on_click`.

A `<div on_click>` with no `role` is **not** activatable: it appears as a paragraph or
generic container and a reader cannot click it. Pointer clicks are unaffected. An element
with `role="button"` and no `on_click` is a button node that does nothing when activated.

## Know what Activate does

Activation is dispatched as a click, so [`on_click`](/docs/data/#event-handlers) rules apply
unchanged: the same array of intents, or the same handler function, runs. A handler that
reads the pointer receives a press at the top-left corner of the element, with `x`, `y`,
`press_x` and `press_y` of `0`, the element's real `width` and `height`, and `buttons` of
`1`.

`on_drag` is not reachable by a reader. Only the click action is supported; any other
action is ignored.

Put `role="button"` and `on_click` on a block-level element — a `<div>`, or a `<span>` that
is a flex or grid item. A pointer click cannot reach a handler on any other `<span>` (see
[Clicks](/docs/elements/#clicks)), and such an element has no bounds in the tree, so a
reader cannot show where it is.

## Test it

The repository's end-to-end suite shows the check: a small program connects to the
accessibility bus, lists the tree, and activates the node named "Mute". It confirms the
button appears as a push button and that activating it delivers its `on_click` event to the
module. To check your own layout, point an AT-SPI client at a running Tauler and look for your
names and roles.

## Know the limits

- Only buttons can be activated. There are no links, checkboxes, sliders or text fields.
- `aria-label` and `title` take plain strings. Other `aria-*` props are ignored.
