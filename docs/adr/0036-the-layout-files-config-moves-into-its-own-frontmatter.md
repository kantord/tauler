# The layout file's config moves into its own frontmatter

`theme` and `fonts` move out of a sibling `config.yaml` and into a YAML frontmatter block
at the top of the layout file itself, which becomes `layout.op.mdx`. One file now declares
both what a bar is drawn with and what it is. `config.yaml` is not deleted: it keeps
working as a legacy path alongside the old `layout.jsx`, and `tauler-e2e/fixtures/showcase`
is deliberately kept on it so the legacy path stays under screenshot-test coverage.

## Why frontmatter, not a plain JS export

The alternative that needed no upstream work at all was a single `.jsx`/`.tsx` file
exporting `const config = {...}` alongside the existing `export default render`. It was
rejected in favor of real `.op.mdx` frontmatter, even though frontmatter requires a new
feature in `optative-script-mdx` and the plain-JS route requires nothing upstream.

This is a partial retreat from [0008](0008-layout-files-are-jsx-on-quickjs.md), which
argued against declarative config formats in favor of a real language, for exactly the
reason 0008 gives: `theme.mode` and `fonts.primary` are data, not a tree that grows
conditionals and loops the way a layout does. 0008's argument was about the *layout*,
which stays JSX; the config half was never the part doing that growing, in `config.yaml`
or anywhere else. Frontmatter names that boundary in the file itself instead of leaving it
implicit across two files.

## Why `.op.mdx`, not a new bare `.mdx`

`optative-script-mdx` already dispatches its markdown lowering on a `.op.mdx` suffix
(`MDX_EXTENSION` in that crate's `lib.rs`); a bare `.mdx` would need a second upstream
change purely to teach it a new extension, for no behavioral gain. Reusing `.op.mdx` means
the only upstream work is the frontmatter feature itself.

The JSX-body lowering underneath needs no change either way: a layout file has no
headings or prose, so `lower_to_tsx`'s `compile_root` already takes the array-of-elements
fallback path that a heading-free document gets today.

## Why coexist instead of a hard cutover

Tauler is a public repo with tagged releases; other dotfiles may point at `layout.jsx`
and `config.yaml`. `layout.op.mdx` is tried first at startup; its absence falls back to
the `layout.jsx` + `config.yaml` pair. Which format is active is decided once, at startup
— a live reload only re-reads whichever format was chosen at boot, so switching formats
means restarting tauler rather than teaching the reload path to flip shapes under itself.

`tauler-e2e/fixtures/showcase` stays on the split format on purpose, not because it was
skipped: it is the fixture most likely to be copied from (see
[0019](0019-a-rice-fixture-is-a-dotfiles-repo.md)), so keeping it on the legacy path is
what gives the legacy path a screenshot regression test at all. Every other fixture
(`monolith`, `signal`, `thermal`, `sidebar`, `three-edge`) moves to `layout.op.mdx`.

0019 separately named converting `showcase` off the flat shape as "the obvious follow-up,"
for an unrelated reason (matching the other fixtures' `home/`-tree shape). This decision
does not do that conversion either — `showcase` now has two independent reasons to stay
as it is.

## Consequences

An invalid config now fails the same way regardless of which format wrote it:
[0033](0033-an-unusable-theme-file-stops-startup-not-a-reload.md)'s rule — exit at
startup, report-and-keep-last-good on reload — extends from `theme.file` to the whole
config. This closes a standing bug where `config.yaml`'s own parse failure was swallowed
silently (`load_font_config` in `main.rs`, via an `.ok()` chain) and fonts fell back to
defaults with no warning at all. A missing layout file in *either* format also stops being
silent: today it draws a blank bar with no log line at all.

`optative-script-mdx` gains a new function returning `(Option<String>, String)` — the raw
frontmatter YAML text, unparsed, alongside the lowered JS source — leaving `lower_to_tsx`
and the `run_script*` family untouched, so esto's own `.op.mdx` prose-document use is
unaffected. Tauler already bypasses `run_script*` for its layout (`jsx.rs` calls
`optative_script::jsx::transform_source` directly); it calls the new function the same
way, and parses the returned YAML with the `TaulerConfig::from_yaml` that already exists
for `config.yaml` today — one config schema, two source locations, no duplication.
