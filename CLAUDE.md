# Development guidelines

## Documents

- `CONTEXT.md` — the glossary. Read it before naming anything; use its terms and avoid the
  ones it lists under `_Avoid_`. Glossary only, no implementation detail.
- `docs/adr/` — decisions that are hard to reverse and surprising without context. Check
  here before "fixing" something that looks wrong.
- `spec.md` — how it actually works. Implementation detail lives here, not in `CONTEXT.md`.

`CONTEXT.md` and `docs/adr/` are maintained by the `domain-modeling` skill and are
repo-internal — neither is published to the docs site.

## TDD

Use "pragmatically strict" TDD: write tests for real behavior, skip tests for pure plumbing (struct fields, wiring, pass-through changes).

Keep cycles small — one behavior per cycle. If a step feels large, break it down first.

Use subagents for each stage:
- **Red**: subagent writes the failing test only
- **Green**: subagent writes minimal implementation to pass it
- **Refactor**: subagent if needed

## Deployment

- Rust code changes: `cargo build --release` — tauler watches its own binary and re-execs automatically
- `tauler-i3` is a separate binary; build with `cargo build --release -p tauler-i3` and replace atomically: `cp target/release/tauler-i3 ~/.cargo/bin/tauler-i3.new && mv ~/.cargo/bin/tauler-i3.new ~/.cargo/bin/tauler-i3`
- Config-only changes: `chezmoi apply ~/.config/tauler/config.yaml` — tauler hot-reloads it within 500ms, no restart needed
- `bar_width` or `outer_gap` changes trigger a full re-exec automatically
