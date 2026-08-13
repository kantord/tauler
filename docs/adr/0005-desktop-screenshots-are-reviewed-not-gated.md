# 0005 — Desktop screenshots are reviewed, not gated

## Status

Accepted.

## Context

`tauler-docgen` already renders component examples and gates them: the rendered
PNGs are committed, and a test fails if a fresh render's hash disagrees. That
works because those renders are hermetic — one process, fixed fonts, no window
manager, no compositing.

The desktop screenshots from `tauler-e2e` are a different kind of artifact. They
come out of a real X server with a real i3 laying out real client windows. Font
hinting, i3's own drawing, xterm's glyph rasterisation and the client startup
race all land in those pixels. Some of that is pinned by the image; not all of
it is, and a base image rebuild can move any of it.

Gating on a hash would therefore mean one of two things: a suite that goes red
for reasons unrelated to the change under review, or a tolerance threshold —
which is a number nobody can justify and everybody eventually raises.

## Decision

Desktop screenshots are published for a human to look at, not compared against a
stored copy. CI runs every scenario on every pull request, pushes the PNGs to an
orphan `ci-screenshots` branch, and keeps a single comment on the PR pointing at
them. They are not committed to `docs/`, and nothing fails because pixels moved.

What *is* gated is the structure around them: the gaps i3 reports, each panel's
geometry as the X server reports it, no managed window overlapping a panel, and
a centre-pixel sample proving a panel painted rather than merely mapped. Those
are exact assertions with hand-written expected values, and they are what turns
the suite red.

## Consequences

A layout regression that is visible but does not violate any asserted invariant
is caught only if someone looks at the comment. That is the accepted trade: the
alternative is a check that cries wolf until it is ignored or deleted.

The `ci-screenshots` branch accumulates a directory per pull request. It is
orphaned, so it carries none of the repository's history, and nothing builds
from it.

Fork pull requests get a read-only token and can neither push the branch nor
comment. They still run every scenario and still fail on a real regression; the
images arrive as a workflow artifact instead.
