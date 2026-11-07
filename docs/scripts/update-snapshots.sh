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
# (what CI does). Further arguments go to Playwright, e.g. tests/docs.spec.ts.
#
# The image tag is derived from @playwright/test, so it can't drift from
# the npm package on its own; renovate.json groups it with the tag
# hardcoded in docs-ci.yaml's `container:` field so those two stay in sync.
set -euo pipefail
cd "$(dirname "$0")/.."

PLAYWRIGHT_VERSION=$(node -e "console.log(require('@playwright/test/package.json').version)")
IMAGE="mcr.microsoft.com/playwright:v${PLAYWRIGHT_VERSION}-noble"

UPDATE=''
if [ "${1:-}" = '--update' ]; then
  UPDATE='--update-snapshots'
  shift
fi
# Anything else is passed through to Playwright (e.g. a spec file).
TEST_CMD="pnpm exec playwright test $UPDATE $*"

# On exit, failure too: delete the container's build output and hand the test
# reports back to the invoking user. The build output is root-owned and has
# the container's paths baked in (.source/ imports /repo/docs/...), so a host
# build that reuses it fails with "Module not found ... outside configured
# roots".
trap 'docker run --rm -v "$(pwd):/d" alpine sh -c \
  "rm -rf /d/dist /d/.next /d/.source; \
   chown -R $(id -u):$(id -g) /d/test-results" \
  2>/dev/null || true' EXIT

docker run --rm -e CI=true \
  -v "$(pwd)/..:/repo" -v /repo/docs/node_modules \
  -w /repo/docs "$IMAGE" bash -c "
    corepack enable pnpm >/dev/null 2>&1
    pnpm install --frozen-lockfile
    # The host's build caches were made by a different Node and toolchain;
    # build from scratch, as CI's fresh checkout does.
    rm -rf .next .source dist
    $TEST_CMD
  "

