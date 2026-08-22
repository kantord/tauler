# Every tick re-renders everything

A layout file borrows JSX syntax from React and almost nothing else. There is no virtual
DOM, no reconciliation, no diffing, and no component identity that survives a tick. When
any stream value changes, the whole layout function runs again and returns a fresh tree.

The closest accurate analogy is a server-side render: each tick is a pure function of the
current data snapshot, and its output is a static tree rather than a live hierarchy.

## Why

The thing being rendered is a status bar — a few hundred nodes that change when a
subprocess prints a line. Re-evaluating all of it costs about 2ms. A reconciler would add a
tree diff, a component-identity scheme, and a mount/unmount lifecycle to save a fraction of
that, against a rasterization pass that still dominates everything.

This ADR originally said 100–200μs. That number was never measured; ~2ms is. The decision
does not change — 2ms against a rasterization pass an order of magnitude larger is still
not worth a reconciler — but the margin is smaller than the original argument implied, and
a future reader should weigh it at its real size.

Skipping work still happens — just at a coarser grain, and without identity. Each panel is
cached by the canonical JSON of its own subtree, so a panel whose content did not change is
never rasterized again (see [0011](0011-panels-are-cached-by-canonical-json.md)). That
gets the benefit a reconciler would have bought, at the level where the cost actually is.

## Consequences

Components are plain functions. There is no registration, no hooks protocol, no ordering
rule, no dependency array, and no cleanup function — `useStringStream` and friends are
named for familiarity, but they are Rust-registered globals that read a value out of a map,
not React hooks.

Nothing survives between ticks except `globals`, which is a deliberate escape hatch and the
one thing that makes the layout function impure. Everything else must be derivable from the
current stream values.

Anyone arriving from React will expect the wrong thing here, in the direction of assuming
more machinery than exists. The differences matter more than the similarities.
