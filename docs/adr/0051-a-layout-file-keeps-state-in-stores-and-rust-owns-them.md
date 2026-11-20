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
  (The amendment below gives a component instance an identity, which only a local Store
  uses.)
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

> Amended [below](#amendment-a-store-declared-inside-a-function-is-local-to-a-component-instance):
> `const name = store(default)` directly inside a function no longer fails the load. It
> declares a local Store, whose identity also names the component instance.

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

## Amendment: a Store declared inside a function is local to a component instance

This supersedes the consequence above, "No person writes an identity" where it says a
`store(...)` call anywhere but the top level fails the load, and the line of ADR
[0007](0007-every-tick-re-renders-everything.md) this ADR repeated: "no component identity".
A component instance now has an identity, and only a local Store uses it. Every Tick
still rebuilds the whole tree.

There is still one function, `store`. Where it is declared decides who shares the value:
at the top level of a module it is a **shared Store**, as above; inside a function body it
is a **local Store**, and each **component instance** that reaches the declaration has its
own value. See [CONTEXT.md](../../CONTEXT.md) for the three terms.

```jsx
function toggle() {
  const s = store({ open: false });          // local
  return { open: s.open, flip: () => { s.open = !s.open; } };
}

function Menu() {
  const menu = toggle();
  const help = toggle();
  return <div on_click={menu.flip}>{menu.open ? "on" : "off"}</div>;
}

<Menu key="left" /> <Menu key="right" />     // four separate values
```

Rejected: a second function (`useLocal`), and a naming convention (`use…`) for a function
that declares a Store. The place of a declaration is the rule a JavaScript variable
already follows, and the compile pass finds `store(...)` by reading the code, so a name
would carry nothing. "Hook" stays the word for a Unit hook.

### The identity is names, never call order

A local Store's State identity has three parts, outermost first:

```text
<component instance>  >  <call names>  >  <declaration>

components/Sidebar.jsx#Sidebar > components/Menu.jsx#Menu[left] > menu > lib/toggle.jsx#toggle.s
```

- The component instance is the components running when this one was written, each as
  `<module>#<function>`, with its `key` when that is a string or a number. It is the chain
  of authors (the function whose code wrote the JSX), not of parents in the layout tree:
  a child element is evaluated before the element it is placed in, so the author is the
  only ancestor known while a component runs.
- A call name is the name of the `const` a function call is the whole value of.
- The declaration is `<module>#<function>.<const>`.

The compile pass writes every static part, and a stack kept while rendering supplies the
`key`s and the order. Nothing depends on the order of calls, so swapping two lines, or
putting a condition or a loop around a declaration, moves no value to another owner.

Rejected: a counter of calls per component instance, as React numbers its hooks. Two calls
swap values when their lines are swapped, and every later value shifts when one call
becomes conditional; "rules of hooks" exist to forbid that. Rejected: a function's `.name`
as a link. Two components of one name from two modules, written as alternatives, shared
one value with no error.

### Two rules

1. At load: a local `store(...)` must be the whole value of a `const`, in a function that
   has a name and is neither `async` nor a generator. The compile pass refuses anything
   else, with the file and the line.
2. While rendering: one identity is declared at most once per render. A second
   declaration throws, names the identity, and says to make the repeated part a component
   with a `key`. This one check covers two siblings with no `key`, a `.map()` with no
   `key`, the body of a loop, a function called twice with no `const` of its own, and a
   component called twice as a plain function.

Rejected: refusing loops and conditions at load. With named identities they are harmless
whenever the declaration is reached once.

**Which calls carry their `const`'s name.** The compile pass reads one file at a time and
names every `const x = f(...)` whose callee is bound in that file. A call with no `const`
carries no name: one such call of a function that declares a Store works, and a second
one in the same render throws (rule 2). A call from a handler throws when it runs. None
of these is refused when the layout loads. Rejected: reading the import graph to refuse both at
load. It is five to eight times the code, needs a second answer for a page, and still
cannot see a function that arrives as a value, so rule 2 would stay.

**A destructured call fails.** `const { open, flip } = toggle();` has no one name to find
the Store by, so a Store declared beneath it throws, at the top level of a module too. The
message says to assign the call to a plain `const` first and destructure that:
`const menu = toggle(); const { open, flip } = menu;`. A destructured call of a function
that declares no Store works as written. Rejected: naming the call after the names it
binds, which resets the Store when one bound name is added or renamed.

A component above a local Store must be a function the compile pass can name: one bound at
the top level of its file. A Store declared beneath any other throws. It is never shared
silently.

### What it survives

- **A local Store that a render does not declare is dropped at once.** A hidden component
  starts from its default when it comes back, as a React component does; state that must
  outlive hiding belongs in a shared Store. Rejected: keeping it until the next reload,
  which leaves an entry behind for every key a list ever had. Only a render of the render
  runtime that returned drops anything: a render that throws, a failed reload, and the
  reconciler and Draw runtimes drop nothing. A handler that outlives its render (a drag in
  progress) and writes a local Store that was dropped since writes nothing: the write is
  ignored, and the handler's other writes are kept.
- A layout reload keeps a local Store whose identity is unchanged and which the first
  render after the reload declares. A reload empties every Stream, so a component that is
  only written once a Stream has a value starts over. That is the rule above applied to
  that render; it is recorded as a risk in `src/app.rs`.
- Renaming a component, a function that declares a Store, a `const` or a file resets what
  is named after it and nothing else. So does writing a component under another author.
  A render prop is called by the component that receives it, so that component is the
  author of what the render prop writes.

### A function that declares a Store, called at the top level of a module

`const menu = toggle();` outside any function declares a shared Store, named after the
module and the `const`. No component is running, so the identity is only names. It is the
one way to reuse the logic around a shared Store, and it follows from the identity rule.
A destructured call there fails the load, as it throws in a component.

### Cost

One local Store costs 1.5 to 3 µs per Tick, by how deep it is. A layout with no local
Store pays for the stack: about 3 to 5% per evaluation in the benches, and the cause of
most of it was not found. The budget was 5% of an evaluation for a layout with none, and
twice an unread shared Store for each local one; `tauler-core/src/local_store.rs` holds
the measurements against it.
