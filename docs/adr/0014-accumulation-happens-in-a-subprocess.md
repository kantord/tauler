# Accumulation happens in a subprocess

tauler keeps one value per stream — the latest line — so a layout file has no way to draw
the last minute of anything. History comes from piping the stream through an **Accumulator**
inside the script the user is already writing:

```jsx
useJSONStream("/bin/sh", `
  while :; do cut -d' ' -f1 /proc/loadavg; sleep 1; done | tauler-accumulate -n 60
`)
```

Each line out is a JSON array of the last 60 samples. This needs nothing from tauler:
`spawn_module` writes the script to a file and runs `bin` against it, so the script can
already contain a pipe.

## Why not accumulate in `globals`

`globals` is the right shelf architecturally — the layout file is a stateless reducer and
`globals` is its only store — but the mechanics do not work.

An incoming line is stored only when it *differs* from the stored one, and a tick fires when
any stream changes (`src/app.rs`, and ADR 0007 for why a tick is whole-tree). So a `globals`
accumulator would sample at tick rate
rather than at its source's rate, letting an unrelated clock decide a CPU graph's
resolution; and it would drop every repeated reading, flattening a genuine plateau into a
single point and lying about the time axis. A subprocess accumulating on its own clock has
neither problem.

## Status

Built-in retention — tauler keeping the last N samples per stream identity — is a likely
direction, not a commitment. It would need per-stream tick delivery with duplicates
preserved, plus a retention policy and a memory bound. Decide the depth and cadence from
real widgets rather than guessing them now.
