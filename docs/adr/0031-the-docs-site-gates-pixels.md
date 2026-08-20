# The docs site gates pixels

ADR 0005 reviews desktop screenshots instead of gating them, and ADR 0028
gates the web renderer on geometry, not pixels — both because tauler's own
render output crosses machines, font stacks and GPU rasterizers that make
exact pixels unreproducible. The docs site's Playwright suite does the
opposite: `toHaveScreenshot` with a 1% diff budget, baselines committed.

The difference is that the landing page removed every source of
nondeterminism those ADRs were defending against: the generative field is
drawn once from a seeded LCG, the fonts are self-hosted subsets loaded
before capture, and there is no animation. Pixel gating here is cheap and
catches what geometry cannot — a token rename that silently turns the
accent gray. This decision is scoped to `docs/tests/`; it does not touch
0005 or 0028.

One assumption in the review run that prompted this ADR did not hold: "the
browser is Playwright's own pinned Chromium on every machine" is true of
the binary but not of the pixels it produces. A pinned Chromium still
renders through the host OS's font hinting and rasterizer, and CI first
failed on baselines generated on a bare-metal dev machine — 124 to 141
pixels different per breakpoint, with no code change between the passing
local run and the failing CI run.

The fix took three tries. First guess: raise `maxDiffPixels` from 64 to
300 to cover the measured drift — CI still failed, now by 390-470 pixels,
because a bare-metal dev machine isn't the only thing that renders
differently from CI; it just happened to be the first one measured.
Second guess: generate baselines inside
`mcr.microsoft.com/playwright:v<version>-noble` and treat that as close
enough to CI's `ubuntu-latest` + `playwright install --with-deps
chromium` — CI *still* disagreed with it. A container that merely
resembles CI's environment isn't CI's environment; `ubuntu-latest` plus
an install step is its own third thing, distinct from both a bare dev
machine and the official image.

The fix that stuck removes the comparison entirely: `docs-ci.yaml`'s
`test` job now runs *inside* `mcr.microsoft.com/playwright:v<version
>-noble` via the job-level `container:` field, instead of on
`ubuntu-latest` with a browser installed alongside it. CI and
`docs/scripts/update-snapshots.sh` run the identical image and the
identical command, so there is no second environment left to drift from
a first — baselines are generated and committed from a local run of that
script. `renovate.json` groups `@playwright/test` with the
`mcr.microsoft.com/playwright` image tag so a version bump can't
reintroduce the gap between the two places that name a version. What's
left is not zero: two runs of the same container image on different
physical CPUs can still round floating-point rasterizer math a few pixels
apart. `maxDiffPixels` stayed at 300 to absorb that, not the
hundred-pixel class of drift the first two tries were fighting.
