---
title: Offload state to an external program
description: Show a value that another program holds, and change it from the layout.
template: article
---

Name a program, and the layout gets the state it prints and a way to send it events. A
program that holds state for the layout like this is a **service**.

The layout:

```jsx
import { Slider } from "@ui/slider";

export default function render() {
  const [audio, dispatch] = useService("~/.config/tauler/bin/volume");
  return (
    <root>
      <panel id="volume" anchor="top" width={240} height={32}>
        <Slider value={audio?.volume ?? 0} step={5}
                on_change={(v) => dispatch.set({ value: v })} />
      </panel>
    </root>
  );
}
```

The program, saved as `~/.config/tauler/bin/volume` and made executable. This one uses
PipeWire's `wpctl`; any program that prints a line of JSON and reads lines of JSON does
the same job:

```sh
#!/bin/sh
report() {
  wpctl get-volume @DEFAULT_AUDIO_SINK@ | awk '{ printf "{\"volume\": %.0f}\n", $2 * 100 }'
}
report
while read -r event; do
  value=$(printf '%s' "$event" | jq -r 'select(.type == "set") | .value')
  [ -n "$value" ] && wpctl set-volume @DEFAULT_AUDIO_SINK@ "$value%"
  report
done
```

## How it works

A service is a [stream](/docs/data/streams/) and a [dispatch](/docs/data/dispatch/) on
the same program. `useService` returns the pair `[state, dispatch]`, the shape of React's
`const [state, dispatch] = useReducer(...)`, with the reducer in another program. The
state is the latest line the program printed, parsed as JSON. A dispatch gets no reply:
the program changes its state, prints it, and the layout runs again with the new value.
The layout holds no copy, so the slider above shows the volume the program reported, and
it moves only when the program says the volume changed. The state can also change
without you, when something else changes the volume and the program prints it.

## Common tasks

### Read the state in one component and dispatch in another

Call `useService` with the same `bin` in each. Tauler runs one program:

```jsx
function VolumeLabel() {
  const [audio] = useService("~/.config/tauler/bin/volume");
  return <div>{audio?.volume ?? 0}%</div>;
}

function MuteButton() {
  const [, dispatch] = useService("~/.config/tauler/bin/volume");
  return <div on_click={[dispatch.set({ value: 0 })]}>mute</div>;
}
```

### Pass settings to the program

Give `useService` a second argument. Tauler sends it to the program when it starts, and
again when a value in it changes:

```jsx
const [i3, dispatch] = useService("~/.cargo/bin/tauler-i3", {
  gaps: { left: 300, top: 8 },
});
```

### Only read a service

Take the first half of the pair, or call `useJSONStream` with the same `bin`:

```jsx
const [notifications] = useService("~/.cargo/bin/tauler-notify");
```

### Combine two services

Both values are plain variables in the same function:

```jsx
const [i3] = useService("~/.cargo/bin/tauler-i3");
const [notify] = useService("~/.cargo/bin/tauler-notify");

const busy = (notify?.notifications ?? []).length > 0;
const focused = i3?.workspaces?.find((ws) => ws.focused);
```

## Rules

**The layout never changes the state itself.** It dispatches, and shows what the program
prints next. Nothing is shown early, so there is one answer to "what is the volume".

**One `bin` is one program.** However many components call `useService`, `useDispatch`
or `useJSONStream` with the same `bin` and no script, they share one running program.

**Settings from several calls are joined.** Each call adds its keys. When two calls give
the same key, the first call in the run keeps it.

**Tauler's own startup fields are kept over your settings.** A setting named `type`,
`output`, `dpi`, `screen_width` or `screen_height` is replaced. See
[the startup event](/docs/data/programs/#the-startup-event).

**Call `useService` on every run.** A run that does not call it stops the program, and
the program's state is lost unless it saved it. Starting to dispatch to a program that
was only read restarts it once, for the same reason.

**The dispatch rules apply.** A click is always sent, and a drag sends the newest
position. See [the rules for dispatch](/docs/data/dispatch/#rules).

## Reference

### `useService(bin, props?)`

| argument | type | description |
|---|---|---|
| `bin` | string | The program to run: a path, a name on `PATH`, or a path starting `~/`. It is run with no arguments. |
| `props` | object, optional | Settings for the program, as JSON. Sent in the startup event, and again when they change. Settings equal to the last ones sent are not sent again. |

Returns `[state, dispatch]`:

- `state` is what `useJSONStream(bin)` returns: the latest line, parsed as JSON.
- `dispatch` is what `useDispatch(bin, props)` returns: `dispatch.name(data?)` builds an
  event of type `name`.

`<Module bin="…">{(state, dispatch) => …}</Module>` is the earlier name of `useService`,
written as a component whose child is a function. It still works: every prop other than
`bin` is passed as `props`.

### Behaviour

- **Before the first value.** `state` is `null` until the program prints its first line.
- **When it updates.** The layout runs again when the program prints a line that differs
  from the last one.
- **On errors.** A line that is not JSON makes `state` `null` until the next line that
  is. When the program exits, Tauler starts it again within about half a second and
  sends a new startup event. `state` stays as it was until the new program prints.
- **On a reload.** Saving the layout file stops the program, clears `state` and starts
  the program again.

## Related

- [Write a program a layout can control](/docs/data/programs/): what a service reads and
  prints.
- [Show and switch i3 workspaces](/docs/data/tauler-i3/) and
  [Show desktop notifications](/docs/data/tauler-notify/): two services that ship with
  Tauler.
- [Control components](/docs/components/#control-components): `<Slider>` and `<Knob>`
  show a service's value and dispatch to it.
- [Share state across the layout](/docs/data/shared-stores/): for state no program needs
  to hold.
