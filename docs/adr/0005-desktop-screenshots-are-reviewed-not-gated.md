# Desktop screenshots are reviewed, not gated

`tauler-e2e` publishes its screenshots for a person to look at rather than comparing them
against a stored copy. CI runs every scenario on every pull request, pushes the PNGs to an
orphan `ci-screenshots` branch, and keeps one comment on the PR pointing at them. Nothing
fails because pixels moved.

## Why not gate them, when docgen does

`tauler-docgen` commits its component renders and fails when a fresh hash disagrees. That
works because those renders are hermetic: one process, fixed fonts, no window manager, no
compositing.

A desktop screenshot is a different kind of artifact. It comes out of a real X server with
a real i3 laying out real client windows, so font hinting, i3's own drawing, xterm's glyph
rasterisation and the client startup race all land in the pixels. The image pins some of
that. It does not pin all of it, and a base image rebuild can move any of it.

Gating on a hash would therefore buy either a suite that goes red for reasons unrelated to
the change under review, or a tolerance threshold — a number nobody can justify and
everybody eventually raises.

## Consequences

What is gated is the structure, not the pixels: the gaps i3 reports, each panel's geometry
as the X server reports it, no managed window overlapping a panel, and a centre-pixel
sample proving a panel painted rather than merely mapped. Those are exact assertions with
hand-written expected values, and they are what turns the suite red.

So a regression that is visible but violates no asserted invariant is caught only if
someone looks at the comment. That is the accepted trade against a check that cries wolf
until it is ignored.

Fork pull requests get a read-only token and can neither push the branch nor comment. They
still run every scenario and still fail on a real regression; the images arrive as a
workflow artifact instead.
