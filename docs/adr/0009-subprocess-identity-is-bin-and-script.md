# A subprocess is identified by `(bin, script)`

Every tick re-declares which subprocesses should exist. Rust diffs that set against the
running one: identities present in both keep their process, identities that disappeared are
killed, new ones are spawned. The identity is the `(bin, script)` pair — nothing else, and
notably not the call site.

## Why identity rather than lifecycle

Declaring data the same way the tree is declared means a layout never manages a process. It
states what it wants to read from, on every tick, and reconciliation is somebody else's
problem. That is only possible if two declarations can be recognised as the same thing, and
`(bin, script)` is the smallest pair that decides it: same binary, same script, same
process. Two components asking for the same clock share one subprocess without either
knowing about the other.

The alternative — handles, start/stop calls, cleanup functions — puts process lifetime in
the layout file, which is a re-render away from being wrong.

## Consequences

A changed spec restarts the subprocess, and registering a bin as a module changes its spec.
So the hooks for a given bin must be called **unconditionally**, at the same level of the
same component — never inside a branch that sometimes does not render.

A hook that comes and goes restarts its process on every transition. For a singleton like
`tauler-notify` that means dropped notifications and a momentarily released D-Bus name,
which looks like a bug in the notification daemon rather than a conditional in a layout
file. This is the sharpest edge in the data layer and it is invisible until it bites.

Module props are part of the spec too, which is why `merge_missing` folds later
registrations into earlier ones additively instead of overwriting: children evaluate before
parents, so a wrapper deriving props from its children can only register after them, and
must not be able to clobber what the author wrote by hand.
