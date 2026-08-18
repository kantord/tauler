# The docs site gates pixels

ADR 0005 reviews desktop screenshots instead of gating them, and ADR 0028
gates the web renderer on geometry, not pixels — both because tauler's own
render output crosses machines, font stacks and GPU rasterizers that make
exact pixels unreproducible. The docs site's Playwright suite does the
opposite: `toHaveScreenshot` with a 1% diff budget, baselines committed.

The difference is that the landing page removed every source of
nondeterminism those ADRs were defending against: the generative field is
drawn once from a seeded LCG, the fonts are self-hosted subsets loaded
before capture, there is no animation, and the browser is Playwright's own
pinned Chromium on every machine. In the review run that prompted this ADR,
all baselines reproduced exactly. Pixel gating here is cheap and catches
what geometry cannot — a token rename that silently turns the accent gray.
This decision is scoped to `docs/tests/`; it does not touch 0005 or 0028.
