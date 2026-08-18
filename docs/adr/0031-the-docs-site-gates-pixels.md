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
pixels different per breakpoint, on every single one, with no code change
between the passing local run and the failing CI run. Baselines are now
generated inside `mcr.microsoft.com/playwright:v<version>-noble` — the
same image CI's `--with-deps chromium` install effectively matches — via
`pnpm run test:update:baselines` (`docs/scripts/update-snapshots.sh`), not
bare `playwright test --update-snapshots`. `maxDiffPixels` moved from 64 to
300: still two orders of magnitude below what a real content or layout
regression moves, but wide enough to absorb the measured host-rendering
drift.
