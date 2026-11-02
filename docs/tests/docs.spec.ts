import { test, expect } from '@playwright/test'
import AxeBuilder from '@axe-core/playwright'

// Every project in playwright.config.ts is a different device breakpoint —
// these run at all seven, so a layout break at one width doesn't slip
// through untested.
const ROUTES = [
  '/',
  '/docs/',
  '/docs/layout-file/',
  '/docs/elements/',
  '/docs/data/',
  '/docs/layout/',
  '/docs/components/',
  '/docs/macos/',
]

for (const route of ROUTES) {
  test.describe(route, () => {
    test('never scrolls horizontally', async ({ page }) => {
      await page.goto(route)
      const overflow = await page.evaluate(
        () => document.documentElement.scrollWidth - window.innerWidth,
      )
      expect(overflow).toBeLessThanOrEqual(0)
    })

    test('no automatically detectable accessibility violations', async ({
      page,
    }) => {
      await page.goto(route)
      const results = await new AxeBuilder({ page })
        // The destructive-variant live preview has a known, already-filed
        // contrast bug: text-destructive-foreground isn't in
        // themes/default.yaml, so nothing resolves it, on either renderer —
        // see docs/scripts/check-classes.mjs's KNOWN_UNRESOLVED entry.
        .exclude('[data-tauler-example]')
        // Fumadocs' stock theme fails these out of the box (muted sidebar
        // and code-token contrast, an unlabelled second <nav>, content
        // outside landmarks, a second <header> banner on mobile). The site
        // runs that theme unmodified for now; re-enable each rule as the
        // theme is customised.
        .disableRules([
          'color-contrast',
          'region',
          'landmark-unique',
          'landmark-no-duplicate-banner',
        ])
        .analyze()
      expect(
        results.violations,
        JSON.stringify(results.violations, null, 2),
      ).toEqual([])
    })
  })
}

test('the sidebar is reachable', async ({ page }) => {
  await page.goto('/docs/layout-file/')
  // Below Fumadocs' md breakpoint the sidebar hides behind the menu button.
  const width = page.viewportSize()?.width ?? 0
  if (width < 768) {
    await page
      .getByRole('button', { name: /open sidebar/i })
      .first()
      .click()
  }
  await expect(
    page.getByRole('link', { name: 'Elements and styling' }).last(),
  ).toBeVisible()
})

test('search finds a page', async ({ page }) => {
  await page.goto('/docs/layout-file/')
  const dialog = page.getByRole('dialog')
  // The button is server-rendered; it only opens the dialog once hydrated.
  await expect(async () => {
    await page
      .getByRole('button', { name: /search/i })
      .first()
      .click()
    await expect(dialog).toBeVisible({ timeout: 1000 })
  }).toPass()
  await dialog.locator('input').fill('subprocess')
  await expect(dialog.getByText('Data and events').first()).toBeVisible()
})

test('an old root-level docs URL redirects to /docs/', async ({ page }) => {
  await page.goto('/layout-file/')
  await expect(page).toHaveURL(/\/docs\/layout-file\/$/)
})
