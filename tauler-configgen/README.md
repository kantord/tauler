# tauler-configgen (prototype, not published)

Reads a two-document YAML schema — a flat list of node declarations, then a `minijinja`
template — and generates the readable `.jsx` component source a layout file would import
directly. Design record: `~/Downloads/tauler-config-files-design-record.md` §13/§14/§17/§18.

**`renderTemplate` is now wired into tauler's actual runtime** (`src/jsx.rs` in the main
`tauler` crate, §18.2) — generated code that calls it really runs. A real end-to-end test
(`the_real_generator_output_runs_end_to_end_against_the_real_reconciler`, in the main
crate's `src/units.rs`) parses and generates from the actual shipped
`examples/rofi-full-theme.schema.yaml`, runs it through the actual reconciler runtime,
and writes a correct `.rasi` file — not a hand-simulated approximation.

**Config-format-independent (design record §17).** There is no rasi-specific concept
anywhere in this crate's Rust code — no "selector," no `ValueType` enum, no `Padding`/
`Border` types. A node is just a component with declared props (each with a validation
`pattern`/`enum` and an optional human-written `error`), optional constant fields, and an
optional `children_as` name if it's a container. The *only* place any config format's own
syntax is known is a schema author's `pattern`/`error` strings and the template text —
both plain data, never Rust code.

## What's here

- `Identifier` — a name that can only exist validated; nothing can format an unchecked
  string into a JS identifier position (`src/identifier.rs`).
- `parse` — reads the two-document schema, validates document 1 against a real JSON
  Schema 2020-12 meta-schema (`schemas/config-schema.metaschema.json` — enforces exactly
  one root node and that every container names its `children_as` field), rejects
  duplicate `component` names (`src/schema.rs`).
- `generate` — emits the `.jsx` source: every node (leaf or container, root or not) goes
  through one function, `emit_node_component`, driven entirely by what the schema
  declares (`src/codegen.rs`). Props are always accessed via bracket notation
  (`props["name"]`), never dot notation. `PropRule.items`, when set, means the prop must
  be an array, every element checked against a *nested* rule (rofi's "list of keywords" —
  `children: [ "inputbar", "message", "listview" ];`, `man rofi-theme`) — the meta-schema
  supports this recursively via `$defs`/`$ref`.
- `render_template` — the Rust-side proof that a schema's template, run against data
  shaped like what the generated components would produce, yields correct output
  (`src/render.rs`).
- `examples/rofi.schema.yaml` — the original worked example (flat properties + Padding/
  Border shorthand). `examples/rofi-full-theme.schema.yaml` — every selector and property
  in the *real* user theme this design has been tested against throughout, including
  compound selectors (`element selected`) and the array-valued `children` property.

34 tests, all green in this crate (`cargo test -p tauler-configgen`); the real end-to-end
proof lives in the main `tauler` crate's test suite instead, since it needs a real
QuickJS engine this crate deliberately doesn't depend on.

## A real bug this design's own tests caught, twice

1. **A schema bug**, not a generator bug: the shipped full-theme schema's `Selector.name`
   pattern required starting with a letter, silently excluding rofi's own global-section
   selector, `*`. Every `<Selector name="*">` threw a validation error, silently caught
   and logged by the runtime's existing hook error-handling, producing `entered: 0` with
   no other symptom — found only by actually running the generated code end to end, not
   by any static check.
2. **A test-verification bug**, not a code bug: fixing an array-item error-label
   construction (which needed the runtime loop index, not just a static JSON literal)
   initially reopened the exact JS-injection class §13.3 exists to close. The fix was
   correct; the *test* verifying it produced a false positive — checked against a raw
   file dump of the actual bytes, which showed the generated code was safe all along.
   Recorded in §18.3 so the mistake in verification method, not the code, isn't repeated.

## What's NOT here yet

Remaining gaps against the real rofi theme, per `~/Downloads/rofi-rasi-format-research.md`:

- **`@variable` references** — deliberately **not** building support for this (design
  record §16.5). Redundant with `ConfigFile`'s full-regeneration-every-Sweep model plus an
  ordinary JS variable. The one non-redundant case (colors from an externally-managed
  `colors.rasi`) needs a loosened `pattern` plus literal `@import "..."` template text,
  not a new value type.
- **`configuration { }`** (rofi's behavior settings, not styling) — lowest priority; same
  node/prop model, different property vocabulary, nothing new needed structurally.
- **No CLI.** `generate`/`parse`/`render_template` are library functions only — nothing
  runs the generator and writes a `.jsx` file to disk as part of a normal workflow yet.
- **No schema-file watching / regeneration-on-change** (design record §13.3's fix,
  specified — extend `reconcile_import_watches`, branch in `handle_layout_reload` — but
  not implemented).
- **No lifecycle/reload (`apply`-equivalent) story for a *generated* `ConfigFile`** — the
  hook exists and works (already shipped), but nothing has designed how a schema author
  would expose that choice through the schema format itself.
- **`serde_yaml_ng`, not `saphyr`.** Fine for now; the design record's own security
  reasoning (pure Rust, no `unsafe-libyaml`) argues for `saphyr` before this handles
  schemas downloaded from strangers at scale.
- **Nothing has been deployed to a real, live rofi config.** The end-to-end proof runs
  against a temp file inside a Rust test — real runtime, zero risk, but deliberately
  short of actually replacing `~/.config/rofi/theme.rasi.tmpl`, which is a separate,
  larger decision (see design record §18.5).
- **No second grammar has actually been tried.** This crate is *shaped* to be
  config-format-independent, but per the "one adapter is a hypothetical seam, two is a
  real one" rule, that claim is still unverified against anything but rofi.
- **Array-of-non-string-values (`tab-stops`) untested.** Only the list-of-keywords case
  (bare strings) has been exercised; `items` supports any nested rule shape, but nothing
  has tried it with, say, nested distances.
