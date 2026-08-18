import { defineConfig } from '@playwright/test'

// One project per breakpoint: the same assertions run at every size, and
// each project keeps its own visual-regression baseline.
const breakpoints = [
  { name: 'desktop', viewport: { width: 1920, height: 1080 } },
  { name: 'big-monitor', viewport: { width: 2560, height: 1440 } },
  { name: 'laptop', viewport: { width: 1366, height: 768 } },
  { name: 'tablet', viewport: { width: 768, height: 1024 } },
  { name: 'tablet-landscape', viewport: { width: 1024, height: 768 } },
  { name: 'mobile', viewport: { width: 390, height: 844 } },
  { name: 'mobile-landscape', viewport: { width: 844, height: 390 } },
]

export default defineConfig({
  testDir: './tests',
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  use: {
    baseURL: 'http://127.0.0.1:4321',
  },
  expect: {
    toHaveScreenshot: {
      // The flow field is seeded and deterministic and the browser is
      // pinned, so snapshots reproduce exactly. A near-zero budget: a 1%
      // ratio was once loose enough to wave through a full copy rewrite
      // on desktop viewports.
      maxDiffPixels: 64,
    },
  },
  projects: breakpoints.map(({ name, viewport }) => ({
    name,
    use: { browserName: 'chromium' as const, viewport },
  })),
  webServer: {
    // Test the production build: no dev toolbar in the screenshots.
    // sirv, not `astro preview`: preview daemonizes when stdin is not a TTY,
    // so Playwright would think it exited and later reuse stale daemons.
    command:
      'pnpm exec astro build && pnpm exec sirv dist --host 127.0.0.1 --port 4321',
    url: 'http://127.0.0.1:4321',
    // Never reuse: a lingering server (astro's TTY-less daemon mode, an old
    // sirv) serves a stale dist and silently poisons every assertion.
    reuseExistingServer: false,
    timeout: 120_000,
  },
})
