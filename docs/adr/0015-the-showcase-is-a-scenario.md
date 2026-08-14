# The showcase is a Scenario, not a render

`tauler-e2e`'s third scenario, `showcase`, exists to look good: a floating bar over a
wallpaper tauler painted itself, a custom palette, and terminals that read that wallpaper
back through `_XROOTPMAP_ID`. It is nonetheless a Scenario in the full sense the glossary
means — a layout file *plus the reservation it is supposed to produce* — with expected
gaps and panel geometry written by hand in `scenarios.rs` and checked by the same `run`
that checks `sidebar` and `three-edge`.

It adds three claims of its own, after `run` returns rather than inside it:

- two pixels in the panel's 12px margin must differ, proving `root-bg` bound a *crop* of
  the wallpaper rather than a flat fill
- the bar's own pixel must carry chroma, proving `theme.file` loaded — an unreadable one
  is only a warning, and the greyscale fallback renders a perfectly correct bar
- both clients must exist before their geometry is asserted, because they deliberately
  start late

## Why not let it opt out of the assertions

Its output is judged by eye, its content will be fiddled with often, and every edit that
moves a pixel is an edit that might have to touch `scenarios.rs`. The tempting move is to
capture the screenshot, skip the contract and call it a render.

That would make the one scenario a reader is most likely to copy the one scenario that
checks nothing. The first two claims above also couple the test to the artwork and the
palette, which is accepted and commented at both ends: editing the wallpaper can fail the
suite, and the alternative is an assertion that passes whatever the image is.

## What building it found

Four things were broken rather than merely missing, all fixed alongside because the
scenario cannot render without them.

`preload_layout_images` was exported, unit-tested and documented, and called from nowhere
in the application — so `<image src="…file…">` had never worked at runtime. Only
`root-bg` did, because it is bound per render. It is now called each tick and skips paths
already in the store, so it stays one read per path.

The renderer decodes only PNG and ICO, so a JPEG wallpaper renders black with nothing in
the log. `<wallpaper>` requires an `id`, which its props table did not list, and omitting
it fails the whole root parse rather than that one node. Both are now in the layout-file
docs.

The e2e image's `COPY assets/fonts` landed in `/usr/local/share/tauler/fonts`, which
nothing has ever read; it now goes to `/usr/local/share/fonts`. Installing `fontconfig` to
place a Nerd Font also revealed that the image had been resolving `sans-serif` to Nimbus
Sans, so every screenshot in the suite had been typeset in a Helvetica clone by accident.
The image now names its generic families.

## Consequences

Two mechanisms exist that did not before, both following the pattern already set by the
optional `config.yaml`: a fixture may carry its own `i3.config` and its own `startup`.
Contract scenarios stay minimal; a scenario with taste keeps its taste to itself.

**A fixture's `i3.config` must not set a per-side outer gap.** i3 reports a workspace's
`gaps` in `GET_TREE` as a delta from the global default, and `focused_workspace_gaps`
reads that field while `scenarios.rs` states absolutes. They agree only because the global
default is zero. `gaps inner` is a different field and is safe; `gaps left 8` would leave
the assertion passing while measuring something else.

The containment check now reads the focused workspace's clients rather than every window
in the tree. That is a correction, not a concession: runtime `gaps` can only target
`current` or `all`, so tauler-i3 writes to whichever workspace is focused and a workspace
never focused since startup carries none. Checking the whole tree against the focused
workspace's gaps was only ever right because no fixture had a second workspace, and it
failed on a correct desktop the moment one did.

Inter is visible to tauler inside the image for the first time, so the other two
scenarios' screenshots change too. Nothing is gated on those pixels (ADR 0005), but the
movement shows up in the pull request comment looking unrelated.
