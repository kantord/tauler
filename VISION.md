# Architecture Vision

## Core idea

One event loop at the top of the stack. All layers below it are pure reconcilers —
they receive desired state and apply the diff. No layer has its own loop or reaches
sideways into another layer.

## Event flow

```
[Wayland/X11 socket] ─┐
[file watcher]        ─┤──→ event queue ──→ CostaeLoop ──→ ReconcilePipe
[data streams]        ─┤                                        ↓↓↓↓↓
[timers]              ─┘                                    (all stages)
```

Events flow **up** into the queue. Reconciliation flows **down** through the pipe.
No layer triggers another layer directly.

## Reconcile pipeline

```
CostaeLoop
  └─ ReconcilePipe
       ├─ ReconcileJSX          (stream values → layout tree)
       ├─ ReconcileOutputs      (connected Screens → Screen geometry)
       ├─ ReconcilePanels       (layout tree → windows)  ← X11/Wayland split lives here only
       ├─ ReconcileTakumi       (layout tree → render nodes)
       └─ ReconcileContents     (render nodes → pixels in windows)
```

## Key invariants

- Only one tick loop, owned by `CostaeLoop` at the binary level
- Platform-specific code (X11 vs Wayland) is isolated to `ReconcilePanels` only
- All other stages are backend-agnostic
- `ManagedSet<T: Lifecycle>` is the implementation of each reconcile stage

## Current state vs target

**Current**: Two parallel tick states (`TickState` for X11, `WaylandTickState` for Wayland)
that duplicate all shared orchestration logic. The X11/Wayland split happens at the top
of the stack instead of only at the panel creation layer.

**Target**: One `TickState<B: DisplayBackend>` where `B` supplies only the panel
lifecycle (`Lifecycle` impl) and event source. Everything else lives once.

## Open questions

- Does `DisplayBackend` need anything beyond `Lifecycle` + a way to push events into
  the top-level queue?
- How does `ReconcileOutputs` interact with `ReconcilePanels` (Screen geometry feeds
  into panel placement)?
- `ManagedSet` ctx/output refactor: bind context at construction, `reconcile()` takes
  items only — unresolved, see `pipeline-vision.md`.

## Constraints from existing vision docs

- **`Outcome` associated type** (`lifecycle-status-vision.md`): `reconcile_self` will
  eventually return something richer than `Result<(), E>`. The outcome type should be an
  associated type on `Lifecycle` — each domain defines its own outcome space (panels may
  use Retry, render stages may have no error state at all). A fixed Ok/Retry/Fatal enum
  is a good default implementation but not the only option.
- **Errors are log events** (`health-vision.md`, `logging-vision.md`): `log_lifecycle_errors`
  is the right direction. Eventually errors flow to a structured log stream, not just
  `tracing::error!`. Do not use reconcile errors as control flow.
- **Supervisor trait** (`health-vision.md`, `datapipe-vision.md`): `ManagedSet` will
  eventually be owned by a `Supervisor` that drives reconciliation cadence and exposes
  per-item health snapshots. `TickState` is the current stand-in for this.
