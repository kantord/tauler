# Reconciler as Memory Model

## Core idea

The `ManagedSet` / `Lifecycle` pattern is not just a process manager — it is a
general-purpose answer to one of Rust's most common ergonomic pain points:
*shared ownership without a clear owner*. By expressing resource lifetimes as
desired-state sets rather than as reference counts or borrow scopes, a large
class of apps can avoid `Arc`, cycle risk, and lifetime annotation fights
entirely.

## Why this is different from a GC

A garbage collector asks: "is this still reachable?" and frees when the answer
is no. A reconciler asks: "is this still in the desired set?" and frees when
the answer is no. The reconciler's contract is narrower, but within that
contract it is strictly better:

- **Deterministic** — freed immediately when removed from the desired set, not
  "eventually"
- **No cycles** — there is no reachability graph to traverse
- **No overhead** — no `Arc` ref-count bumps, no stop-the-world pauses
- **Legible lifetimes** — the reconcile call site in the source code *is* the
  lifetime; no need to reason about who holds the last reference
- **No borrow fights** — one thing (`ManagedSet`) is the unambiguous owner;
  everything else borrows transiently

## The constraint

Resources must fit the enter / update / exit shape and be expressible as a
desired-state iterator. This rules out deeply nested, arbitrarily-shared object
graphs. But it fits a surprisingly large class of real problems:

- Process / thread pools
- Connection pools
- UI panels and windows (current tauler use-case)
- Background task runners
- Cache entry sets
- Subscription / event-listener sets
- File watcher registrations
- Any "pool of things driven by external desired state"

## The React analogy

React's reconciler is this pattern applied to UI nodes. React feels dramatically
less painful than manual DOM management not because it has a GC, but because it
owns the lifecycle and drives it from a desired virtual tree. Components have
`mount` / `update` / `unmount` — exactly `enter` / `update` / `exit`. The
reconciler diffs desired against current and applies minimal changes.

`ManagedSet` is that idea pulled into general Rust, with no framework
dependency and no runtime overhead beyond a `HashMap` lookup.

## Implications for app architecture

An app structured around this pattern has a natural shape:

1. **One or more `ManagedSet` instances** own all long-lived resources
2. **A tick / event loop** computes desired state and calls `reconcile`
3. **Everything downstream** borrows from the set transiently within a tick

This eliminates the need for `Arc` in the interior of the app. `Arc` is still
appropriate at true shared-ownership boundaries (e.g. passing a connection to
multiple threads), but the *lifecycle* question — when to create and destroy —
is handled by the reconciler, not by reference counting.

The result: memory management feels almost automatic (like a GC) but with
deterministic timing, zero overhead, and no cycles possible.

## Open questions

- Can the pattern be extended to handle one-to-many relationships (one resource
  shared across multiple sets) without reintroducing `Arc`?
- Is there a composable way to express hierarchical desired state (a set of
  sets) so that child resources are automatically freed when their parent exits?
- Could this pattern be extracted into a standalone crate with a richer trait
  surface, making it useful beyond tauler?
