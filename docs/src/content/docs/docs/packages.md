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
