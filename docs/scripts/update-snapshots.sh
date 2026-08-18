#!/usr/bin/env bash
# Runs the Playwright visual-regression suite inside the same
# mcr.microsoft.com/playwright image CI runs — font hinting and
# antialiasing differ enough across host OSes that baselines generated on
# a bare-metal dev machine can fail in CI (and vice versa) by 100+ pixels
# even with an identical pinned Chromium build. `pnpm test` on a plain dev
# machine is fine for the other suites (above-fold, overflow, a11y), but
# don't trust its visual-regression verdict; use this instead. See
# playwright.config.ts and ADR 0031.
#
# --check verifies the committed baselines (docs/test:docker). No flag
# regenerates them (docs/test:update:baselines).
#
# Keep the tag in sync with the @playwright/test version in package.json.
set -euo pipefail
cd "$(dirname "$0")/.."

PLAYWRIGHT_VERSION=$(node -e "console.log(require('@playwright/test/package.json').version)")
IMAGE="mcr.microsoft.com/playwright:v${PLAYWRIGHT_VERSION}-noble"

TEST_CMD='pnpm exec playwright test'
if [ "${1:-}" != '--check' ]; then
  TEST_CMD='pnpm exec playwright test --update-snapshots'
fi

docker run --rm -e CI=true \
  -v "$(pwd)/..:/repo" -v /repo/docs/node_modules \
  -w /repo/docs "$IMAGE" bash -c "
    corepack enable pnpm >/dev/null 2>&1
    pnpm install --frozen-lockfile
    $TEST_CMD
  "

# The container runs as root; hand ownership of anything it touched back
# to the invoking user so a plain `pnpm test` still works afterward.
docker run --rm -v "$(pwd)/..:/repo" alpine \
  chown -R "$(id -u):$(id -g)" /repo/docs/dist /repo/docs/test-results 2>/dev/null || true
