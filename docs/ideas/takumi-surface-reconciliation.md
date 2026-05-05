# Takumi Surface Reconciliation

## Core idea

Embed the abstract reconciliation system directly into takumi so that each
independently-updating render target — a panel, a notification, a monitor dot —
is represented as a first-class `TakumiSurface`. A surface owns its output
image and its local caches (text layout, shaped glyphs, rendered subtrees).
A `TakumiGroup` owns a collection of surfaces and mediates sharing of resources
that are expensive to duplicate (font data, shaping tables, image stores).

## Why

Right now tauler manages surfaces externally: `ManagedSet<PanelSpec>` tracks
panel lifecycles, `RenderCache` lives on each `Panel` struct, and font/image
state lives in a global context. This works but the boundaries are ad-hoc —
the reconciliation logic, the cache invalidation, and the render pipeline are
three separate systems that have to be manually kept in sync.

If takumi itself understood surfaces and groups, the library would own the
full lifecycle: create surface → render → diff → cache → destroy. Costae would
just hand takumi a desired set of surfaces and a stream of value updates, and
takumi would figure out the rest.

## What a TakumiSurface would be

- A single updateable render target (an image / framebuffer)
- Owns its own text layout cache, glyph cache, and render subtree cache
- Keyed by a stable identity (panel id, notification slot, etc.)
- Knows how to invalidate and repaint only what changed
- Lifecycle: `enter` (allocate buffers), `update` (diff + repaint), `exit` (free)

## What a TakumiGroup would be

- A reconciled collection of surfaces, driven by a desired-state iterator
- Holds shared resources: font data, shaping tables, persistent image store
- Surfaces within the group can borrow shared resources without cloning
- The group's reconcile loop maps directly onto `ManagedSet::reconcile` — the
  same enter/update/exit semantics, just owned by takumi rather than tauler

## Why this replaces the need for a garbage collector

Because lifetimes are explicit and driven by reconciliation. When a surface is
removed from the desired set, `exit` is called immediately — caches freed,
buffers released. There is no reachability graph to traverse. Shared resources
in the group live exactly as long as the group. Nothing is alive longer than its
owner, and the owner's lifetime is determined by the reconcile loop, not by
reference counting or a GC cycle.

This is the same reason `ManagedSet` avoids GC for process lifecycle management:
explicit enter/exit gives you deterministic resource cleanup without tracing.

## Relationship to the current codebase

- `ManagedSet<PanelSpec>` → replaced by `TakumiGroup::reconcile`
- `RenderCache` on `Panel` → owned by `TakumiSurface`
- `GlobalContext` (font store, image store) → owned by `TakumiGroup`, borrowed
  by surfaces
- tauler's render pipeline → calls `group.update(surface_id, new_layout_value)`
  and gets back a framebuffer or a diff

## Open questions

- Should `TakumiGroup` be generic over a key type (like `ManagedSet<T>`) or
  opaque with string keys?
- Can the shared font/image store use Rust's borrow checker to enforce that
  surfaces don't outlive the group, or does that require `Arc`?
- Does this belong in upstream takumi or in a tauler-specific wrapper?
