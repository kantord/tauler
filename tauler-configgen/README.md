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

## What's actually deployed

As of the rofi/kitty config-ownership work: `rofi-full-theme.schema.yaml` (theme
selectors/properties), `rofi-config.schema.yaml` (rofi's `configuration{}` block —
behavior, not styling), and `kitty-config.schema.yaml` (kitty's static settings) are all
generated, deployed via chezmoi, and running against the real, live desktop —
`~/.config/rofi/{theme,config}.rasi` and `~/.config/kitty/tauler-kitty-settings.conf` are
tauler-`ConfigFile`-owned, not hand-written. `@variable` references were retired for real
(not just designed away): colors flow as literal hex from
`~/.config/tauler/rofi-colors.json`, read fresh every render() — no `@import`, no
`colors.rasi`. Two hand-written-template-literal files remain unmigrated —
`RecentFilesTheme.jsx` (rofi's fullscreen recent-files picker: `fullscreen`,
`orientation`, `flow`, `calc()`, `@media` conditionals — constructs this crate's
node/prop model doesn't yet have a story for) and none else; every other flat,
non-selector settings file (kitty, rofi's `configuration{}`) now goes through the schema,
closing what used to be an unprincipled inconsistency between which files got validation
and which didn't.

Second grammar tried for real: `kitty-config.schema.yaml` is a genuinely different shape
(flat `directive value` lines, no selectors/nesting at all) from rofi's selector/property
model, and it generalized cleanly with zero rasi-specific Rust code touched — the "one
adapter is a hypothetical seam, two is a real one" bar is now met.

## What's NOT here yet

- **`RecentFilesTheme.jsx` stays a hand-written template literal.** `@media` conditional
  blocks aren't expressible in the current node/prop/template model at all (the template
  only ever iterates over collected children — there's no mechanism for a literal,
  schema-independent passthrough block). Extending the schema for this is a real design
  question, not yet answered, not attempted under time pressure.
- **No CLI.** `generate`/`parse`/`render_template` are library functions only — nothing
  runs the generator and writes a `.jsx` file to disk as part of a normal workflow yet.
  Every deployed generated file in this project was produced by manually running a
  `cargo run --example verify_*` and copying its output into the chezmoi source by hand
  — 4 manual steps across 2 repos (edit the `.yaml`, run the example, `cp` the output,
  `chezmoi apply` it), still real debt. One step of what used to be 5 is no longer manual:
  `annotate_with_source_path` makes each `verify_*.rs` stamp its own "Source schema: ..."
  line into the output itself, so that line no longer needs hand-typing after every `cp`
  (found and fixed in review — a schema-format-agnostic generator has no way to know its
  own file path, so something has to be told it once).
- **No schema-file watching / regeneration-on-change** (design record §13.3's fix,
  specified — extend `reconcile_import_watches`, branch in `handle_layout_reload` — but
  not implemented). Editing a `.yaml` schema does not regenerate its `.jsx` component;
  the manual step above is still required every time.
- **No lifecycle/reload (`apply`-equivalent) story for a *generated* `ConfigFile`** — the
  hook exists and works (already shipped, exercised by hand-written `ConfigFile`s), but
  no generated schema so far has needed it, so nothing has designed how a schema author
  would expose that choice through the schema format itself.
- **`serde_yaml_ng`, not `saphyr`.** Fine for now; the design record's own security
  reasoning (pure Rust, no `unsafe-libyaml`) argues for `saphyr` before this handles
  schemas downloaded from strangers at scale.
- **Array-of-non-string-values (`tab-stops`) untested.** Only the list-of-keywords case
  (bare strings) has been exercised; `items` supports any nested rule shape, but nothing
  has tried it with, say, nested distances.
