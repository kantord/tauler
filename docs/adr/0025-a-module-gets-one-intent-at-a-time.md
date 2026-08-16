# A Module gets one intent at a time

A Module is handed one intent and nothing further until it emits a line. Intents produced
for that Module in the meantime replace each other in a slot rather than queueing, so what
it receives next is the newest one, and the ones in between are never sent.

## Why

A Module is a subprocess reading its stdin a line at a time, and nothing about that says
how fast it reads. The volume module on the desktop this was found on forks `jq` twice and
`wpctl` once per intent, serially, in a bash `while read` loop — twenty-five to sixty
intents a second at best. A drag produces one intent per Pass, and Passes ran at a hundred
and fifty a second.

Nothing stood between those two numbers. The intents went into an unbounded channel, so a
seven-second drag left the module minutes-of-work behind, and the slider went on moving for
seconds after the pointer stopped — showing, faithfully, positions the pointer had been in
several seconds earlier.

The queue was the whole defect. It could not be tuned away, because the ratio depends on a
subprocess this project does not control: any Module slower than the pointer reproduces it,
and the next one written will be slower than some pointer somewhere.

**Why not rate-limit instead.** A fixed ceiling would need a number, and the right number is
a property of each Module — unknowable here, and different on a laptop under load than on a
desktop. Waiting for the answer needs no number: a fast Module is asked often, a slow one
seldom, and neither is configured.

## Consequences

The queue is at most one deep, so what the bar shows is the freshest thing the Module has
produced rather than the head of a backlog. A slow Module degrades to a lower update rate,
which looks like a coarser drag — it does not degrade to a lag that grows for as long as
you keep dragging.

**A Module that never answers is not silenced.** Plenty of them only act; a "play a sound"
Module has nothing to report. Waiting forever for an answer that is not coming would take
its channel out of service after one intent, so an unanswered intent stops holding its
channel after `REPLY_GRACE` (40ms) and such Modules settle at that rate instead.

**A click is never superseded.** A drag's intents describe where the pointer is, and only
the newest is worth sending. A click describes something that happened. They can share a
channel — a `<Slider>` and the mute button beside it usually do — so a click goes out
whatever is in flight, and claims the channel so the drag behind it waits rather than
racing it. Dropping a mute because the pointer moved afterwards would be losing an event,
not superseding a position.

**Channels are independent.** One slow Module says nothing about any other, so a bar with a
fast clock and a slow mixer behaves as two separate things.

This is the third place the same shape appears: one slot, newest wins, nothing queued. A
Render target holds one unpainted frame (ADR 0023), the loop holds one wakeup (ADR 0024),
and a Module holds one unanswered intent. The recurring lesson is that anything with a
producer faster than its consumer wants a slot rather than a queue — and that the way to
find out which those are is to measure, not to reason. Two rounds of this investigation
were spent optimising parts of the pipeline that were never the bottleneck.

## What this does not fix

The Module is still slow, and the update rate while dragging is now bounded by it. That is
the honest ceiling — tauler cannot show a value the Module has not produced (ADR 0012) —
but it means a Module that forks per intent still feels coarse. That belongs to whoever
wrote the Module, and it is now a visible frame rate rather than an invisible backlog.
