# The loop waits on one thing

The main loop blocks on a single notifier. Anything with work for it — a subprocess line, a
pointer event, a file change, a built-in source — pings that notifier, and the pass which
follows drains every channel it owns. The timeout on that wait is no longer how input is
noticed; it is only how often supervision runs when the desktop is idle.

## Why

The loop used to block on subprocess stdout and nothing else, and `try_recv` five other
channels once per pass. So the period of that block was the staleness bound on all five,
and it was set by the fastest consumer among them — pointer input — while being paid by
every idle second of the process's life.

That produced two costs at once. A pointer event waited up to a full poll period before
anything happened: a **Slow** Hop, in a path whose only other Slow hops do real work. And a
completely static bar still woke twenty times a second forever.

Waiting on one thing collapses both. Input is acted on when it arrives, and the idle rate
drops to the slowest thing that genuinely needs polling.

**Why a notifier and not a merged event channel.** The process pool is an external crate
that takes an `mpsc::Sender` of its own item type; it cannot be handed a different channel
and cannot be taught to ping. Two bridge threads convert its items — and the built-in
sources' — onto one item channel and ping as they go. That is also what left `try_recv_item`
reading a single channel instead of interleaving two.

**Why 400ms.** A dead subprocess signals nothing, so this timer is the only thing that finds
one, which makes restarting it the binding constraint rather than the freeze watchdog's
heartbeat (which tolerates ten seconds). `CONTEXT.md` calls that job **Non-interactive**, and
400ms is that class's budget. The number is derived, not chosen.

**Why a floor as well as a ceiling.** A pass may not finish faster than `COALESCING_FLOOR`
(2ms). Passes are what batches are formed in, so a loop free to spin would present each
event alone and there would be nothing to collapse. This is a floor on how often the loop
may run, which is the opposite of the ceiling it replaced — the old constant was a maximum
sleep, this one is a minimum interval.

## Consequences

A drag no longer costs a JavaScript call per motion event. `compress_motion` reduces a
drained batch to the motions worth acting on: a run of motions on one panel collapses to its
last, because a handler maps the pointer to a value against the rect snapshotted at press
(ADR 0022) and remembers nothing (ADR 0012), so the ones in between can only produce intents
the last supersedes. Only *consecutive* motions on one panel collapse, so a press or release
between two of them keeps both, and nothing but a motion is ever dropped.

A drag remains self-clocked. Nothing about a pointer event repaints anything: it produces
intents, the module changes its own state, and the new value arrives as an ordinary stream
line. So the rate is still the module's round-trip rate — the change is that the first
motion of a gesture no longer waits out a poll period first.

The stop flag is now checked immediately after `on_tick` as well as before it, because
`on_tick` is where a replaced binary asks for the re-exec, and the loop would otherwise wait
out a supervision interval before noticing its own instruction.

Items are drained *before* `on_tick` rather than after, so a value is acted on in the pass
it arrives in rather than the one after it.

## What is deliberately not built

The design this came from goes one step further: every stream declares a latency class, and
a value from a tolerant stream is held back until its deadline, so slow data never causes a
frame of its own and only ever rides one that was happening anyway. Two things make that
worth writing down before it exists:

- **The dedupe half already works.** `stream_values` is keyed by `(bin, script)` and
  overwritten as the batch drains, and `changed` compares only the final value. A burst from
  one stream already costs one comparison and one eval. Only the deadline half is missing.
- **It cannot break existing layouts.** Dropping superseded values is already the contract:
  `CONTEXT.md`'s **Accumulator** says tauler keeps only the latest line, and ADR 0014 says a
  layout needing history must run an accumulator subprocess. Nobody is entitled to every
  line, so deferring one takes no liberty that has not already been taken.

Two rules that layer will need, learned by arguing about it rather than by building it. A
deadline must be measured from the *first* unadmitted value, or a continuously-emitting
stream pushes its own deadline out forever and starves — the bug `tauler-i3`'s scheduler
already avoids by taking the minimum of a debounce deadline and a heartbeat ceiling. And the
deadline must be a *render by*, never a *render at*: admitting every pending value whenever
an eval happens for any other reason is what makes tolerant data free rather than merely
late.

A fourth layer — per-lifecycle debouncing inside the reconciler — was considered and
rejected. Reconciliation is already a dedupe: it diffs desired against actual, and an early
value that turns out to change nothing costs a comparison. A timer there would only delay
work the diff would have skipped for nothing.
