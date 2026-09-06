# `<Workspaces>`'s arithmetic lives in `src/`, not `tauler-core`

`<Workspaces>` is split the same shape as `<I3Layout>` (ADR 0003): a small JS global in
`JSX_GLOBALS_JS` dispatches to a Rust half. But the Rust half is a plain native function
registered in `src/jsx.rs` (`__workspaces_layout`, `src/workspaces.rs`), not a
`tauler-core` `#[component]` like `ui::components::i3_layout` is. The split is
deliberate.

## Why not a `tauler-core` component

`<I3Layout>`'s arithmetic only ever operates on numbers already in hand — each
`<Panel>`'s declared `size`. `<Workspaces>` has the opposite shape: nothing is declared.
The whole point is deriving a frame's thickness from a wrapper's own CSS, which means
actually laying the wrapper out and reading back where its `<Contents/>` placeholder
landed. That measurement is `crate::hit_test::painted_boxes`, which drives
`takumi_core::layout::tree::LayoutTree` directly.

`tauler-core` may never depend on `takumi`/`takumi_core` — that boundary is what lets it
compile for `wasm32-unknown-unknown` and run a layout file in a browser (ADR 0010), and
it's enforced in CI, not just convention. So the measurement this component needs cannot
live there.

## Consequences

`__workspaces_layout` is registered the way `useStringStream`/`registerModule` already
are — a plain `rquickjs::Function` global set up alongside the QuickJS context in
`src/jsx.rs` — rather than through the `tauler-ui-macro`/`#[component]`/`UI_COMPONENTS`
registry, which only ever reaches `tauler-core` code. The JS-side shim itself
(`Contents`, `Workspaces`, and the few lines `<I3Layout>` gained to dispatch to a
trailing `<Workspaces>`) is still plain JS with nothing takumi-shaped in it, so it stays
in `tauler-core/src/globals.rs`'s shared `JSX_GLOBALS_JS` next to `<I3Layout>`'s own shim
— only the measurement primitive needed to move.

One consequence of this split, not a cause of it: `<Workspaces>` has no meaning in the
web renderer, same as `<I3Layout>` — there's no i3 to reserve gaps in a page, and no
wasm-side `__workspaces_layout` is ever registered. A layout file that uses either inside
a page renders nothing where they were, rather than failing.

## The generated panels are re-rendered and translated, not cropped

Each panel `<Workspaces>` emits re-renders the *whole* wrapper subtree inside an
`overflow-hidden` box, shifted by a `translate` — `ScrollArea`'s existing
`content_translate` trick (`tauler-core/src/ui/components/scroll_area.rs`) — rather than
`position: absolute`. `docs/takumi-absolute-sibling-bug-research.md` documents an
unresolved takumi bug where two-or-more `position: absolute` siblings under one parent
blank the whole subtree; `translate` on a single normal-flow child was already proven in
production and sidesteps that bug family entirely.

This also means these panels are not binary crops of one pre-rendered buffer, the way
`tauler:root-bg` crops a wallpaper (`src/backdrop/mod.rs`). Re-running takumi's own
layout and paint on the same small decorative subtree up to four times a tick was judged
simpler than plumbing a new cropped-image resource type through the render pipeline, and
is no different in kind from ADR 0007's "every tick re-renders everything."

## Naming: `EdgeStrips`, not `Frame`

`src/workspaces.rs`'s internal types are `EdgeStrips` and `WorkspacesLayout`, not
`Frame`/`WorkspacesFrame` as first written — CONTEXT.md already defines **Frame** as a
Render target's finished pixels (ADR-unrelated, much older), and reusing the word for
"the four border strips around a workspace area" would have been the exact collision the
glossary exists to prevent, just in code and docs instead of conversation.
