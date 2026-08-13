# `<I3Layout>` is a JS shim over a Rust component

`<I3Layout>` is split: the edge arithmetic is a Rust UI component
(`ui::components::i3_layout`), and a small JS global in `JSX_GLOBALS_JS` dispatches to it
and registers the resulting gaps. The split is deliberate.

## Why not all Rust

Registering the gaps means calling `useEvents`, which is a JS-side call. A Rust UI
component receives serde data and returns a node tree — it has no `Ctx` to call back into
the runtime with. Something on the JS side has to make that call.

## Why not all JS

A `.jsx` helper file shipped alongside the layout was considered and rejected: the project
has no JavaScript test runner, so the arithmetic would have been untestable. Keeping it in
Rust puts the arithmetic under ordinary unit tests and leaves the JS holding nothing but a
call and a registration — still untested, but with no branches or arithmetic in it to be
wrong about.

## Consequences

The gaps are registered *after* the children, because `<I3Layout>` cannot know them until
every `<Panel>` has been evaluated, and children evaluate before their parent. That forced
`registerModule` to merge registrations for a bin instead of keeping the first — see
`jsx::merge_missing`. Anything relying on first-registration-wins would break.

The Rust half is marked `# Internal` so docgen excludes it. It shares a name with the
global but returns `{panels, gaps}` data rather than nodes, and a generated docs entry
telling a user to `import` it would be actively wrong.
