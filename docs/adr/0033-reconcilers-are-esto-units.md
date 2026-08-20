# Reconcilers are esto units, not a tauler API

A layout file makes something reconcilable by calling `unit()` from `optative-script` — the
same call `esto` scripts already use — and rendering the component it returns:

```jsx
const DeskLight = unit({
  key:    (i) => i.entity,
  value:  (i) => `${i.state}:${i.brightness}`,
  reconciler: optativeSet({ observe: () => [...] }),
  enter:  (i) => sh`...`,
  update: (i) => sh`...`,
  exit:   (i) => sh`...`,
})

<DeskLight entity="light.tradfri_bulb_4" state="on" brightness={180} />
```

A Unit is a call, not a node. Only its Items are nodes, and they are Shell nodes: they sit
under `<root>` beside `<panel>` and `<wallpaper>`, and they describe an effect rather than
structure or content.

tauler does not define a reconciler API of its own.

## Why

The obvious alternative was a tauler-native node — `<resource key enter update exit/>` —
with the hooks written as strings or as functions returning strings. Four rounds of design
walked into the same wall from different directions, and each time the fix was something
`unit()` already has.

**The value has to be per Item, not per Unit.** Diffing the rendered `enter` string looked
workable until the four `sync_*_with_kitty_theme.sh` scripts: their command line is
constant, so nothing would ever re-run. Passing a separate value is not a refinement of
that design, it is `unit()`'s `value` hook.

**The hooks have to be parametric.** `key`, `value` and every lifecycle hook are functions
of the Item's props. Once that is true, the thing being declared is a template, and a
template is not a node.

**Reading the world is not optional.** `meta.op`'s `observe()` walks the GitHub API with a
memoised client and shares the walk with the JSX descent. Any design that reduces observing
to "a command that prints lines" is a different, smaller feature.

**It already exists, and it was extracted from here.** `optative` says so in its README:
the pattern came out of tauler. `optative-script` is already a tauler dependency.
Re-inventing the vocabulary one crate above the crate that holds it would leave two
dialects of the same idea in one author's repos, with `.op.tsx` files unable to move
between them.

**Why not depend on the `esto` crate.** `unit`, `optativeSet` and `sh` live in
`optative-esto` today, which is a CLI: clap, glob, notify, a `[[bin]]`. Those builtins move
down into `optative-script`, which both embedders already depend on, and each keeps its own
driver — esto's is one-shot, tauler's is long-running. The vocabulary is shared; the loop is
not.

## Consequences

**A new kind of reconcilable thing needs no tauler change.** It needs a `unit()` call in a
layout file. That is the whole extension point, and it is the same bargain tauler already
offers for data: a Stream is any program that prints lines, and a Unit is any four functions
that agree on a key.

**Batch is the primitive; per-item is sugar.** A hook is handed the whole array of Items it
should act on and may act on some of them; the rest are offered again on the next Sweep.
A `unit()` that defines a per-item hook is wrapped automatically, so every existing
`.op.tsx` keeps working. This is a change to `optative`, not to tauler — until it lands,
tauler simulates it by looping, which is wasteful and invisible from JS.

Batching is also how a reconciler rate-limits itself: a hook given ten Items that does three
*is* the rate limit. No decorator, no shared pool, no context to thread.

**Higher-level components are components.** A `<KittyTheme>` or a `<DeskLight>` is a
function returning Items, exactly as `<I3Layout>` is a shim over Panel declarations
([0003](0003-i3layout-is-a-js-shim-over-a-rust-component.md)). There is no second authoring
layer to design, and no plugin boundary to cross.

**Config files are not this.** Writing a config file is a Unit like any other, but a Unit
whose value is a whole rendered file wants a serialiser, and that is a separate feature with
its own decisions. Nothing here is specialised for it.

**The glossary moved to make room.** `CONTEXT.md`'s `Units` section — logical and physical
pixels — is now `Measurement`, because Unit means this.
