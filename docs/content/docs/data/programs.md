---
title: Write a program a layout can control
description: What Tauler writes to your program and what it reads back, one line of JSON at a time.
template: article
---

A program that prints its state as lines of JSON and reads events as lines of JSON can
hold state for a layout, in any language. From the layout's side it is a **service**.

The program, saved as `~/.config/tauler/bin/counter` and made executable:

```python
#!/usr/bin/env python3
import json, sys

count = 0
for line in sys.stdin:
    event = json.loads(line)
    if event["type"] == "add":
        count += event["by"]
    print(json.dumps({"count": count}), flush=True)
```

The layout that uses it:

```jsx
export default function render() {
  const [counter, dispatch] = useService("~/.config/tauler/bin/counter");
  return (
    <root>
      <panel id="counter" anchor="top" width={120} height={32}>
        <div on_click={[dispatch.add({ by: 1 })]}>{counter?.count ?? 0}</div>
      </panel>
    </root>
  );
}
```

## How it works

Tauler starts the program with no arguments and connects to its standard input and
output. It writes a startup event as the first line of input, then one line for each
event the layout dispatches. Each line the program prints is the layout's new state.
There are no request ids and no replies: the program reads, changes what it holds, and
prints the whole state again. It is the same exchange as a worker and `postMessage`,
with lines of JSON in place of objects.

## Common tasks

### Try the program without Tauler

Type the lines Tauler would write:

```
$ ~/.config/tauler/bin/counter
{"type":"init","output":"DP-2","dpi":96.0,"screen_width":1920,"screen_height":1080}
{"count": 0}
{"type":"add","by":2}
{"count": 2}
```

### Print the state when it changes by itself

Print a line whenever the state changes, whether or not an event caused it. This one
counts seconds and takes a `reset` event:

```python
#!/usr/bin/env python3
import json, sys, threading, time

start = time.time()

def tick():
    while True:
        print(json.dumps({"seconds": int(time.time() - start)}), flush=True)
        time.sleep(1)

threading.Thread(target=tick, daemon=True).start()
for line in sys.stdin:
    if json.loads(line)["type"] == "reset":
        start = time.time()
```

### Read settings from the layout

Settings given as `useService(bin, { step: 5 })` arrive as keys of the startup event:

```python
for line in sys.stdin:
    event = json.loads(line)
    if event["type"] == "init":
        step = event.get("step", 1)
```

### Keep state across a restart of Tauler

Save the state to a file when it changes, and load it at the start:

```python
import json, os

PATH = os.path.expanduser("~/.local/state/counter.json")
count = json.load(open(PATH))["count"] if os.path.exists(PATH) else 0
```

## Rules

**Print one JSON value per line, and flush after each.** Tauler reads lines. Buffered
output reaches the layout late or never.

**Print the whole state every time.** The layout keeps only the latest line, so a line
that holds a part of the state replaces the rest.

**Skip event types you do not know.** The startup event arrives first, with type `init`,
and can arrive again. A program that stops on an unknown type stops at once.

**Print a line after you handle an event, when you can.** During a drag, Tauler sends
the next position after the program prints. A program that prints nothing gets drag
events about 25 times a second. See [the rules for dispatch](/docs/data/dispatch/#rules).

**Exit when standard input closes.** That is how the program learns that Tauler is
gone. A program that keeps running holds whatever it had claimed.

**Write logs to standard error.** Every line on standard output is taken as state.

**Expect to be restarted.** Tauler starts the program again when it exits, when the
layout file is saved, and when a run of the layout stops naming it and a later run names
it again. Save what must outlast that.

## Reference

### The startup event

The first line Tauler writes. It is one JSON object:

| key | type | description |
|---|---|---|
| `type` | string | Always `"init"`. |
| `output` | string | The name of the primary screen, such as `"DP-2"`. An empty string on Wayland. |
| `dpi` | number | The DPI Tauler draws at. `96` on Wayland; the scale factor times 96 on macOS. |
| `screen_width`, `screen_height` | number | The size of the primary screen, in logical pixels. |
| any other key | JSON | The settings the layout gave as `props`. |

A setting with the name of one of the keys above is replaced by Tauler's value.

Tauler writes the event again, with every key, when its content changes: when the layout
gives different settings, or when the screen facts change. It does not write it again
when nothing changed.

### An event

One line, one JSON object, written as the layout built it: `dispatch.add({ by: 1 })`
arrives as `{"type":"add","by":1}`. Nothing is wrapped around it, and it does not say
which click or element caused it.

### A line of state

Any JSON value on one line. The layout reads it as the first half of
`const [state, dispatch] = useService(bin)`.

### Behaviour

- **Before the first value.** The layout's state is `null` until the program prints.
  Print the state once at the start, or after the startup event, so the layout has it.
- **When it updates.** Each line that differs from the last one runs the layout again.
- **On errors.** When the program exits, with any status, Tauler starts it again within
  about half a second and writes a new startup event. A line that is not JSON gives the
  layout `null`.
- **On a reload.** Saving the layout file stops the program and starts it again.

## Related

- [Offload state to an external program](/docs/data/services/): the layout's side.
- [Send events to an external program](/docs/data/dispatch/): how often events arrive.
- [Packages](/docs/packages/#shipping-a-program-with-a-package): ship a program with
  components that use it.
- [Show and switch i3 workspaces](/docs/data/tauler-i3/): a shipped program that reads
  the startup event.
