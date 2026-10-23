---
title: Packages
description: Importing a shared component straight from a git repository — auto-fetched, pinned by a lockfile, updated on request.
---

A layout file can import a component directly from a public GitHub repository, no
install step:

```jsx
import { WeatherCard } from "@gh/someone/tauler-weather";

<WeatherCard city="Budapest" />
```

The first time a layout imports `@gh/owner/repo`, tauler clones it, pins it to whatever
commit is current, and writes that pin to a lockfile next to the layout file —
`tauler-pkg.lock`. Every later run reads the same pin, so the bar never silently changes
because upstream pushed a new commit.

The lockfile is plain YAML. Commit it to your dotfiles repo alongside `layout.op.mdx`:

```yaml
someone/tauler-weather:
  commit: 4f2a9e1c8b7d3a0e5f6c1b2d9a8e7f6c5b4a3d2e
```

## Updating a pin

```
tauler pkg update
```

Re-resolves every Package to its repository's current commit, fetches whatever changed,
and rewrites the lockfile. Nothing updates a pin on its own — a Package only ever moves
when you run this.

## Working on a Package yourself

If you're the one maintaining `someone/tauler-weather`, pinning to a commit gets in the
way — you want tauler to use your working checkout as you edit it, not a locked
snapshot. Mark the entry `development: true` by hand:

```yaml
someone/tauler-weather:
  commit: 4f2a9e1c8b7d3a0e5f6c1b2d9a8e7f6c5b4a3d2e
  development: true
```

tauler clones it once and leaves it alone from then on — no pin enforced, never
touched by `tauler pkg update`.

## What importing a Package means

A `@gh/owner/repo` import runs with the same trust as your own layout file — the
same access to `sh` and everything else a layout file can reach. Importing one is
the same kind of decision as pasting its code into your own layout, or installing
any other package that runs code you haven't read. Only pull in a Package whose
source you trust.

## Shipping a program with a Package

A Package can carry the program its component runs. Put it in the repository next to
the component and name it through `import.meta.dirname`, the directory of the file
that reads it:

```
tauler-volume/
├── index.jsx
├── data/Volume.jsx
└── bin/tauler-volume
```

```jsx
// data/Volume.jsx
export const DEFAULT_BIN = `${import.meta.dirname}/../bin/tauler-volume`;

export function Volume({ bin = DEFAULT_BIN }) {
  const volume = useJSONStream(bin);
  return <span>{volume?.percent ?? "…"}%</span>;
}
```

`import.meta.dirname` points into the checkout of the pinned commit, so the program
always matches the component that runs it. Nobody has to copy anything out of tauler's
cache. After `tauler pkg update` the path changes, and the program restarts on the new
commit.

Commit the program with its executable bit set (`chmod +x bin/tauler-volume`, then
`git add`). tauler fetches Packages with `git`, which keeps that bit.

`import.meta.dirname` is `undefined` where there is no file on disk: in a browser
preview, and in a screenshot rendered from a source string. Give it a fallback if your
component must render there too:

```jsx
export const DEFAULT_BIN = import.meta.dirname
  ? `${import.meta.dirname}/../bin/tauler-volume`
  : "~/.local/bin/tauler-volume";
```
