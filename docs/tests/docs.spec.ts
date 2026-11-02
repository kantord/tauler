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
        .analyze()
      expect(
        results.violations,
        JSON.stringify(results.violations, null, 2),
      ).toEqual([])
    })
  })
}

test('the contents tile links every page', async ({ page }) => {
  await page.goto('/docs/layout-file/')
  const contents = page.getByRole('navigation', { name: 'Docs' })
  await expect(
    contents.getByRole('link', { name: 'Elements and styling' }),
  ).toBeVisible()
  await expect(
    contents.getByRole('link', { name: 'The layout file' }),
  ).toHaveAttribute('aria-current', 'page')
})

test('the theme toggle switches between light and dark', async ({ page }) => {
  await page.goto('/docs/layout-file/')
  const html = page.locator('html')
  const toggle = page.getByRole('button', {
    name: /switch to (light|dark) mode/i,
  })
  await expect(async () => {
    const before = await html.getAttribute('class')
    await toggle.click()
    expect(await html.getAttribute('class')).not.toBe(before)
  }).toPass()
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
