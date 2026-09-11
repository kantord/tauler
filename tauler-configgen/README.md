# tauler-configgen (prototype, not published)

Reads a two-document YAML schema — structure/validation, then a `minijinja` template — and
generates the readable `.jsx` component source a layout file would import directly. This
is the "for starters" slice of the design worked out in
`~/Downloads/tauler-config-files-design-record.md` §13/§14: real code, real tests, proven
against the actual injection exploit the design's skeptic rounds found — but **not yet
wired into tauler's own QuickJS runtime.**

## What's here

- `Identifier` — a name that can only exist validated; nothing can format an unchecked
  string into a JS identifier position (`src/identifier.rs`).
- `parse` — reads the two-document schema, validates document 1 against a real JSON
  Schema 2020-12 meta-schema (`schemas/rasi-grammar.metaschema.json`), rejects duplicate
  `component` names (`src/schema.rs`).
- `generate` — emits the `.jsx` source: validated leaf/selector components returning plain
  data, a root component embedding the schema's own template as a JSON-escaped constant
  and calling `renderTemplate` (`src/codegen.rs`).
- `render_template` — the Rust-side proof that a schema's template, run against data
  shaped like what the generated components would produce, yields correct output
  (`src/render.rs`).
- `examples/rofi.schema.yaml` — the worked example, exercised end-to-end by
  `integration_tests` in `src/lib.rs`.

21 tests, all green. `cargo test -p tauler-configgen`.

## What's NOT here yet

- No `renderTemplate` builtin registered in tauler's actual QuickJS runtime — generated
  code calls a function that does not exist at runtime today. Wiring that in is
  `src/jsx.rs` work in the main `tauler` crate, not this crate.
- No CLI binary — `generate`/`parse`/`render_template` are library functions only.
- No schema-file watching / regeneration-on-change (design record §13.3's fix, specified
  but not implemented).
- No lifecycle/reload (`apply`-equivalent) story for generated `ConfigFile`s — flagged in
  conversation, not designed yet.
- `serde_yaml_ng` is used for simplicity; the design record's own security analysis
  recommends `saphyr`/`serde-saphyr` (pure Rust, no `unsafe-libyaml`) before this handles
  schemas downloaded from strangers at scale.
