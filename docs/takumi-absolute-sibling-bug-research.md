# takumi: multiple `position: absolute` siblings blank the parent subtree — research notes

Research date: 2026-07-25. Scope: primary-source investigation only (vendored crate source,
`kane50613/takumi` GitHub issue tracker, `DioxusLabs/taffy` GitHub issue tracker, crates.io
version history, upstream CHANGELOG.md files). No blog posts or secondary sources were used.

## TL;DR

- **Known upstream, exact match: no.** No issue in `kane50613/takumi` or `DioxusLabs/taffy`
  describes this exact symptom (2+ direct `position: absolute` siblings under one parent →
  the *entire* parent subtree, including non-absolute siblings, fails to paint with no error).
- **Closely related, known and fixed family: yes, but it doesn't fully match.** There is a
  well-documented, actively-patched bug family in takumi about out-of-flow (`position: absolute`)
  siblings interacting badly with takumi's "does this container need an inline formatting
  context?" classifier — issues
  [#695](https://github.com/kane50613/takumi/issues/695),
  [#711](https://github.com/kane50613/takumi/issues/711),
  [#738](https://github.com/kane50613/takumi/issues/738), and a 2026-07-20 regression,
  [#992](https://github.com/kane50613/takumi/issues/992). Every one of these, however, is
  triggered by a **whitespace/text node** sitting between the positioned siblings (e.g.
  pretty-printed HTML input), and their symptom is narrower: the absolute children vanish but
  the parent's own background/border and other in-flow siblings still render. Our vendored
  1.8.7 already contains the code-level fixes for #711 and #738 (see citations below), and pure
  JSX with **no** text-node children does not hit the code path those fixes patch, per full trace.
- **Newer version that mentions a fix: not found for this exact bug.** The vendored `takumi`
  1.8.7 is the *last* pre-v2 release (2026-06-20); `takumi` 2.0.0-rc.1 shipped one week later
  (2026-06-27) as a ground-up split into `takumi-core`/`takumi-raster`/`takumi-svg`/`takumi-html`/
  etc. Current latest is `takumi@2.5.0` / `takumi-core@0.9.0` (2026-07-25). Scanning every
  `takumi`/`takumi-core` CHANGELOG entry between the 1.8.7-era code and today, nothing describes
  a fix matching "subtree not rendering", "stacking context", "z-index bucket", or "multiple
  positioned siblings" beyond the whitespace-sibling family above (already present pre-1.8.7)
  and its 2.4.0 regression fix (also whitespace/HTML-specific, not JSX).
- **Root cause: not conclusively pinned down after full tracing.** I fully read
  `takumi-1.8.7/src/rendering/stacking_context.rs` (1271 lines) and the two most relevant
  subsystems of `takumi-1.8.7/src/layout/tree.rs` (the out-of-flow "hoisting" algorithm in
  `push_layout_node`, and the inline-formatting-context / anonymous-box classification in
  `RenderNode::from_node_iterative`/`should_create_inline_layout`) end to end. Neither shows an
  ID-collision, `HashMap` overwrite, or single-slot-per-parent assumption that would explain the
  reported symptom for a **pure, text-free** 2-sibling case. My best-supported hypothesis,
  detailed below, is that this is an **unfiled edge case in the same "out-of-flow siblings
  vs. container classification" bug family**, specific to a parent whose *entire* in-flow content
  is absolutely positioned (so it retains zero taffy children) while itself living at a non-root
  depth (so its absolute children are actually hoisted, unlike the shipped fixture tests, see
  below). I could not fully trace `tree.rs` end to end (~1700 of its 2435 lines were not read in
  this session — the `LayoutPartialTree`/taffy trait glue and absolute-measurement code lay
  outside the two subsystems above) so a bug in code I didn't reach remains possible.

## Bug description (context only — already confirmed live, not re-verified here)

When a parent JSX element has two or more direct children with
`style={{position: 'absolute', ...}}`, takumi's renderer fails to paint the entire subtree rooted
at that parent — nothing renders, not even a single previously-working absolute child, not even
non-absolute siblings. No panic, no error, even with `RUST_LOG=debug`. A single absolute child
(with or without its own children) renders fine. Wrapping each absolute element in its own
non-positioned intermediate `<container>` (so no two `position: absolute` nodes are ever direct
siblings of the same parent) fixes it completely with an identical visual result.

## Upstream repos

Confirmed by reading the vendored `Cargo.toml` files directly (not guessed):

- `takumi` (main crate): `repository = "https://github.com/kane50613/takumi"` —
  `/home/kantord/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/takumi-1.8.7/Cargo.toml`
- `takumi-css` (internal CSS layer, "Not a public API"): also
  `repository = "https://github.com/kane50613/takumi"` — same GitHub repo, monorepo layout —
  `/home/kantord/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/takumi-css-0.1.2/Cargo.toml`
- `taffy` (layout engine dependency): `takumi-1.8.7/Cargo.toml` pins
  `taffy = "0.11"` with features `flexbox, grid, alloc, calc, taffy_tree, strict_provenance,
  block_layout, float_layout`. `taffy`'s repository is `github.com/DioxusLabs/taffy` (well-known,
  and used directly via `gh search issues --repo DioxusLabs/taffy` successfully in this session,
  confirming the repo exists and is reachable under that name).

Note on repo topology change: as of `takumi` 2.0.0, the project split from a single `takumi` +
`takumi-css` pair into a multi-crate workspace still hosted in the same `kane50613/takumi` GitHub
repo: `takumi-core`, `takumi-raster`, `takumi-svg`, `takumi-html`, `takumi-js`, `takumi-napi`,
`takumi-wasm`, and a thin `takumi` umbrella crate (46 lines of Rust as of 2.5.0 — a pure
re-export facade). All GitHub issue links below are against the same `kane50613/takumi` repo.

## Issue tracker search results

Searched via `gh search issues --repo kane50613/takumi <query>` and `gh issue view` for full
bodies/comments. Queries tried: `absolute`, `position`, `blank`, `sibling`, `not rendering`,
`disappear`, `vanish`, `invisible`, `multiple absolute`, `two absolute`, `stacking context`,
`subtree`, `hoist`. No exact match for "N absolute siblings blank the whole parent subtree with
no text nodes involved." The closest family, all confirmed read in full:

- **[#695](https://github.com/kane50613/takumi/issues/695)** — closed 2026-05-19 —
  "`fromHtml` preserves whitespace-only text nodes, producing spurious grid rows". Whitespace
  text node between `display: grid` siblings became a spurious grid row. Fixed on the Rust side
  per the maintainer's own comment.
- **[#711](https://github.com/kane50613/takumi/issues/711)** — closed 2026-05-22 —
  "Whitespace text nodes between siblings break absolutely-positioned children". Explicitly the
  same shape as our bug but requires a whitespace text-node sibling; symptom is narrower (only
  the absolute children vanish, the block container itself still paints). Filed against a
  `position: relative` block container with one absolute child and one in-flow sibling.
- **[#738](https://github.com/kane50613/takumi/issues/738)** — closed 2026-06-01 —
  "\"position: relative\" is incorrect in certain situations" — an absolutely-positioned
  flex-item child of a `display: block; position: relative` parent, alongside a non-absolute
  sibling, mispositions; root-caused to out-of-flow boxes wrongly getting swept into an anonymous
  block box.
- **[#992](https://github.com/kane50613/takumi/issues/992)** — closed 2026-07-20, fixed in
  `2.4.0` per maintainer comment — "Whitespace text nodes between absolutely-positioned siblings
  drop the siblings again in 2.3.x (HTML input; #711 regressed)". A **regression** of #711 that
  reappeared in the post-v2 (`takumi-js` 2.3.x) codebase, HTML-input path only ("JSX input...is
  unaffected — only the HTML path" per the reporter). Symptom: "a bare dark card, BOTH
  absolutely-positioned children gone" — again, the *parent* still rendered; only the two
  absolute children were missing. This is the closest symptom match in count-of-siblings (2) but
  still requires whitespace text nodes and does not blank the parent itself.

No hits for `DioxusLabs/taffy` on any of: `multiple absolute`, `absolute siblings`,
`two absolute`, `not rendered`, `childless`, `only absolutely positioned`. `overwrite` and
`collapse` returned only unrelated issues (RTL support, visibility:collapse flex step, Display
Block/Grid tracking issues, a `content_size` border-inclusion bug, a closed margin-collapsing
issue for root children).

## Changelog / version findings

- `takumi` crates.io version history (`https://crates.io/crates/takumi/versions`, confirmed via
  the crates.io API): 1.8.7 was published 2026-06-20T16:29:59Z and is the **last 1.x release**.
  `2.0.0-rc.1` followed 2026-06-27T20:57:38Z, i.e. one week later — the v2 rewrite superseded 1.x
  entirely; there was no 1.8.8/1.9.0. Current latest: `takumi@2.5.0` (2026-07-25).
- Fetched `takumi/CHANGELOG.md` and `takumi-core/CHANGELOG.md` in full from the GitHub repo
  (`gh api repos/kane50613/takumi/contents/<path>`). Read every entry from `takumi-core@0.2.0`
  through `takumi-core@0.9.0` and every `takumi@2.x`/`2.0.0-rc.*` entry. The only entry
  touching this bug family:
  - **`takumi-core@0.6.3`**: *"Drop whitespace between absolute-only block siblings — When
    every element child of a block container was absolutely positioned, the whitespace text
    nodes from pretty-printed HTML formed an inline formatting context that swallowed the
    out-of-flow boxes, so none of them rendered. The whitespace drop now also runs when the only
    in-flow content is whitespace, keeping the absolute children in the layout."* This is a
    **v2-era** (post-split) instance of essentially the same #711/#695 family, evidently
    reintroduced or newly discovered during the v2 rewrite and independently fixed there. Still
    whitespace-triggered, not pure-JSX.
  - No entry in either changelog mentions "stacking context", "subtree", "paint order", "z-index
    bucket", or "positioned siblings" outside this whitespace family.
- Nothing in `takumi/CHANGELOG.md`'s 2.x entries (2.0.0 through 2.5.0, all read) references this
  bug family at all — the umbrella crate's changelog is dominated by font/animation/buffer-pool
  entries unrelated to positioning.

**Conclusion on versions:** there is no newer takumi version whose changelog claims to fix the
exact reported bug. The only closely-related fix (`takumi-core@0.6.3`) requires a whitespace text
node and is already a different (v2, post-split) codebase from the vendored 1.8.7.

## Root cause analysis

### What was ruled out by full trace

**`takumi-1.8.7/src/rendering/stacking_context.rs` (all 1271 lines, read in full).** No
single-slot-per-parent assumption exists anywhere in this file:

- `PaintItemKind::Context(usize)` ids are assigned via `contexts.len()` at push time
  (`stacking_context.rs:422`) — a monotonically increasing global counter across the whole `Vec`,
  not scoped per parent, so two sibling contexts created back-to-back cannot collide.
- Per-context children live in `StackingBuckets { negative: Vec, auto_zero: Vec, positive: Vec }`
  (`stacking_context.rs:239-274`) — plain `Vec`s pushed to via `StackingBuckets::push`
  (`stacking_context.rs:247-253`), not keyed by any `HashMap`/`BTreeMap` that could overwrite an
  entry for a second sibling.
- `merge_bounds` (`stacking_context.rs:667-681`) and its own unit test
  `merge_bounds_ignores_empty_bounds` (`stacking_context.rs:1208-1233`) explicitly verify that an
  empty/zero-size bounds from one child does not "poison" or replace a real sibling's bounds
  during the bottom-up bounds merge (`stacking_context.rs:483-498`) — ruling out a naive
  "zero-size node zeroes the whole context" theory.
- The only two `HashMap`s in the file (`node_transforms`, `node_content_box`,
  `stacking_context.rs:351-352`) are keyed by `NodeId` and populated once per visited node in
  strict pre-order (root visited first via the initial `visits` stack seed,
  `stacking_context.rs:353-361`), so a hoisted child's `hoisted_cb` lookup
  (`stacking_context.rs:460-466`) always finds its CB's transform already memoized by the time
  it's needed, regardless of how many siblings share that CB.

**`takumi-1.8.7/src/layout/tree.rs`, `push_layout_node` (lines 300-432, the out-of-flow
"hoisting" algorithm) — read in full.** This is the code that re-parents `position: absolute`/
`fixed` nodes in the taffy tree onto their CSS containing block while preserving DOM order in a
separate `box_children` list for painting. Traced by hand for the exact "parent P, two direct
`position: absolute` children A and B, P itself not positioned" scenario:

- `cb_stack: Vec<NodeId>` push/pop (`tree.rs:379-380`, `tree.rs:399-401`) is a strictly-nested
  LIFO tied to DFS descent/finish, so it cannot desync between two siblings — A's push/pop
  fully completes before B is even visited (this is a `while let Some(current) = stack.last_mut()`
  iterative DFS, not concurrent).
- `hoisted: HashMap<NodeId, Vec<NodeId>>` (`tree.rs:366`) is keyed by CB node id and stores a
  `Vec`, appended via `.or_default().push(fid)` (`tree.rs:415`) — both A and B correctly
  accumulate under the same CB key without overwriting each other.
- `parent.box_children.push(OrderedChild { render_index, node_id: fid, hoisted_cb })`
  (`tree.rs:423-427`) runs once per child regardless of hoisting, so P's paint-order list
  (consumed by `collect_layout_children` in `stacking_context.rs:161-166`) correctly contains
  both A and B with distinct `render_index` values (0 and 1) matching their position in the
  original `RenderNode.children` slice.

I could not find a bug in this code for the reported topology.

**`should_create_inline_layout` / anonymous-box classification (`tree.rs:1158-1171`,
`tree.rs:1396-1469`) — the mechanism actually responsible for the whitespace-sibling family
above.** `should_create_inline_layout` returns true for a `Display::Block`/`InlineBlock`
container only if **at least one** child satisfies `participates_in_inflow_inline_formatting_context`
(`is_inline_level() || is_inline_atomic_container() || anonymous_text_content.is_some()`,
`tree.rs:1142-1146`) **and every** child satisfies the broader
`participates_in_inline_formatting_context` (adds `is_out_of_flow() || float != None`,
`tree.rs:1148-1152`). A whitespace/anonymous-text child satisfies the first ("any") clause on its
own; two plain `Display: Block`/`Flex` `position: absolute` element children, with **no** text
sibling, do **not** — `is_inline_level()` is false for a block-level element, so the "any" clause
is false and `should_create_inline_layout` returns false for a pure 2-absolute-sibling case,
regardless of how many absolute siblings there are. The anonymous-box-wrapping logic at
`tree.rs:1411-1469` (explicitly citing #711 and #738 in its comments) reaches the same
conclusion by a different path (`needs_anonymous_boxes = has_inline && has_block`, both false
when every child is out-of-flow). **This rules the exact known-bug mechanism in/out**: it
explains the *whitespace-sibling* family precisely, but by its own logic does not fire for a
pure, text-free, 2-absolute-sibling JSX case, based on the code as written.

### Best-supported hypothesis (not confirmed)

One structural fact fell out of tracing the shipped test fixtures
(`takumi-1.8.7/tests/fixtures/style_position.rs`) that is worth recording as a lead for further
investigation:

- `test_style_stacking_context_z_index_siblings` (`style_position.rs:39-91`) has **three** direct
  `position: absolute` children under one container — but that container *is itself the render
  tree's root* (passed straight to `run_fixture_test`). Per `push_layout_node`'s hoisting rule
  (`tree.rs:405-409`: `Position::Absolute => Some(*cb_stack.last().unwrap_or(&root_id))`), when a
  container is the root, its absolute children's containing block *is* the container itself, so
  `cb == parent.node_id` and the hoisting branch (`tree.rs:413-422`) is never taken — these
  children stay as ordinary, non-hoisted taffy children. This test therefore does not exercise
  hoisting at all.
- `test_style_absolute_paint_order_under_z_sibling` (`style_position.rs:337-391`) has two
  absolute elements under one `position: relative` container, but only **one** of them ("red")
  is wrapped in an intermediate `position: static` container before reaching its sibling
  ("green" is a direct child) — i.e. this test itself uses the same "wrap it in a plain
  container" pattern as the user's confirmed workaround, and only one of the two absolute nodes
  is actually hoisted.
- **No shipped fixture test exercises two (or more) direct `position: absolute` siblings, under
  a non-root parent, where both are actually hoisted to the same containing block** — which is
  exactly the topology of the reported bug (a parent nested inside a larger app tree, not the
  literal document root). This is circumstantial (absence of a test is not proof of a bug) but is
  the most concrete, evidence-backed gap found.

One consequence of hoisting *is* directly confirmed by trace, though it does not by itself
explain the reported symptom: when every one of P's DOM children is `position: absolute` and
gets hoisted away, P ends up with **zero** real taffy children
(`tree.rs:396`, `taffy_children` stays empty since both `A` and `B` go into the `hoisted` map
instead of `parent.taffy_child_ids`). This was considered as a "P's content box collapses to
0×0, which some other code treats as empty/invisible" theory, but it is **directly falsified**
by the user's own isolation data: a *single* absolute child alone also fully hoists away, leaving
P with zero taffy children in exactly the same way — and that case renders fine. So whatever
differs between 1 and 2+ hoisted siblings, it is not "P has no taffy children."

**Given the above, I did not reach a confirmed line-level root cause.** The strongest lead is
that this is an unfiled edge case adjacent to the #695/#711/#738/#992 family — something about a
non-root parent whose *entire* in-flow content is two-or-more actually-hoisted `position:
absolute` children — but I could not pin the exact failing line. A meaningful fraction of
`layout/tree.rs` (roughly 1700 of its 2435 lines — the `LayoutPartialTree`/taffy trait glue that
actually feeds taffy's flexbox/block algorithms, and the absolute-child measurement/placement
code) was not read in this session and remains the most likely place to find the literal cause if
further tracing is done. It's also possible the true cause sits inside taffy 0.11's own
absolute-positioning pass rather than in takumi's glue code — no taffy issue matching this
symptom was found either, but taffy's absolute-layout code itself was not inspected in this
session (out of scope of the vendored takumi source; would require pulling taffy 0.11's own
source).

## Alternative APIs / workarounds found

- No takumi-specific "layer"/z-index/portal API distinct from raw CSS `position` was found in
  the vendored crate, its `README.md`, `tests/`, or `examples/` — takumi's positioning model is
  plain CSS `position: absolute | relative | fixed | static` plus `z-index`, matching the browser
  model (confirmed by reading `tests/fixtures/style_position.rs` in full, `README.md` head, and
  the `examples/` directory listing — no examples touch positioning at all, only
  profiling/perf scenarios).
- The **only** structural pattern in the shipped test fixtures that avoids two direct
  `position: absolute` siblings under the same parent is exactly the workaround already found
  independently: wrap a positioned element in an intermediate non-positioned
  `Node::container([...])` before adding a sibling (see
  `test_style_absolute_paint_order_under_z_sibling` and `test_style_absolute_percentage_resolves_against_cb`,
  both of which wrap one absolute child in a `Position::Static` `static_wrap` container). This is
  circumstantial support (not an explicit recommendation from the maintainer) but shows the
  pattern is already implicitly "the way the test suite writes these cases," consistent with the
  confirmed workaround.

## Sources

GitHub issues (fetched via `gh issue view --json ...`, full bodies/comments read):
- https://github.com/kane50613/takumi/issues/695
- https://github.com/kane50613/takumi/issues/711
- https://github.com/kane50613/takumi/issues/738
- https://github.com/kane50613/takumi/issues/992

GitHub issue search queries run (via `gh search issues --repo <repo> "<query>"`), all returning
either no results or the issues listed above:
- `kane50613/takumi`: `absolute`, `position`, `blank`, `sibling`, `not rendering`, `disappear`,
  `vanish`, `invisible`, `multiple absolute`, `two absolute`, `stacking context`, `subtree`,
  `hoist`
- `DioxusLabs/taffy`: `multiple absolute`, `absolute siblings`, `two absolute`, `not rendered`,
  `overwrite`, `out of flow`, `absolute children only`, `only absolutely positioned`,
  `childless`, `collapse`

Repo/version metadata:
- https://github.com/kane50613/takumi (repo root; `gh api repos/kane50613/takumi`)
- https://crates.io/crates/takumi/versions (via crates.io API,
  `https://crates.io/api/v1/crates/takumi/versions`)
- https://github.com/kane50613/takumi/blob/master/takumi/CHANGELOG.md (fetched in full via
  `gh api repos/kane50613/takumi/contents/takumi/CHANGELOG.md`)
- https://github.com/kane50613/takumi/blob/master/takumi-core/CHANGELOG.md (fetched in full via
  `gh api repos/kane50613/takumi/contents/takumi-core/CHANGELOG.md`)
- `gh api repos/kane50613/takumi/releases` and `gh api repos/kane50613/takumi/tags` (release/tag
  listing, confirmed 1.8.7 is the last 1.x tag before `2.0.0-rc.1`)

Vendored crate source files read (paths under
`/home/kantord/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`):
- `takumi-1.8.7/Cargo.toml`, `takumi-css-0.1.2/Cargo.toml` (repository field confirmation)
- `takumi-1.8.7/src/rendering/stacking_context.rs` — read in full (1271 lines)
- `takumi-1.8.7/src/layout/tree.rs` — read in part (lines 1-480, 1100-1512; ~735 of 2435 lines):
  `push_layout_node` (out-of-flow hoisting), `RenderNode::should_create_inline_layout`,
  `participates_in_inflow_inline_formatting_context`/`participates_in_inline_formatting_context`,
  and the anonymous-box-wrapping branch of `from_node_iterative`
- `takumi-1.8.7/tests/fixtures/style_position.rs` — read in full (all positioning fixture tests)
- `takumi-1.8.7/README.md` (head), `takumi-1.8.7/tests/`, `takumi-1.8.7/examples/` directory
  listings
- `takumi-css-0.1.2/src/style/stylesheets_query.rs` (grep-confirmed location of
  `participates_in_positioned_paint_bucket`/`creates_stacking_context`, not re-read line by line
  in this session — already reviewed by the requester per task context)

Not read in this session (flagged above as the most likely place to continue): the remaining
~1700 lines of `takumi-1.8.7/src/layout/tree.rs` (the `LayoutPartialTree`/taffy trait
implementation and absolute-child measurement/placement code), and taffy 0.11's own source.

## Addendum: a second, distinct nesting-related failure (2026-07-26)

Found while adding a second line of text to `ModePill.jsx` in the dotfiles
config (`~/.local/share/chezmoi/dot_config/tauler/components/ModePill.jsx`).
**Not the same bug as above** — no `position: absolute` involved at all, and
worth recording separately since it broadens what's known to be fragile in
this renderer.

### Repro

Original (known-good, shipped for a long time before this):

```jsx
return (
  <container tw="flex flex-row w-full px-3 pt-[8px]">
    <container tw="flex flex-row items-center gap-[6px] rounded-full border px-[10px] py-[3px]" style={{...}}>
      <container tw="w-[6px] h-[6px] rounded-full flex-shrink-0" style={{...}} />
      <text tw="text-[10px] font-bold" style={{...}}>{label}</text>
    </container>
  </container>
);
```

Changed to add a second line below the pill (wrapping the existing single row
in one extra `flex-col` level, plus a new sibling `<text>`):

```jsx
return (
  <container tw="flex flex-col w-full px-3 pt-[8px] gap-[4px]">
    <container tw="flex flex-row w-full">
      <container tw="flex flex-row items-center gap-[6px] rounded-full border px-[10px] py-[3px]" style={{...}}>
        <container tw="w-[6px] h-[6px] rounded-full flex-shrink-0" style={{...}} />
        <text tw="text-[10px] font-bold" style={{...}}>{label}</text>
      </container>
    </container>
    {secondLine && <text tw="text-[9px] pl-[2px]" style={{...}}>{secondLine}</text>}
  </container>
);
```

**Symptom:** the entire sidebar panel (not just this component) rendered as a
solid black rectangle. Confirmed via a live restart-and-check cycle (not just
a hot-reload check — hot-reload was independently unreliable throughout this
same debugging session, see the tauler-side reload-mechanism symptom report
from the same day if one was filed).

**Confirmed NOT the cause:** a same-session `useJSONStream(...)` call added at
the same time (a new, unconditional data-fetching hook feeding `secondLine`).
Isolated by reverting *only* the wrapper-nesting shape back to the original
flat single-row structure while *keeping* the new hook and folding its result
into the existing single `<text>` as an inline suffix (`` `${label} · ${extra}` ``)
instead of a second sibling text node. That version — same hook, same data,
zero new `<container>` nesting — rendered correctly. So the new
`useJSONStream` call itself, and the JSON payload it produced, were not
implicated.

**Confirmed cause (by elimination):** adding one extra level of `<container>`
nesting (`flex-col` wrapping what used to be the root `flex-row`) plus a new
sibling `<text>` node broke rendering for the *entire panel*, not just this
component's subtree — consistent with the panel-wide corruption pattern
described in the "black box" observation earlier in this doc's original
research, though that was never conclusively tied to a root cause either.

**Workaround applied:** avoid the extra nesting level; fold additional
information into the existing single row's text content instead of adding a
sibling row. Not investigated further at the source level (no takumi/taffy
code read for this specific case) — this is a symptom report for whoever
picks up the reload/rendering investigation next, not a root-caused fix.
