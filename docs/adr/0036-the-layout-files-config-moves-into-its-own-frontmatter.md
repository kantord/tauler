# The layout file's config moves into its own frontmatter

`theme` and `fonts` move out of a sibling `config.yaml` and into a YAML frontmatter block
at the top of the layout file itself, which becomes `layout.op.mdx`. One file now declares
both what a bar is drawn with and what it is. `config.yaml` is not deleted: it keeps
working as a legacy path alongside the old `layout.jsx`, and `tauler-e2e/fixtures/showcase`
is deliberately kept on it so the legacy path stays under screenshot-test coverage.

## Why frontmatter, not a plain JS export

The alternative that needed no new convention at all was a single `.jsx`/`.tsx` file
exporting `const config = {...}` alongside the existing `export default render`. It was
rejected in favor of frontmatter, on the same reasoning [0008](0008-layout-files-are-jsx-on-quickjs.md)
already used against declarative config formats, read the other way: `theme.mode` and
`fonts.primary` are data, not a tree that grows conditionals and loops the way a layout
does. 0008's argument was about the *layout*, which stays JSX; the config half was never
the part doing that growing, in `config.yaml` or anywhere else. Frontmatter names that
boundary in the file itself instead of leaving it implicit across two files.

## Why `.op.mdx` doesn't mean real mdx lowering

`optative-script-mdx` (the crate behind esto's own `.op.mdx` documents) turned out to be
the wrong tool here, for a reason only surfaced by testing it against a real layout file
rather than a synthetic one. Its lowering *synthesizes* the module's `export default`
from whatever flow JSX it finds — the crate's own docs say outright that "the author
never writes the default export themselves." Tauler's layout files do the opposite: they
declare `export default function render() { ...local consts, useJSONStream() calls...;
return <tree>; }` themselves, per [0008](0008-layout-files-are-jsx-on-quickjs.md) and
[0007](0007-every-tick-re-renders-everything.md) (a plain function, called fresh every
tick — deliberately not a reconciler/diffing model, for the same performance reasoning
0007 gives). Feeding a real layout file through `lower_to_tsx_with_frontmatter` produces
two competing `export default`s — the author's, plus the crate's synthesized empty one —
which is simply a syntax error, not a subtle bug.

This is not a gap in optative-script-mdx: it was built for esto's prompt-authoring
documents, which have no render function and no concept of "re-render this tree from
live data" at all — reconciliation there is about converging external side effects
([0033](0033-reconcilers-are-esto-units.md)), not producing a tree. Making tauler's
render side fit `.op.mdx`'s model would mean reverting 0007, not adopting a library
feature — a full rearchitecture for an unrelated, still-valid decision, not something
this change does.

So `layout.op.mdx`'s frontmatter is extracted by tauler itself, with a plain text search
(`layout_source::split_frontmatter`): a line that is exactly `---`, then arbitrary YAML
lines, then another line that is exactly `---`. Everything after the closing fence is the
JS/JSX body, passed to `JsxEvaluator` **byte-for-byte unchanged** — the same
`export default function render() {...}` shape a layout file has always used. No markdown
parsing, no lowering, no dependency on `optative-script-mdx` at all.

The `.op.mdx` extension is kept anyway, for two reasons: it still reads as "an optative
script, markdown-flavored" to a human, and it stays open to a real future feature —
opt-in markdown-as-content via a runtime component (something like `<Markdown
source={...} />` turning a string into HTML nodes at render time) is a plausible later
addition, and it would be fully decoupled from this file-format question: no lowering
step would need reopening to add it, since it would operate on a string value at
evaluation time, not on the file itself.

One artifact of this reversal: `optative-script-mdx` 0.0.5 already ships
`lower_to_tsx_with_frontmatter`, built and published before this dead end was found. It
is not being reverted upstream — it is a legitimate, tested feature for esto's own
prose-document use case, which has no render function to collide with. Tauler simply
never calls it.

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
silent: today it draws a blank bar with no log line at all. An opened-but-never-closed
`---` fence is its own distinct failure (`LayoutLoadError::Frontmatter`) rather than
silently reading as "no frontmatter" — a typo'd closing fence would otherwise swallow the
whole config block into the JS body and fail as a confusing JS syntax error instead.

`TaulerConfig::from_yaml` — already used for `config.yaml` today — is reused unchanged for
the frontmatter text; one config schema, two source locations, no duplication.
`src/layout_source.rs` is where both formats converge into one `(TaulerConfig, String)`
pair the rest of `app.rs`/`main.rs` treats identically regardless of which format
produced it.
