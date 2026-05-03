# Pipeline & Structural Recursion Vision

## Core insight: data depth = logic depth

The architecture follows the principle of **structural recursion**: the nesting depth of the
data structure mirrors exactly the nesting depth of the logic. Each level of the pipeline
peels exactly one layer from the input tree and hands a strictly smaller piece to the next
level. No level reaches past its own boundary.

This is already visible in the JSX layout:

```
<root>                          ← pipeline root / main loop
  <panel id="sidebar">          ← monitor/panel level
    <Module>                    ← data source level
      <WorkspaceList>           ← takumi subtree (currently a black box)
        <container>             ← (theoretical) widget reconciler
          <text>                ← (theoretical) leaf reconciler
```

Each layer corresponds to one reconciler level. The JSX tree is not just UI — it is the
architecture made visible. The depth of the tree specifies the depth of the computation.

### Why this matters

- **Each level is independently testable.** A panel-level test only needs a panel subtree.
  It does not instantiate a root or a monitor.
- **Complexity is bounded per level.** Each reconciler only understands its own slice of the
  data. It cannot accidentally reach into a sibling or parent.
- **Zero-cost at depth in Rust.** Generic reconcilers monomorphize away. A ten-level deep
  tree of typed reconcilers compiles to the same machine code as hand-written imperative
  updates. There is no runtime dispatch overhead for depth.

This converges on the same principle from two directions: the code-review "narrow inputs /
value drilling at the boundary" principles are the per-function statement of the same
constraint that structural recursion expresses at the system level.

---

## Fan-out is inherent, not special-cased

Fan-out arises naturally when one level's desired state is a collection and each item spawns
its own child reconciler. No special framework support needed — it falls out of
`Lifecycle::enter()` creating a child `ManagedSet`.

Fan-out already exists in the actual tauler config at three levels:

```
root
├── sidebar panel (static)
│   ├── tauler-i3 process
│   ├── weather process
│   └── 2× bash processes (BashCard)
├── outputs.map(o => <panel>)       ← fan-out: 1 stream → N monitor-dot panels
└── notifications.map((n,i) => <panel>)  ← fan-out: N notifications → N panels
```

The notification case also exposes the **ordered key problem**: keys are `notif-pos-${i}`
(index-based). Dismissing notification 0 causes every remaining panel to exit and re-enter.
Stable keys (notification ID) plus a position field would reduce this to one exit + position
updates on the rest.

---

## The streaming pipeline is not optional

Streams are not bolted onto the reconciler system — they are what make reconcilers composable
at all. Without them:

- The reconciler must know *where* desired state comes from (config file? API? parent
  reconciler?). That coupling prevents composition.
- The reconciler must know *where to send* health events, logs, and errors. Callbacks or
  polling creep in.
- If desired state changes faster than reconciliation runs, queuing logic lives inside the
  reconciler instead of in the pipeline.

Once a reconciler has stream I/O (desired states in, health/log events out), all of that
becomes the pipeline's concern. The reconciler collapses to: consume a stream of desired
states, produce a stream of lifecycle events.

In Rust terms, this is `futures::Stream` + `StreamExt`. The transducer model from Clojure
maps directly onto composable async streams — `map`, `filter`, `flat_map` are already
first-class. No separate transducer library needed.

### Fan-out in the streaming model

Fan-out in a streaming pipeline uses `flat_map`: when an item enters the outer reconciler,
its child reconciler emits a stream of events; `flat_map` merges all child streams into one.

```
desired_panels → [Panel Reconciler]
                    panel A enters → [Process Reconciler A] → events ──┐
                    panel B enters → [Process Reconciler B] → events ──┤ flat_map
                    panel A exits  → Process Reconciler A torn down    │
                                                                        ↓
                                                        aggregated event stream
```

Cleanup is structural: when the outer reconciler drops an item, the child reconciler's stream
ends. No manual coordination.

---

## Black-boxing and the leaf boundary

The nested reconciler model naturally supports treating any subtree as a black box from the
parent's perspective. The parent only sees: enter, update, exit. What the child does
internally is opaque.

Today, the takumi layout engine is the black-box boundary. Costae hands a `serde_json::Value`
subtree to takumi; takumi owns everything below that point.

In a fully recursive system, the takumi boundary dissolves: collection nodes (`<container>`,
`<row>`) become collection reconcilers; leaf nodes (`<text>`, `<image>`) become singleton
reconcilers. The same enter/update/exit interface applies at every level.

**The right leaf granularity is not arbitrary.** Going to individual CSS property granularity
would pay reconciler overhead to avoid costs the renderer already handles internally.
The right leaf node is the unit where re-rendering is expensive — a widget or draw call, not
a style property.

This insight also bounds ambition: black-boxing a subtree (leaving takumi as-is) is always
valid. Recursing deeper only pays off when the re-render cost at that level justifies the
added complexity.

---

## Connection to industry art

The combination of streaming pipelines with declarative reconciliation is **not a single
standardised named concept**. The closest term in circulation is **"reactive control plane"**
— a system where desired state is produced reactively (from event streams) and a reconciler
closes the gap against actual state. This term appears in cloud-native infrastructure writing
but has no formal definition. Knowing this is the closest label helps when searching for
related prior art.

| System | How it expresses the same pattern |
|---|---|
| **React** | Reconciler diffs virtual DOM tree; component = composable reconciler unit |
| **Xilem** (Rust) | `View` trait = typed reconciler; tree monomorphizes to zero overhead |
| **Elm TEA** | Nested `Model`/`Msg` — each level handles its own slice, children are opaque |
| **Erlang OTP** | Supervisor tree — each supervisor owns its children, health propagates up |
| **kube-rs** | Watch stream triggers controller reconcile; composable controller runtime |
| **Crossplane / Flux** | GitOps desired-state source feeds a streaming reconciler loop |

Costae arrives at the same place from the process-management direction rather than the GUI
direction. React arrived from the GUI direction. They converge on the same abstraction —
the reconciler — because the problem structure is identical: desired state in, minimal
operations out, composable at any depth.

---

## Context composition across levels

Context in a nested reconciler tree is not a single thing. There are three distinct tiers,
each with a different scope and access model.

### Tier 1 — level-local context (current `T::Context`)

The supervisor at a given level owns a context value and passes it to every item it manages.
Items at the same level share this context. This is what `T::Context` already is today.
Each supervisor provides its own tier-1 context independently.

### Tier 2 — inherited context via state-as-context

A supervised supervisor is simultaneously a `Lifecycle` subject (supervised by its parent)
and a `Supervisor` (managing its own children). Its own `State` — which the parent tracks —
naturally becomes the context for its children. This can be enforced at the trait level:

```rust
trait Supervisor: Lifecycle {
    type ChildItem: Lifecycle<Context = Self::State>;
    //                        ^^^^^^^^^^^^^^^^^^^^^^
    // compile-time: children's context IS this supervisor's state
}
```

Context flows down through the tree as a consequence of nesting, not as a separate
mechanism. No special machinery needed — the type constraint does the work.

**Implementation constraint.** This requires passing `&self.state` as the child context
during reconciliation. Because `self.state` lives inside `ManagedSet`, the borrow checker
will object to simultaneously borrowing it as context while mutating managed items. Likely
solved by splitting state into a "shared-with-children" portion (behind `Arc`) and a
"private" portion.

### Tier 3 — global pipeline context (React Context analog)

Cross-cutting concerns (global config, feature flags, display DPI) that are not tied to the
tree structure. Any node can subscribe to a part of this context if explicitly wired in as a
context client.

**The write access rule.** React learned that allowing any consumer to write to shared
context creates unpredictable update cascades. The safe model:
- One **provider** node can write (like a reducer/store)
- **Consumer** nodes can only read, via explicit wiring
- A consumer wanting to change global state sends a message *up* to the provider; the
  provider updates and the new value flows back down

This maps to the single back-edge rule in `datapipe-vision.md`: one allowed feedback arc,
everything else flows downward.

---

## Open questions

- **Error propagation across levels.** `ReconcileErrors<K, E>` is flat. A deep failure loses
  its path. See `health-vision.md` for the error path design and `lifecycle-status-vision.md`
  for the argument that errors are primarily log events — structural failure information is
  carried by `ItemStatus.convergence` instead, which propagates naturally through
  `health_snapshot()`.

- **Tick propagation in deep trees.** The pipeline orchestrator drives timing by sending
  typed messages (including "reconcile now" ticks) to each stage independently, at different
  rates. For deeply nested reconcilers the orchestrator can either route ticks directly to
  inner levels (requires topology knowledge) or the parent propagates a tick signal to
  children via context during its own reconcile cycle. The latter keeps the orchestrator
  topology-agnostic; timing propagates structurally alongside data.

- **Reconciler boundary depth.** The JSX evaluator is the desired-state source node at the
  top of the pipeline: it reacts to data streams and emits a new tree whenever any input
  changes. That emission is the message that triggers the top-level reconciliation cycle.
  The current `serde_json::Value` boundary is well-defined and fits the pipeline model.
  The open question is not "how to integrate JSX" but *at what depth the reconciler
  boundary lives*: today it stops above takumi; theoretically it could reach widget nodes,
  at which point the evaluator would need to emit typed reconciler descriptions rather than
  raw JSON. That is a deliberate future choice, not a current gap.

- **Ordered collection reconciliation.** `ManagedSet` is unordered (HashMap). UI children
  need stable keys + position + a diff algorithm that emits moves. A different data
  structure implementing the same `Reconcile` trait interface would handle this naturally —
  the trait design does not need to change, only the implementation.
