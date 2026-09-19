# A delay on an interactive path is admitted only against a Trace

Every thread in tauler sleeps only until something it is waiting for happens; nothing
sleeps on a clock to check. A thread has sources — a channel, a display-server socket,
a file watcher, a stop signal, and sometimes a genuine deadline such as a Refresh
interval or the Repaint floor — and it blocks on all of them at once. Whoever produces
input wakes the consumer: sending *is* waking. A wait longer than the sources require
is admitted in exactly two cases: it is a domain interval with a budget stated in
`CONTEXT.md`'s latency classes, or a **Trace** of the reference interaction shows the
path is better end to end with it than without, and no interactive path gains a delay
a person could notice. Anything else is a defect.

The reference interaction is dragging a Control inside a full-height Panel, pointer
motion to pixels on screen. Its budget: input-to-pixels inside **Minimal** wherever the
rasterization Hop allows it, every non-raster Hop **Negligible**, and those non-raster
Hops summing to no more than 10 ms Stacked.

## Why

The tempting optimizations are all timers: poll less often, batch more, cap the repaint
rate. Each trades a per-event cost that is easy to see in a profile for a per-event
latency that is invisible there, because a profile shows where CPU goes and not where
time goes.

The presenter's 8 ms poll is the worked example, and it was never a decision. The first
rendering loop (`806cd53`, April 2026) was one thread that could not wait on both a
channel and the X socket, so it polled the socket and waited 50 ms on the channel —
"50ms timeout for X11 event polling", in its own words. When the presenter got its own
thread (`a6e8ab6`, #22) the shape moved with it and the number dropped to 8 ms: a
latency fix, taking the worst-case input Hop from Slow to Negligible, paid for with 125
wakeups a second for the life of the process. The macOS backend (#325) copied the 8 ms
into winit's `WaitUntil`. The constraint that justified the poll in April vanished two
weeks later, and the poll outlived it by five months. ADR 0024 had already removed the
same defect from the main loop; this ADR removes it from the definition of a thread.

The rule is not "never add a delay". Motion events are collapsed per Pass and a Module
gets one intent at a time (ADR 0024, 0025); both delay individual events and both make
the path faster end to end. What separates them from the poll is that the win can be
shown on a Trace. That is the bar: a delay is a claim, and a claim needs a measurement
no coarser than the class one step faster than the one claimed.

## The Trace

tauler keeps a permanent, opt-in Trace: an X event's receipt is stamped in the
presenter, and that stamp travels with the pointer event, the intent, the stream value
it produces (attributed to the newest intent outstanding on that Channel, since a Module
never learns what caused a message), the render request and the frame, so that one log
line per frame names every Hop from socket to `PutImage` flush. A request superseded in
the worker's slot is counted, not lost. It is a debug tool, off by default, and the only
evidence this ADR accepts. A wall-clock measurement from outside the process brackets it
at the start and end of an optimization pass, because the Trace cannot see the X server
or the compositor.

## What the rule is, and is not

It is a review rule and a seam, not a type. The threads it governs split into two
families with no common representation: those waiting on channels and deadlines (the
main loop, the Render worker, the reconciler — `std::sync::mpsc` suffices, platform
neutral) and those waiting on a file descriptor (the presenters, the `tauler:outputs`
thread — `poll(2)` or the platform's own event loop). A `Driver` trait over both would
be two implementations sharing a name, and a Linux-only one at that; it is not built.

The one shared piece of code is the command sender into a **Presenter**: a plain channel
plus "and wake the receiver", the pattern `PresenterEvents` already uses in the other
direction. On Linux the wake is a ping into the presenter's event loop (calloop, already
compiled through smithay-client-toolkit, and confined to `src/presenter/`); on macOS it
is winit's `EventLoopProxy`; in tests it is nothing. The presenters keep the same shape
on all three platforms — block on commands and display events together, a `Shutdown`
message ends the loop — and each platform's own event loop provides the blocking. The
display half of each presenter stays per platform, because the platforms genuinely
differ there; sharing it would be a separate refactor of the macOS backend onto
`DisplayManager`, not part of this rule.

## Consequences

The rasterization Hop is accepted as it is. Repainting a Panel costs what ADR 0011 says
it costs, and dirty tracking, partial repaint and `<BufferBoundary>` are architectural
changes (ADR 0007, 0011, 0023) outside any optimization pass; a layout author who needs
a Control to repaint cheaply gives it a small Panel of its own, which ADR 0011 already
names as the author's lever.

A stop is a source. The reconciler, its watchdog and the `tauler:outputs` thread sleep
in 50 ms slices today so that a stop flag is noticed; under this rule the stop is a
channel or a pipe end they block on, and the slices go.

The main loop's 2 ms coalescing floor (ADR 0024) is an unconditional sleep during which
no input can wake the thread. ADR 0024 admitted it by argument; under this ADR it keeps
its place only until a Trace has said whether it earns it, and if it stays it takes the
Render worker's shape — a deadline folded into the wait, not a sleep before it.

Idle cost — wakeups per second on a static desktop — is a separate goal with a separate
measurement (context-switch counts per thread) and is ranked by battery and CPU. It is
judged by this ADR the moment a fix touches a path an input travels: the blocking
presenter removes both the idle poll and the 0–8 ms input Hop, while a variant that
lengthened the poll when idle would have added up to 100 ms to the first click after a
quiet spell. The first was taken; the second is what this ADR exists to refuse.
