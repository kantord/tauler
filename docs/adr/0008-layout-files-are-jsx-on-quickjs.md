# Layout files are JSX evaluated on QuickJS

A layout file is a `.jsx` file. The JSX transform is reached through `optative-script`
(which wraps OXC), and the result is evaluated by QuickJS via `rquickjs`. There is no
custom parser, no template language, and no preprocessor of our own.

## Why a real language

The alternative was a declarative config format — TOML, YAML, a bespoke DSL. Every one of
them arrives at the same place eventually: a bar wants conditionals ("hide this when the
battery is full"), then loops ("one chip per workspace"), then reuse ("this card, four
times"). Adding those to a config format means growing a language badly. Starting with a
language means never having that conversation.

JSX specifically, because the tree it describes *is* the output. A layout is a tree of
boxes; JSX is the least ceremonious tree syntax with editor support already everywhere.

## Why QuickJS

It embeds as a vendored C library through `rquickjs-sys` — no system dependency to install,
no runtime to ship alongside the binary. A layout evaluation costs 100–200μs, which is
noise against rasterization. The cost is a C compiler at build time.

## Why the sandbox is deny-by-default

`rquickjs` grants nothing on its own: a layout file has no filesystem, no network, and no
process access unless Rust registers it. tauler registers exactly `useStringStream`,
`useJSONStream`, `useEvents`, `Module`, `globals` and `ctx`.

That is not primarily about untrusted layouts — a layout file is written by the person
running the bar. It is about the boundary being explicit. Every capability a layout has is
one line of Rust somewhere, and the list above is exhaustive by construction rather than by
audit. A layout that wants to read a file spawns a subprocess that reads it, which puts the
access where it can be seen.

## Consequences

Layout files are ES modules, and the module's **default export is the render function**. A
file that ends in a bare expression does not load — it fails with a type error about
`undefined` not being a function, which is not an obvious message to debug from.

The transform runs once per layout-file change, not per tick; the QuickJS runtime and
context are created once and reused forever. Both facts are what keep a tick cheap enough
for [0007](0007-every-tick-re-renders-everything.md) to be reasonable.

tauler does not depend on `oxc_transformer` directly. It arrives through
`optative-script`, so OXC version questions belong to that crate.
