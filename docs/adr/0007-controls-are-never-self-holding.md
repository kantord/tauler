# Controls are never self-holding

A Control component renders the value it is given and emits intents when touched. It never
stores a value of its own. Whatever the control adjusts — a Module, a stream, `globals` —
owns the value, and the control sees the result only on the next tick.

## Why not a value store

The obvious alternative was a runtime-held, key-addressed state system: Observable's
`viewof`, or React's `useState`. It was rejected on two counts.

It would be a second source of truth beside stream values, so "where does this number come
from" would have two answers, and a widget could disagree with the process that owns its
subject.

And it would break the rule that nothing survives a tick except `globals`. The layout file
is a stateless reducer over stream values; a control that remembers things is no longer part
of that reduction.

## Consequences

A control with no external owner has nowhere to keep its value. A slider that only filters
what a chart displays still needs a Module, a stream or `globals` behind it. That cost was
accepted deliberately — the alternative gives every widget two places to look for its state.
