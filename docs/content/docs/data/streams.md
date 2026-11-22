---
title: Read a value from an external program
description: Run a program and show the latest line it prints.
template: article
---

Name a program, and the layout gets the latest line it printed, again on every new line.
A program read this way is a **stream**.

```jsx
export default function render() {
  const time = useStringStream("/bin/sh", `
    while true; do date +"%H:%M:%S"; sleep 1; done
  `);
  return (
    <root>
      <panel id="clock" anchor="top" width={ctx.screen_width} height={32}>
        <div class="px-3 text-foreground">{time}</div>
      </panel>
    </root>
  );
}
```

## How it works

Tauler runs the program and reads what it prints, one line at a time. It keeps one value
for each stream, the latest line, and runs the layout again when that value changes. The
layout never starts or stops anything: each run names the programs it reads, and Tauler
starts the new ones, keeps the ones named again, and stops the rest. A program is
recognised by its `bin` and its `script` together, so two components that name the same
pair read one running program.

## Common tasks

### Read JSON

`useJSONStream` parses each line, so the program can print an object:

```jsx
const disk = useJSONStream("/bin/sh", `
  while true; do
    df -P / | awk 'NR == 2 { printf "{\\"used\\": \\"%s\\"}\\n", $5 }'
    sleep 30
  done
`);

<div class="text-foreground">{disk?.used ?? "…"}</div>
```

### Run a program of your own

Leave the script out to run an executable as it is. `~/` is your home directory:

```jsx
const weather = useJSONStream("~/.config/tauler/bin/weather");
```

### Keep the last values

Pipe the output through `tauler-accumulate` to get the last lines as an array, oldest
first:

```jsx
const load = useJSONStream("/bin/sh", `
  while :; do cut -d' ' -f1 /proc/loadavg; sleep 1; done | tauler-accumulate -n 60
`) ?? [];

const now  = load.length ? load[load.length - 1] : 0;
const peak = load.length ? Math.max(...load) : 0;
const mean = load.length ? load.reduce((a, b) => a + b, 0) / load.length : 0;
```

### Trim lines before they are kept

`tauler-accumulate` has no filter option. Put `jq` in the pipe in front of it:

```jsx
const recent = useJSONStream("/bin/sh", `
  journalctl -f -o json | jq -c --unbuffered .MESSAGE | tauler-accumulate -n 5
`) ?? [];
```

### Show the last lines in a table

The window is an ordinary array, so a component takes it as it is:

```jsx
import { DataTable } from "@ui/datatable";

function RecentLogs() {
  const lines = useJSONStream("/bin/sh", `
    journalctl -f -o json --output-fields=MESSAGE,_COMM | tauler-accumulate -n 5
  `) ?? [];

  return (
    <DataTable
      columns={[{ key: "_COMM", label: "UNIT" }, { key: "MESSAGE", label: "MESSAGE" }]}
      rows={[...lines].reverse()}
    />
  );
}
```

### Use one value in two places

Read the stream at the top of the render function and use the variable below. Reading
the same `bin` and `script` in two components also works, and still runs one program:

```jsx
const CLOCK = `while true; do date +"%H:%M"; sleep 1; done`;

function Clock() {
  return <div>{useStringStream("/bin/sh", CLOCK)}</div>;
}

function Greeting() {
  const hour = Number(useStringStream("/bin/sh", CLOCK).slice(0, 2));
  return <div>{hour < 12 ? "Good morning" : "Good afternoon"}</div>;
}
```

## Rules

**Call a `use` function on every run, never inside a branch that sometimes does not
run.** A program that a run does not name is stopped, and it starts from nothing when a
later run names it again. For a program that holds state, such as
[`tauler-notify`](/docs/data/tauler-notify/), that loses the state.

**Keep `bin` and `script` the same from run to run.** They identify the program. A
script built from a changing value is a new program on every change: the old one is
stopped and the new one starts with no value.

**Call the `use` functions inside the render function or a component.** A call at the
top level of a file runs once, when the file loads, so later runs do not name the
program and Tauler stops it.

**Guard against the first run.** No line has arrived yet, so `useStringStream` returns
`""` and `useJSONStream` returns `null`. `?? []` and `?.` cover it.

**Print one value per line, and flush it.** Tauler reads lines. A program that buffers
its output shows nothing until the buffer fills.

**Do not expect a `use` function to remember.** They are not React's hooks: there is no
state slot, no effect and no dependency array behind them. To keep a value between runs,
use a [store](/docs/data/shared-stores/).

## Reference

### `useStringStream(bin, script?)`

| argument | type | description |
|---|---|---|
| `bin` | string | The program to run: a path, a name on `PATH`, or a path starting `~/`. |
| `script` | string, optional | Text Tauler saves to a temporary file and passes to `bin` as its one argument. With `/bin/sh` it is a shell script; with another interpreter it is a script in that language. |

Returns the latest line the program printed, as a string without the line ending.

### `useJSONStream(bin, script?)`

The same arguments. Returns the latest line parsed as JSON. A `bin` that starts with
`tauler:` reads a [stream Tauler provides](/docs/data/built-in-streams/) and takes no
script.

### `tauler-accumulate -n <count>`

A program that ships with Tauler (`cargo install tauler-accumulate`). It reads lines and,
for each one, prints a JSON array of the last `count` lines, oldest first.

- The array grows from the first line. It does not wait until it holds `count` values.
- Each line is kept as JSON if it parses, and as a string if it does not. A bare number
  arrives as a number.
- Numbers are written again from their value, so `0.60` comes back as `0.6`. Accumulate
  strings if the exact text matters.
- There is no query or filter option. Filter before it with `jq`, and reshape the array in
  the layout.

```
$ printf '0.41\n0.52\noops\n' | tauler-accumulate -n 2
[0.41]
[0.41,0.52]
[0.52,"oops"]
```

### Behaviour

- **Before the first value.** `useStringStream` returns `""`. `useJSONStream` returns
  `null`.
- **When it updates.** The layout runs again when the latest line differs from the one
  before. A line equal to the last one changes nothing. When several lines arrive
  together, the layout sees the last.
- **On errors.** A line that is not JSON makes `useJSONStream` return `null` until the
  next line that is. When the program exits, Tauler starts it again within about half a
  second, and the value stays as it was until the new program prints. A restarted pipe
  through `tauler-accumulate` begins with an empty window.
- **On a reload.** Saving the layout file stops every program, clears every value and
  starts again from the first run.

## Related

- [Handling dynamic data](/docs/data/): the other places a value can live.
- [Offload state to an external program](/docs/data/services/): when the layout must also
  change the value it reads.
- [Streams Tauler provides](/docs/data/built-in-streams/): values that need no program.
- [`<D3Figure>`](/docs/components/#d3figure): drawing a chart from a window of values.
- [Screenshots](/docs/develop/screenshots/): streams do not run in a screenshot.
