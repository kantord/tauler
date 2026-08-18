import { test, expect } from '@playwright/test'

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

test('visual regression', async ({ page }) => {
  await expect(page).toHaveScreenshot('landing.png')
})
