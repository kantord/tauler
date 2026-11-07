import { test, expect, request as pwRequest } from '@playwright/test'

// Every page loads without a console error or an uncaught exception.
//
// The build, the linter and the type checker all pass on a page React can't
// hydrate: `**3:1**` in prose once became a <div> inside a <p>, which the
// static HTML carries silently and only the browser complains about. The
// production build doesn't print the dev-mode "In HTML, <div> cannot be a
// descendant of <p>" message — it reports a minified "Minified React error
// #418" (hydration mismatch) through reportError, which surfaces here as a
// `pageerror`. Catching both channels catches either form.
//
// Nothing is allowlisted. Warnings are not errors: the home page logs a
// console.warn when it falls back to the wallpaper without hardware WebGL,
// and that is expected.

// Matches playwright.config.ts's `use.baseURL` — routes are discovered at
// module load, outside any fixture, as in responsive-safety.spec.ts.
const BASE_URL = 'http://127.0.0.1:4321'

async function discoverRoutes(): Promise<string[]> {
  const ctx = await pwRequest.newContext({ baseURL: BASE_URL })
  try {
    const xml = await (await ctx.get('/sitemap.xml')).text()
    return [...xml.matchAll(/<loc>(.*?)<\/loc>/g)].map(
      (m) => new URL(m[1]).pathname,
    )
  } finally {
    await ctx.dispose()
  }
}

const routes = [...new Set(['/', ...(await discoverRoutes())])]

// Long enough for React to hydrate and for effects scheduled after it (the
// wallpaper, the home page background) to run and report.
const SETTLE_MS = 500

for (const route of routes) {
  test(`${route} loads without console errors`, async ({ page }, testInfo) => {
    test.skip(
      testInfo.project.name !== 'desktop',
      'console errors do not depend on the viewport',
    )
    const errors: string[] = []
    page.on('console', (message) => {
      if (message.type() === 'error') errors.push(`console: ${message.text()}`)
    })
    page.on('pageerror', (error) => {
      errors.push(`pageerror: ${error.message}`)
    })

    await page.goto(route, { waitUntil: 'networkidle' })
    await page.waitForTimeout(SETTLE_MS)

    expect(errors).toEqual([])
  })
}
