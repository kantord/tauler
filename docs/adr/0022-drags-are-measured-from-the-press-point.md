# Drags are measured from the press point

Every pointer a handler receives carries `press_x` and `press_y` beside `x` and `y` —
where the button went down, in the same element-relative CSS pixels as the position
itself. On the press the two are the same point, and they stay in the payload for the
life of the capture ([0020](0020-controls-capture-the-pointer.md)).

```js
p => {                                  // a control that reads displacement
  const dx = p.x - p.press_x;
  const dy = p.y - p.press_y;
}
```

## Why the runtime has to supply it

A handler is a pure function of one event ([0021](0021-a-handler-is-intents-or-a-function.md)):
a pointer goes in, intents come out, and nothing is kept between calls. That is enough for
a control that reads a *position* — a slider asks what value is under the pointer, and the
pointer alone answers it. It is not enough for a control that reads a *displacement*, which
has to know its press point.

On a web page you keep that yourself, in a variable a `pointerdown` handler wrote. tauler
has nowhere to put it. Every tick rebuilds the tree and every closure in it
([0007](0007-every-tick-re-renders-everything.md)), so a variable a handler wrote would be
thrown away mid-drag, and a control is not allowed to be a second source of truth for its
own value in the first place ([0012](0012-controls-are-never-self-holding.md)).

The runtime, though, already keeps exactly this. A capture snapshots the element's box at
press because a drag outlives the tree that started it. The press point sits in the same
snapshot, costs one more pair of floats, and is dropped at release with everything else.

## Why not a per-event delta, and why not speed

The obvious alternative is the DOM's `movementX`/`movementY`: how far the pointer came
since the *last* event rather than since the press. It is strictly worse here.

Both make the result depend on how many motion events X11 happened to deliver. A per-event
delta has to be summed by someone to be useful, and there is nobody to sum it. Anything
scaled by speed — pointer acceleration, the reflex when a fast flick feels like it should
travel further — goes further wrong: the same physical gesture lands somewhere different on
a busy machine than an idle one, and sweeping back does not undo it, because the return
trip is scaled too.

Measuring from the press has neither problem by construction. The answer depends only on
two points, so the path between them, the speed along it, and the number of events sampling
it are all irrelevant. A gesture is reproducible, and reversible by reversing it.

## Consequences

**Displacement controls need no further runtime change.** A rotary knob, a two-axis pan, a
scrub — each is a mapper over two points, in JavaScript, exactly as `<Slider>` is a mapper
over one. `<Knob>` was written without touching anything below it.

**Nothing jumps to meet the pointer.** A control reading displacement starts every drag at
zero, so pressing it anywhere leaves it where it was. A control reading position, like
`<Slider>`, keeps jumping to the press — which is what a slider should do, and why this is
the handler's choice rather than the runtime's.

**Two points make that possible, not automatic.** The guarantee holds only where the
mapping is well behaved across them. A mapping with a singularity has to defend it itself:
`<Knob>` reads a bearing about the dial's centre, which is undefined *at* the centre and
wildly sensitive near it, so a press near the middle would leap — the displacement is zero
but the reading is not. It answers by refusing to report from inside its own hub. Any
mapper with a pole, an asymptote or a fold owes the same defence.

**Displacement is bounded by half a circle, and it does not matter.** Two points give an
angle in −180°..180°; there is no reading of them that says "one and a half turns". For
`<Knob>` this is invisible, because the angle it reports wraps into 0–360 and on a circle a
sweep of +270° arrives where one of −90° does. It would matter to a control that counted
whole turns, and nothing here counts them.

**A control that does not want it ignores it.** `<Slider>` reads `x` and never looks at
`press_x`. The payload gained two fields; no existing handler changed.
