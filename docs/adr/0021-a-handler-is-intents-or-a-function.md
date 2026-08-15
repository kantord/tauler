# A handler is intents, or a function returning them

`on_click` and `on_drag` each accept two shapes: an array of intents, or a function that
takes the pointer and returns one. Functions are kept on the JavaScript side during
evaluation, referenced from the layout tree by id, and called at input time.

```jsx
on_click={[i3.focus({ ws })]}                       // data, unchanged
on_click={p => [i3.focus({ ws })]}                  // same thing, computed
on_drag ={p => [m.set({ level: level(p) })]}        // the reason this exists
```

This ends "no JavaScript runs on input", which was true until now and is documented as a
property in several places. Those all have to be rewritten.

## Why something had to change

A captured drag hands the handler a pointer position
([0020](0020-controls-capture-the-pointer.md)). Something has to turn that position into
intents, and there were three places it could live.

**The module.** It receives `{x, y, width, height}` and does the arithmetic. Nothing new in
the runtime and nothing new in the language — but then every backend reimplements the same
clamp, divide and round, and `<Slider>` stops being a component and becomes a drag surface
that each module has to finish. A generic control whose contract is "and now you write the
slider maths" is not a generic control.

**The runtime, from a declarative spec.** A mapping written in JSON — an axis, a range, a
step, a clamp flag. It covers a horizontal slider and then grows: the second axis needs
another entry, then inverted axes, then non-linear response, then interactions between the
two. Configuration languages that begin at "map x onto 0–100" do not stay there, and every
step of that growth is a thing to specify, parse, document and version.

**JavaScript, at input time.** A function. It is what the DOM does, it has no ceiling, and
it needs no syntax at all because the language is already there.

## Why this does not undo what it looks like it undoes

**Controls still hold nothing** ([0012](0012-controls-are-never-self-holding.md)). The
function is pure: a position goes in, intents come out, and nothing is retained between
calls. The rule 0012 established was that a control must not be a second source of truth for
its own value, and a stateless mapper is not one. What 0012 rejected was `useState`, not
arithmetic.

**Intents are still delivered verbatim.** The runtime does not rewrite what a handler
produces; it delivers it. The difference is only that the handler produced it a moment ago
rather than a tick ago. No placeholder syntax, no substitution pass, and a module still
cannot tell whether the object it received was written as a literal or computed.

**Rendering is still a pure function of streams** ([0007](0007-every-tick-re-renders-everything.md)).
A tick still takes stream values and returns a tree. Input is a separate path that reads
nothing and writes nothing; it calls a function and dispatches the result.

**It was never a structural barrier.** JavaScript already runs inside the event loop — the
output-change branch re-evaluates the whole layout synchronously while handling a presenter
event. Handlers were data because functions do not survive JSON serialization, which is a
fact about the transport, not a principle. Keeping the function on the JS side and passing
an id across is the ordinary way around it.

## How a function gets to input time

During evaluation, the node flattener replaces a function in any `on_*` attribute with
`{"$handler": n}` and keeps the function in a JavaScript-side registry, rebuilt each tick.
Only real node attributes reach it — a component's props are consumed by the component
before its output is flattened, so `<Slider on_change={fn}>` never registers anything.

On a press, the runtime moves the referenced function into a capture slot, where it survives
the ticks the drag spans ([0020](0020-controls-capture-the-pointer.md) explains why it is
copied rather than re-resolved). Release clears the slot.

## Consequences

**A handler can now be slow, or throw.** It sits on the input path, so a mapper that loops
forever hangs the bar the way a bad render function already can. A throwing handler is
caught, logged once, and dispatches nothing — a click that does nothing beats a bar that
dies.

**Two shapes to read.** Someone reading a layout has to recognise both forms. The
alternative was making every handler a function, which would have rewritten every `on_click`
in every layout, every doc page and every test to add `() =>` in front of a value that needs
no computation.

**The escape hatch is general.** Anything that wants to compute intents from a pointer now
can, without a runtime change: an XY pad, a colour field, a two-axis pan. That generality is
the whole reason this was preferred over a mapping spec.
