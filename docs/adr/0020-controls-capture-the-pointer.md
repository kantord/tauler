# Controls capture the pointer

An element with `on_drag` captures the pointer when a button goes down on it. Every motion
until release is delivered to *that* element, whatever is underneath, and the handler
receives the pointer's position relative to the element's own box. The element's geometry
and its handler are snapshotted at press and dropped at release.

This is `setPointerCapture` and `pointermove`, which is how every slider on the web works.

## Why the position is 2D and unclamped

The handler gets `{ x, y, press_x, press_y, width, height, buttons }`, in CSS pixels
relative to the border box. `x` goes negative left of the box and past `width` to the right
of it, and the same vertically — exactly what `offsetX`/`offsetY` do under capture in a
browser. `press_x`/`press_y` are where the button went down, for controls that read a
displacement rather than a position ([0022](0022-drags-are-measured-from-the-press-point.md)).

Not a fraction, and not a value: dragging is two-dimensional, and any single number is
already wrong for an XY pad, a colour field or a two-axis pan. Clamping is a policy the
handler can apply and the runtime cannot take back. `buttons` is a DOM-style bitmask,
carried because X11's `MotionNotify` already supplies it in `state`.

The runtime therefore knows nothing about ranges, steps or sliders. `<Slider>` owns that
arithmetic, in JavaScript, where a `min` and a `max` are visible to the person who wrote
them ([0021](0021-a-handler-is-intents-or-a-function.md)).

## Why not one clickable element per settable value

The rejected design was the opposite routing: no capture, re-hit-test every motion, and one
element per step — twenty-one `div`s for a `step={5}` slider, each carrying the intents for
its own value, baked at render time. The value would then be computed by the component
during evaluation, and no coordinate would ever enter the input path.

It was genuinely attractive. It needed no runtime state, no callbacks, and the intents
stayed literal. It also had a property capture cannot match: because a zone the pointer is
still inside resolves to the intents already sent, the flood of motion events collapses to
one dispatch per zone crossed, for free.

It lost on three counts, in increasing order of weight:

- **Resolution is the step**, and the node count is the step count, so a range wanted a cap
  (256) and a fractional device pixel ratio left sub-pixel seams between abutting zones.
- **It is 1D by construction.** Zones tile a line. Nothing about the design extends to an
  area, so the second control that needed two axes would have had to abandon it.
- **Nobody arrives knowing it.** It is a tauler-specific idiom for a problem every UI
  toolkit has already solved the same way. Capture is what a person bringing web experience
  expects, and a bar is written by people bringing web experience.

The dedup survives anyway — see below — so the one thing zones were uniquely good at was
not actually theirs.

## Why the press is the first drag event

`on_drag` fires on the press, not only on movement after it. The DOM splits these into
`dragstart` and `drag`, and a slider that ignored a click with no movement would be broken,
so the split would only mean every control carrying two handlers with identical bodies.
Merging them makes a control need exactly one handler and makes a plain click work by
construction.

`on_drag_end` is not exposed. Release is wired, because capture has to end somewhere, but
nothing needs to observe it yet — a control that is too expensive to drive live is the
reason to add it, and no module is.

## Why the handler is snapshotted rather than looked up again

A drag outlives the tree that started it. Every tick rebuilds everything, including a fresh
closure over fresh props, so "run the current handler" needs a node identity that survives a
tick. tauler has none: no reconciliation, no keys ([0007](0007-every-tick-re-renders-everything.md)).

Terminating the drag when the tree changes is not an alternative, because a tick is not an
event — it is the heartbeat. A bar re-evaluates whenever any stream produces a line, which
on a working desktop is bursty and often several times a second. A drag lasting a second
routinely spans a few, so ending one on each tick would end every drag.

Snapshotting diverges from the DOM, which re-attaches handlers on every render. The
divergence is only observable if a control's `min` or `max` changes *during* the half-second
somebody is dragging it, and it corrects itself on release.

## Consequences

**No timer, no throttle.** Motion is only reported when the pointer actually moves, so
dispatches are bounded by pixels traversed rather than by event rate. Dispatching is skipped
when the handler returns the intents it returned last time, which — because a mapper almost
always rounds — collapses a full sweep of a 200px track at `step={5}` to twenty-one
dispatches. A control with no quantisation at all is bounded by its own width in pixels.

**X11 only.** The mask is `BUTTON1_MOTION`, so motion is reported only while button 1 is
held and a bar nobody is dragging costs nothing extra. The Wayland presenter reports no
motion, so a control there is click-to-set; macOS likewise. Those presenters still emit a
release after each press — a capture nothing ever ends would pin a stale handler for the
life of the process.

**Hover is not available, and this is why.** Delivering motion with no button held needs
`POINTER_MOTION`, and resolving it needs `hit_test`, which runs an uncached takumi layout
per call. That is fine once per click and ruinous once per mouse twitch. A captured drag
never pays it, because the box was snapshotted. So no hover event is named here: naming one
we cannot fire would be inventing an API for a constraint we have not solved.

**A drag off the panel sends nothing rather than sticking.** X11 grabs the pointer to the
window the press landed in, so motion keeps arriving with coordinates outside the surface.
Those produce a position outside the box, which is exactly what the handler asked for — it
can clamp, or not.
