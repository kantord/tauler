#!/usr/bin/env bash
# Runs the Playwright suite inside the exact mcr.microsoft.com/playwright
# image CI's `container:` field pins (docs-ci.yaml) — font hinting and
# antialiasing differ enough across host OSes that a bare dev machine's
# verdict on `toHaveScreenshot` cannot be trusted (ADR 0031). `pnpm test`
# on a plain machine is fine for the other suites (above-fold, overflow,
# a11y), but not for visual regression; use this instead, both to check
# and to regenerate baselines — CI and this script are now the same
# environment, so what passes here passes there.
#
# --update regenerates the committed baselines. No flag just verifies them
# (what CI does).
#
# The image tag is derived from @playwright/test, so it can't drift from
# the npm package on its own; renovate.json groups it with the tag
# hardcoded in docs-ci.yaml's `container:` field so those two stay in sync.
set -euo pipefail
cd "$(dirname "$0")/.."

PLAYWRIGHT_VERSION=$(node -e "console.log(require('@playwright/test/package.json').version)")
IMAGE="mcr.microsoft.com/playwright:v${PLAYWRIGHT_VERSION}-noble"

TEST_CMD='pnpm exec playwright test'
if [ "${1:-}" = '--update' ]; then
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
