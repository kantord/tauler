# Packages plug into the existing resolver and reload path, not a new one

A layout file may import `@gh/<owner>/<repo>` directly — a **Package**: a git
repository tauler fetches, pins to a commit in a **Lockfile** sibling to the layout
file, and caches on disk. No new evaluation model, no new failure-handling path, no
new blocking point is introduced to get there. Everywhere this touches an existing
mechanism — the module resolver, the startup/reload split, the package cache's own
write path — the existing mechanism is reused as-is, not special-cased for Packages.
Where that was not possible (there is no existing git-fetch, no existing lockfile,
no existing CLI subcommand), what's added is kept as small as the requirement
actually demands. See [CONTEXT.md](../../CONTEXT.md) for **Package** and
**Development mode**. Supersedes
[docs/ideas/takumi-modules-registry.md](../ideas/takumi-modules-registry.md), which
proposed a single shared registry repository rather than arbitrary third-party
repositories and left lockfile-based pinning as an open question — this decision
answers it.

## Specifier and entry point

`@gh/<owner>/<repo>` only — no subpath. It resolves to an extensionless `index` at
the checkout root, through the same extension-fallback (`.js`/`.jsx`/`.ts`/`.tsx`)
`optative_script::loader::ConfinedFsResolver` already applies to a relative import,
so a Package can be authored in `.tsx` exactly as easily as `.jsx`, with no separate
convention to maintain. A subpath (`@gh/owner/repo/some/file`) was considered and cut:
nothing in the issue's own example needs it, and every part of this design — the
cache path, the Lockfile key, Development mode — keys on `owner/repo` alone.

`@gh/` sits in the same visual family as `@ui/*`, tauler's existing namespace for
its own first-party, Rust-registered built-in components
(`tauler-ui-macro`'s `#[component("@ui/card")]`). That is a real, deliberate
tension: `@ui/*` is vetted code you ship, `@gh/*` is arbitrary third-party code that
gets a full trust grant (see Security posture, below) — and the two now look alike
at a glance. Kept anyway, because the issue's own example already spells out
`@gh/user/repo`, and the two prefixes remain distinguishable on an actual read, not
just a glance.

## Discovery lives in the resolver, not a separate scan step

A `GitPackageResolver`/`GitPackageLoader` pair, following the wrap-and-delegate
shape `SchemaAwareLoader` already establishes (`src/jsx.rs:341-397`): recognize
`@gh/owner/repo`, delegate every other specifier to the existing
`ConfinedFsResolver`/`ConfinedFsLoader`. It holds an `Arc<FetchManager>` — a plain
private struct, not a subsystem: an in-process dedup table plus a `reload_tx` clone
— threaded into `with_effects` (`src/jsx.rs:444-464`) the same way `base_dir`
already is.

On each `@gh/owner/repo` specifier QuickJS asks it to resolve: check the Lockfile
and the package cache. Warm → resolve to the entry file, synchronously, no
different in cost from resolving a relative import today. Cold → non-blockingly
enqueue a fetch on the `FetchManager` (deduped by `(owner, repo, ref)`; a second
request for a key already in flight is a no-op) and return a resolution error, the
same `rquickjs::Error::new_resolving`/`new_loading` shape `NoFsResolver`/
`NoFsLoader` already use for "this can't be resolved."

`JsxEvaluator::new` then fails exactly the way it already fails for a JS syntax
error, a missing relative import, or a bad `.schema.yaml` — there is no new gate in
front of it, and no separate pre-check step. `initial_load` and
`handle_layout_reload` (`src/app.rs:861-888`, `912-993`) already treat every
`JsxEvaluator::new` failure identically: log, and don't touch whatever state is
already live (`None` at startup, the previous evaluator on reload). A cold Package
is just one more entry in that same bucket. When the enqueued fetch completes, it
sends on its `reload_tx` clone — the exact channel a file watcher already uses —
and the normal reload path retries on its own next tick.

This replaces an earlier draft of this design that scanned the layout's raw source
text for `@gh/...` specifiers with a regex, run before `JsxEvaluator::new` was ever
called. That version worked, but for the wrong reason: it was trying to avoid ever
letting evaluator construction be attempted on a doomed cycle, which is not
something this codebase needs avoided — every other resolution failure already
goes through construct-and-fail-gracefully. Once that motivation is gone, the
regex is strictly worse than asking the real parser: it can miss the bindingless
`import "@gh/...";` form, it can false-positive on a specifier that only appears in
a comment or a string, and — because it only ever looked at the top-level layout
file's own text — it cannot see a Package that imports another Package
transitively at all. Resolver-based discovery gets all three for free: real ES
syntax has no bindingless-form gap and no comment/string false positive, and a
transitively-imported Package is discovered the moment its own module is linked,
which is the same resolver being asked the same question again.

The one honest cost of that: a multi-level dependency chain discovered for the
first time now resolves one layer per reload cycle rather than all at once — a
cold `A` fails this cycle; the next reload discovers `A`'s import of `B` is also
cold; and so on. A few extra seconds of convergence on a brand-new deep tree, not
a correctness problem, and strictly better than the alternative, which did not
support transitive Packages at all.

Dynamic `import()` is untouched by any of this. It was never matched by the old
scan and is not specially recognized by the resolver either — it is the ordinary
JS mechanism already available to a layout author who wants one risky Package to
degrade in isolation rather than blank the whole layout on a cold miss (see
Consequences).

## Lockfile

A YAML file sibling to whichever `LayoutSource` path was actually resolved (not a
hardcoded `~/.config/tauler/` constant — the Lockfile must sit next to the real
layout file, wherever that is, per the issue's own requirement that it be
checkable into a dotfiles repo alongside it). Maps `owner/repo` to `{commit,
development}`. Importing a Package tauler has never seen creates its entry, pinned
to whatever commit was current at first fetch — there is no separate "install"
step distinct from the first successful import.

## Package cache

`~/.cache/tauler/pkg/gh/<owner>/<repo>/<ref>`. `<ref>` is the pinned commit sha, or
`development-<DefaultHasher(canonicalize(lockfile_path))>` for a Package in
Development mode — canonicalized so the same physical Lockfile reached via a
symlink or a different relative spelling doesn't produce a spurious second
checkout, and hashed with `std::hash::Hash`/`DefaultHasher` specifically (already
in `std`, no new dependency — there is no cryptographic need here, only
collision-avoidance for a directory name).

A write always goes: clone/fetch into a per-attempt, randomly-named temp directory
under the same cache root, `git checkout <ref>`, then `fs::rename` into the final
path. "Warm" is defined purely by that final path existing, which is only ever
reached via a completed rename — never true for a partial or orphaned clone,
regardless of anything else. A rename that fails because the target already exists
is a benign race (discard the redundant temp directory, the loser did no useful
work); any other rename failure is a real, loudly-logged error, not silently
treated as success.

Content-addressing by `<ref>` is load-bearing, not decoration: two different
Lockfiles on the same machine can legitimately pin the same `owner/repo` to two
different commits, or mark it Development mode independently, and a single mutable
checkout shared between them would race. The per-attempt-random temp directory
name matters for the same reason in the other direction — two concurrent fetch
attempts for the same key, however they arose, never share a working tree.

## Cache hygiene: no automated bookkeeping beyond a narrow orphan reaper

A cold or failed Package is simply retried on the next externally-triggered
reload — a layout/theme edit, a `bar_width`/`outer_gap` re-exec, a
binary-replacement re-exec, or an explicit `tauler pkg update` — logged loudly on
every attempt, success or failure. No backoff table, no persisted failure marker.
This was tried during design and cut: the retry cost is bounded by how often those
external events happen, not by anything Packages themselves do, so the worst case
is wasted network calls, not a hang or a crash — the issue's own bar (requirement
2), not a stronger one this design volunteers to clear.

One piece of bookkeeping is kept, and it is not the same problem as the one just
described. `main.rs` re-execs tauler's own binary (`cmd.exec()` — a full process
image replacement) in response to ordinary events, not just crashes, and `exec()`
does not kill a spawned `git clone` child: it is reparented and keeps writing into
its temp directory, invisible to the new process image, whose own in-process dedup
table starts empty. Left alone, every re-exec that races an in-flight clone leaves
a permanent orphan — disk that is never reclaimed, not merely a wasted retry. So
`tauler pkg update` also sweeps `.tmp-*` directories in the package cache older
than a generous, standalone age floor (on the order of an hour — long enough that
no real clone is ever mistaken for abandoned; not shared with any other constant,
since nothing else needs one anymore). This is the only automated cache
bookkeeping in v1. Everything else — a stale sha left behind after a `pkg update`
moves a pin forward, a hung-rather-than-merely-slow clone that outlives even the
reaper's floor — is not reclaimed automatically. Deleting
`~/.cache/tauler/pkg` in its entirety is always safe and everything in it
self-heals by re-fetching; that is the documented recovery path in place of a
fuller garbage collector, which this issue does not ask for.

## Development mode

A Package fetched once and left alone: never re-pinned, never touched by `tauler
pkg update`, its checkout the maintainer's to edit directly. Marked
`development: true` in the Lockfile — not by a comment in the layout file, since
the Lockfile is already the single place pinning state lives, and a second,
source-level place to declare the same fact would be two sources of truth for one
piece of state. A concurrent clone race for the same Development-mode Package can
genuinely diverge (there is no commit to arbitrate which write "wins," unlike the
pinned case) — accepted, since this mode explicitly promises no pinning at all.

## Security posture

Importing a Package grants its top-level code the same capabilities a layout
file's own code already has, shell exec included, the moment the reconciler
runtime (`Effects::Allowed`, ADR 0034) evaluates it — no sandboxing is attempted,
and building one is out of scope for this decision; it would mean redesigning what
the reconciler runtime is, not extending the module resolver. Treat every
`@gh/owner/repo` import the same as pasting that code into your own layout file, or
`npm install`ing a package with a postinstall script: a trust decision made the
moment it's typed, not something this feature can make safe after the fact.

What is in scope, and is kept: nothing re-pins a Package without an explicit,
user-invoked `tauler pkg update` — never automatically, never on a timer. Every
first-time pin and every pin change `pkg update` makes is logged loudly, repo and
old/new sha both, never silently.

## `tauler pkg update`

The first CLI subcommand this codebase has had — `main.rs` has never parsed
`std::env::args()` before this. Dispatches on `args().nth(1) == "pkg"`, before any
window or `App` setup. Re-resolves every non-Development-mode Lockfile entry to its
remote's current HEAD, fetches it through the same atomic-rename path, rewrites the
Lockfile, logs every change, and runs the orphan-temp-directory reaper described
above.

## Consequences

**One cold import blanks the whole layout evaluation, not just the piece that
needed it.** ES module linking is all-or-nothing per module — this is inherent to
the mechanism, not a gap this design leaves open. An author who wants one risky
Package to degrade in isolation reaches for a dynamic `import()` inside that
component, which is ordinary JS already available today, not new tauler-specific
API.

**A toolchain bump can silently force a re-clone of every Development-mode
Package.** `DefaultHasher` is deterministic within one build, not guaranteed
stable across Rust compiler versions. Accepted for the same reason the
symlink/canonicalization case is: a wasted re-clone, never corruption or data
loss.

**Disk is not automatically reclaimed beyond the narrow orphan reaper above.** A
stale sha left behind by a `pkg update` that moved a pin forward, or a clone that
somehow outlives even the reaper's generous floor, sits until someone deletes the
whole cache directory by hand. A fuller garbage collector — one that could
distinguish "no Lockfile anywhere still needs this sha" from "some other Lockfile
on this machine still does" — is real, cross-referencing complexity that nothing
in the issue asks for, and is left for a future decision if disk growth actually
proves to be a problem in practice rather than a theoretical one.

**No new domain concept was needed for the reconciler-runtime trust boundary.**
`Effects::Denied`/`Allowed` already exists and already governs this; Security
posture, above, states the consequence in prose rather than naming a new
CONTEXT.md term for it, the same way ADR 0040 decided `ConfigFile` needed no new
term — it names no new relationship, only what an existing one now implies.
