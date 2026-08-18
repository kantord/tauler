import { test, expect } from '@playwright/test'
import AxeBuilder from '@axe-core/playwright'

// The landing page marks everything that must be visible before the fold —
// without scrolling or any other action — with the `above-fold` class.
// Keep this count in sync with the markup: it exists so that silently
// dropping an element fails the test instead of shrinking the loop.
const ABOVE_FOLD_COUNT = 10

test.beforeEach(async ({ page }) => {
  await page.goto('/')
  // Type and canvas both affect layout and the screenshot.
  await page.evaluate(() => document.fonts.ready)
})

test('every above-fold element is visible without scrolling', async ({
  page,
}) => {
  const critical = page.locator('.above-fold')
  await expect(critical).toHaveCount(ABOVE_FOLD_COUNT)

  for (const element of await critical.all()) {
    await expect(element).toBeVisible()
    // ratio: 1 — fully inside the viewport, not just clipped at the fold.
    await expect(element).toBeInViewport({ ratio: 1 })
  }
})

test('page never scrolls horizontally', async ({ page }) => {
  // A visual diff crops to the viewport and can't see content pushed past
  // its right edge; this is the one check for real horizontal overflow —
  // e.g. the panel bar and the capped hero column silently drifting out of
  // alignment on very wide viewports.
  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - window.innerWidth,
  )
  expect(overflow).toBeLessThanOrEqual(0)
})

test('visual regression', async ({ page }) => {
  await expect(page).toHaveScreenshot('landing.png')
})

test('no automatically detectable accessibility violations', async ({
  page,
}) => {
  const results = await new AxeBuilder({ page }).analyze()
  expect(
    results.violations,
    JSON.stringify(results.violations, null, 2),
  ).toEqual([])
})
