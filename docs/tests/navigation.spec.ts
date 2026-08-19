import { test, expect } from '@playwright/test'

// The landing page and the docs site are two different shells (plain Astro
// page vs. Starlight) sharing one design system — this is the seam between
// them. Runs at every breakpoint project like the rest of the suite, since
// which HOME/DOCS control is reachable differs below Starlight's 50em
// breakpoint (menu button vs. a direct link).

test('landing → docs via the DOCS link', async ({ page }) => {
  const errors: string[] = []
  page.on('pageerror', (err) => errors.push(String(err)))

  await page.goto('/')
  await page.getByRole('link', { name: 'DOCS', exact: true }).click()

  await expect(page).toHaveURL(/\/docs\/$/)
  await expect(page.locator('h1')).toContainText('tauler')
  expect(errors).toEqual([])
})

test('docs → landing via the HOME link', async ({ page }) => {
  const errors: string[] = []
  page.on('pageerror', (err) => errors.push(String(err)))

  await page.goto('/docs/layout-file/')
  const width = page.viewportSize()?.width ?? 0
  if (width < 800) {
    await page.locator('starlight-menu-button button').click()
    await page.locator('.dx-mobile-links a[href="/"]').click()
  } else {
    await page.locator('.dx-nav-link[href="/"]').click()
  }

  await expect(page).toHaveURL(/\/$/)
  // The landing's own above-fold contract (see landing.spec.ts) — spot-check
  // that the shell actually rendered, not just that the URL changed.
  await expect(page.locator('.above-fold').first()).toBeVisible()
  expect(errors).toEqual([])
})

test('browser back from docs returns to a working landing page', async ({
  page,
}) => {
  await page.goto('/')
  await page.getByRole('link', { name: 'DOCS', exact: true }).click()
  await expect(page).toHaveURL(/\/docs\/$/)

  await page.goBack()
  await expect(page).toHaveURL(/\/$/)
  await expect(page.locator('.above-fold').first()).toBeVisible()
})
