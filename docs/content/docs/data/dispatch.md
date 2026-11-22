---
title: Send events to an external program
description: Make a click do something outside Tauler by sending a line of JSON to a program.
template: article
---

Name a program, and a click can send it an object that says what to do. Sending is
called a **dispatch**, and the object is an **event**.

The layout:

```jsx
export default function render() {
  const browser = useDispatch("~/.config/tauler/bin/open-url");
  return (
    <root>
      <panel id="links" anchor="top" width={ctx.screen_width} height={32}>
        <div on_click={[browser.open({ url: "https://tauler.dev" })]}>docs</div>
      </panel>
    </root>
  );
}
```

The program, saved as `~/.config/tauler/bin/open-url` and made executable. It reads one
event per line and reports nothing back. On macOS, write `open` in place of `xdg-open`:

```sh
#!/bin/sh
while read -r event; do
  url=$(printf '%s' "$event" | jq -r 'select(.type == "open") | .url')
  [ -n "$url" ] && xdg-open "$url"
done
```

## How it works

`useDispatch` starts the program and returns an object for building events for it.
`browser.open({ url })` sends nothing. It returns an event addressed to that program,
and a handler such as `on_click` takes a list of them. When the click happens, Tauler
writes each event to its program as one line of JSON:
`{"type":"open","url":"https://tauler.dev"}`. The program gets no word about what caused
the event, and the layout gets no reply. If the program has something to say afterwards,
it prints a new line and the layout reads it: that pairing is a
[service](/docs/data/services/).

## Common tasks

### Send an event with no data

Call the property with no argument, and the event is only its `type`:

```jsx
<div on_click={[player.pause()]}>pause</div>
// {"type":"pause"}
```

### Send to two programs with one click

A handler is a list, so name an event for each program:

```jsx
<div on_click={[browser.open({ url }), sounds.play({ name: "click" })]}>docs</div>
```

### Send a value that follows the pointer

A function handler builds the event when the pointer arrives. See
[Build a control that drags](/docs/interaction/drags/):

```jsx
<div on_drag={(p) => [lights.setLevel({ level: Math.round(p.x / p.width * 100) })]} />
```

### Ship the program beside the layout file

`import.meta.dirname` is the directory of the file that reads it:

```jsx
const browser = useDispatch(import.meta.dirname + "/bin/open-url");
```

## Rules

**A dispatch gets no reply.** Nothing tells the layout that an event arrived or what it
did. Show the result by reading the program's next line.

**A click is always sent.** Every event a click produces is written to its program, in
the order the clicks happened for that program.

**A drag sends the newest position, not every position.** While the program has not
printed a line since the last event it was sent, newer drag events replace each other,
and the program gets only the newest one next. A slow program makes a drag coarser. It
does not fall behind.

**A program that prints nothing is sent drag events about 25 times a second.** Tauler
waits a short time for a line, then sends the newest event anyway, so a program that only
acts still works.

**The program is named by its `bin` alone.** `useDispatch` takes no script. To dispatch
to a script, save it as an executable file. `useDispatch("/bin/sh")` would start a shell
that reads the events as its input.

**Call `useDispatch` on every run.** A run that does not call it stops the program, as
for a [stream](/docs/data/streams/#rules).

**An event for a program no run has named is dropped.** Tauler logs it and still sends
the other events of the same handler.

**The program's first line of input is not yours.** Tauler writes a startup event,
`{"type":"init",…}`, before any other. A program must skip the types it does not know.
See [Write a program a layout can control](/docs/data/programs/).

## Reference

### `useDispatch(bin, props?)`

| argument | type | description |
|---|---|---|
| `bin` | string | The program to run: a path, a name on `PATH`, or a path starting `~/`. It is run with no arguments. |
| `props` | object, optional | Settings for the program. Tauler includes them in the startup event and sends that event again when they change. See [Pass settings to the program](/docs/data/services/#pass-settings-to-the-program). |

Returns an object on which every property is a function. `dispatch.name(data?)` returns
an event addressed to `bin`: its `type` is the property name, and the keys of `data` sit
beside it. The return value goes in a handler's list. It is plain data, and calling the
function sends nothing.

`useEvents` is the earlier name of `useDispatch`. It still works and is the same function.

### Behaviour

- **Before the first value.** There is no value. `useDispatch` returns its object on the
  first run, and events can be sent as soon as the program has started.
- **When it updates.** It does not. A dispatch never causes the layout to run again.
- **On errors.** When the program exits, Tauler starts it again within about half a
  second and sends it a new startup event. An event written while it was down is lost.
- **On a reload.** Saving the layout file stops the program and starts it again.

## Related

- [Do something on a click](/docs/interaction/clicks/): the handlers that take events.
- [Offload state to an external program](/docs/data/services/): dispatch to a program and
  read it too.
- [Write a program a layout can control](/docs/data/programs/): the same exchange from
  the program's side.
