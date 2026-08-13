# 0006 — e2e scenarios are `#[ignore]`, and fail loudly when asked to run

## Status

Accepted.

## Context

The scenarios need Docker and a built image. Neither is something a contributor
fixing a typo should have to install, so `cargo test --workspace` must keep
working without them.

The established way to arrange that in this repository is a runtime check and a
silent skip. `tests/wallpaper_x11_test.rs` skips when Xvfb is absent;
`tauler-docgen`'s hash test skips when the screenshot binary has not been built.
CI installs Xvfb in neither job, so the first has never once executed there. It
is green, and has been green while covering nothing, for as long as it has
existed.

That is the failure mode to avoid: a check whose absence is indistinguishable
from its success.

## Decision

The scenarios are `#[ignore]` by default. They do not run under
`cargo test --workspace` at all — not skipped, not reported as passing, simply
not part of that run.

When they are asked to run — `just e2e`, or the CI job — a missing Docker daemon
or image is an error, with the recipe to fix it in the message. Nothing about
the suite decides for itself that it should not run.

CI additionally counts the produced screenshots against the number of fixture
directories. A harness that started, did nothing and exited cleanly would
otherwise be indistinguishable from one with no work to do.

## Consequences

`cargo test --workspace` no longer covers everything the repository can test,
and there is now a second command to know about.

The two existing silent skips are untouched by this decision. They are still
wrong for the same reason; changing them is not this ADR's business.
