# Observation is the truth channel

What a Unit believes exists comes from its `observe`, and from nothing else. A hook does not
report which Items it handled; running a hook is not evidence that it worked.

A Sweep therefore ends by observing again — immediately, if it made progress, because that
is the moment the world has just changed. If it made no progress it waits the Unit's refresh
interval first. A hook that failed counts as no progress.

Nothing is torn down when tauler exits or re-execs. On start, `observe` seeds the set, the
diff comes back empty, and nothing runs.

## Why

**A hook can be interrupted, and a report cannot survive that.** tauler re-execs when its
own binary changes, which on a development machine is constantly. A hook that half-ran and
reported nothing is indistinguishable from a hook that never ran, unless something looks at
the world.

**Partial batches need it anyway.** A hook is handed many Items and may act on some
([0026](0026-reconcilers-are-esto-units.md)). Either it reports which ones, or the next
Sweep finds out. Reporting makes silence mean success — a hook that throws halfway says
nothing, and the runtime concludes everything worked. Observing makes silence mean nothing
happened, which is the safe direction to fail in.

**The pattern is already in use, hand-rolled.** `meta.op`'s `lib/execState.ts` says it
outright: *"Completion needs no explicit transition: once the real fix lands, observe()'s own
check stops matching."* The same file also hand-rolls a JSON marker with a seven-day
staleness sweep to record that a fix is in flight — because `esto` is one-shot and has no
notion of work in progress. Here that is not userland: a Unit with a hook in flight is not
swept again until it returns. Without that rule, a 40-second `enter` observed every 5s fires
eight times.

**Why nothing is torn down at exit.** The alternative is running every `exit` before
re-exec, the way Modules are torn down. Modules are torn down because they are child
processes tauler owns; a reconciled Item is the opposite — state out in the world, put there
deliberately to outlive tauler. Tearing those down on every `cargo build --release` would
stop a dictation daemon because a binary was recompiled. `OptativeSet::with_initial_state`
exists exactly so a restart is a cold start with a warm world.

**Why refresh is an interval and not a rate.** The gap only matters when nothing is
happening, and what it catches is drift caused by something tauler never saw: a light
switched at the wall, a config file edited by hand, a daemon that died. How fast that
matters is a property of the Unit, so the Unit states it. After a Sweep that did something,
waiting out the rest of an interval is pure latency, so it does not.

## Consequences

**A Unit with nothing to observe does not work.** `optativeJsonSet` — jsonl-persisted state,
for things with no external check — is the answer for those, and it is not part of this. A
notification id or a `mktemp -d` path has no observation, so such a Unit would re-enter
forever.

**An Item dropped while tauler is down never exits.** Delete its line from the layout file
while tauler is stopped and the Item stays in the world until tauler runs again — at which
point observe reports it, the diff finds it undeclared, and it exits then.

**A hook interrupted mid-way leaves whatever it left.** Observe sorts it out on the next
Sweep, which for a non-idempotent script means doing some of the work twice. Every hook has
to tolerate that; there is no arrangement of this design in which it does not.

**A failing hook retries at the refresh interval, not in a spin.** That is the only reason
failure is folded into "no progress" — a hook that fails in five milliseconds would
otherwise re-observe and re-run immediately, forever.

**Two Units observing the same thing will fight.** Nothing detects it; they will simply take
turns, at their own intervals, and the world will flap. Keys are unique within a Unit and
mean nothing across Units.
