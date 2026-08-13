# e2e scenarios are `#[ignore]`, and fail loudly when asked to run

The scenarios need Docker and a built image, which a contributor fixing a typo should not
have to install. They are `#[ignore]` by default, so they do not run under `cargo test
--workspace` at all — not skipped, not reported as passing, simply not part of that run.
When they are asked to run, by `just e2e` or the CI job, a missing Docker daemon or image
is an error with the fix in the message.

## Why not skip silently, as this repo already does twice

`tests/wallpaper_x11_test.rs` skips when Xvfb is absent, and `tauler-docgen`'s hash test
skips when the screenshot binary has not been built. CI installs Xvfb in no job, so the
first has never once executed there. It is green, and has been green while covering
nothing, for as long as it has existed.

That is the failure mode worth avoiding: a check whose absence is indistinguishable from
its success. `#[ignore]` states the same thing honestly — this did not run — instead of
reporting a pass.

## Consequences

`cargo test --workspace` no longer covers everything the repository can test, and there is
a second command to know about.

CI counts the produced screenshots against the number of fixture directories, because a
harness that started, did nothing, and exited cleanly would otherwise be indistinguishable
from one with no work to do.

The two existing silent skips are untouched. They are wrong for the same reason; fixing
them is not this decision's business.
