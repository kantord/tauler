# A layout file keeps state in Stores, and Rust owns them

A layout file may keep state between Ticks in a **Store**: a JSON object it declares at the
top level of a module. Rust owns every Store; each JavaScript runtime holds only a cache of
it. A handler changes a Store with a plain JavaScript write, and that write causes one Tick.
`globals` is removed in the release that ships Stores (issue #271). See
[CONTEXT.md](../../CONTEXT.md) for **Store** and **State identity**.

```jsx
import { store } from "tauler";

const desk = store({ open: false, items: [] });

<div on_click={() => { desk.open = !desk.open; desk.items.push("x"); }} />
```

This supersedes two standing decisions in part:

- ADR [0007](0007-every-tick-re-renders-everything.md) said nothing survives between Ticks
  except `globals`, a deliberate escape hatch. Stores now survive, and they are not an
  escape hatch. The rest of 0007 stands: every Tick still rebuilds the whole tree, and
  there is still no reconciler and no component identity.
- ADR [0012](0012-controls-are-never-self-holding.md) rejected a runtime-held,
  key-addressed state system, and accepted that a control with no external owner has
  nowhere to keep its value. A Store is now that place. The rule in its title stands: a
  Control component still renders the value it is given and holds none of its own.

## Why 0012's two objections no longer hold

0012 objected that held state breaks the rule that the layout is a stateless reducer over
stream values. A Tick is now a function `(streams, state) -> (layout, changed state)`, and
Rust holds every input and every output. So a Tick can still be replayed, tested or
skipped, which is what the rule was for.

0012 also objected that a value store is a second source of truth beside stream values.
That is accepted. The cost 0012 chose instead was a Module, a Stream or `globals` behind
every control, and in practice that meant `globals`: untyped, one namespace for the whole
layout, and copied into and out of every evaluation. A slider that only filters a chart
has no process that owns its subject, so there is nothing for it to disagree with.

## Rust owns the state

The JavaScript object is never the truth. Three constraints decide this:

- Layout evaluation stays a pure function of inputs that Rust holds.
- A second scripting language, or a component written in Rust, can read the same state.
- State must outlive a JavaScript runtime, because a layout reload builds a new one.

Rejected: the JavaScript object as the truth with Rust as a backup. A Tick would then
depend on values Rust cannot see.

A Store is a JavaScript object over a small set of host functions, not an object type
defined in Rust. The same layout file also runs on the browser's own engine (ADR
[0027](0027-the-browsers-own-engine-runs-the-layout-file.md)), where there is no Rust to
define one; a page supplies the same functions itself.

## No person writes an identity

A Store is declared as `const name = store(default)` at the top level of any module,
exported or private, and a file may declare several. A compile pass gives each one its
State identity, `<module>#<name>`, from the module that declares it and the name of the
`const`. A `store(...)` call anywhere else fails the load.

Rejected: a key the author writes, as `globals` had. A name chosen by a person collides
across Packages, and the importer cannot rename it.

`store` is imported from `"tauler"`. It is not a global like `useEvents`, because the
import gives TypeScript a place to find the types. The compile pass finds tauler's `store`
by scope analysis, so a file with its own `store` function is left alone.

## Writes are plain writes, nested ones included

`desk.count += 1` and `desk.items.push(x)` both work. A top-level write goes to Rust
directly. A nested value that a handler or a render was handed is compared after that
handler or render, and written back if it differs.

Rejected: handing nested values out deep-frozen, so that only
`desk.items = [...desk.items, x]` is a write. It was measured faster: 59.1 µs against
108.5 µs per evaluation when the render reads everything. But a nested write would throw,
and layouts change nested values in place. The comparison still costs no more than
`globals` did: 108.5 µs against 112.8 µs for the bulk copy of `globals` with nothing read.
A render that reads nothing costs 3.9 µs. All figures are medians from
`benches/store.rs`, shape `realistic`: a Store's from the groups `eval_with_store/..`, the
bulk copy's from `eval_with_bulk_copy/read_none`, which does the copy `globals` did now
that the product does not. The 59.1 µs is the exception: the deep-frozen alternative was
not kept, so that figure cannot be run again.

Also rejected: comparing nested values in Rust. Decoding the value is the half of the
`globals` copy that this design removes.

## What a write causes

- A handler's write causes exactly one Tick, in the Pass after the one the handler ran in.
  Several handlers that write in one Pass cause one Tick. A handler that changes nothing
  causes none. Rejected: evaluating again at the end of the same Pass. It saves one
  Coalescing floor and lets a Pass tick twice, with a present between the two.
- A write made during a render is kept and causes no Tick. It shows on the next Tick that
  something else causes, as `globals` behaved. Rejected: a render-time write causing a
  Tick. It loops whenever the value never settles, for example a timestamp.
- A handler or a render that throws leaves every Store as it was before it ran.

## Consequences

**State survives a layout reload.** A Store whose State identity is unchanged keeps its
value, and a key the new default has and the saved value lacks is filled in. Renaming the
`const`, or moving it to another module, resets that Store and no other. A Store that no
evaluation declares after a reload is dropped.

**Unit hooks and D3Figure draw code read Stores and cannot write them.** The reconciler
runtime (ADR [0034](0034-hooks-run-in-a-second-runtime.md)) and the Draw runtime (ADR
[0044](0044-d3figures-draw-in-a-third-runtime-and-arrive-as-pictures.md)) see the same
state as the render runtime, read-only. Both runtimes also run the render, and a write the
render makes there is kept for that evaluation only and never reaches the state.

**`globals` is removed, with no period where both exist.** Two state systems is the
complexity this decision avoids, and the copy of `globals` into and out of every
evaluation is the cost issue #271 is about. A layout file that names `globals` fails to
load with a message that points at `store`.

**A Package can ship a Store.** Its identity comes from the Package's own module, so it
cannot collide with the importer's state.

**Component-local state is out of scope and undecided.** State that one component instance
keeps for itself (a `useLocal`) is not part of this decision.
