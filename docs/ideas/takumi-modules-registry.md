# Takumi Modules — Git-backed Module Registry

## Core idea

A `takumi_modules/` directory in the tauler git repository serves as a shared,
community-accessible module registry for layout components. The import resolver
recognises imports from this namespace and fetches the file via a public git API
(e.g. GitHub raw content API), caching it locally so the network request only
happens once.

No package manager, no lockfile, no `node_modules`. Shared components are just
files in a git repo with a stable, addressable URL.

## How it would work

1. A layout file imports a shared component:
   ```js
   import { WeatherCard } from 'takumi_modules/weather/WeatherCard.jsx'
   ```

2. The resolver recognises the `takumi_modules/` prefix as a remote namespace.

3. It checks the local cache (`~/.cache/tauler/modules/`). On a cache hit, it
   loads from disk. On a miss, it fetches the file via the GitHub raw content API
   (or equivalent), writes it to the cache, then loads it.

4. Subsequent runs always use the cached copy — no network dependency at runtime.

## Why git as the registry

- No central package registry to maintain or depend on
- Files are content-addressed and auditable via git history
- ToS-compliant public access via raw file APIs
- Pull requests are the contribution model — no publish step

## Open questions

- **Cache invalidation**: manual (delete cache), version-pinned imports, or
  time-based staleness? Version-pinned (commit SHA in the import path) is the
  most reproducible option.
- **Namespacing**: single repo or allow third-party repos via a URL-style import
  specifier (similar to Deno's remote imports)?
- **Integrity checking**: optionally verify a hash of the fetched file against a
  lockfile to prevent supply chain issues.

## Current state

Not implemented. The local import resolution feature (oxc_resolver + rquickjs
custom loader) is the prerequisite — the registry is a later extension of that
same resolver, adding a remote fetch path alongside the local file path.
