# Development guidelines

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
