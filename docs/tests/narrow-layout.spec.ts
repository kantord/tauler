import { test, expect } from '@playwright/test'

// How the docs chrome (header, contents, article, "on this page") lays out
// below and above the 768px breakpoint. Below it the contents live in a
// drawer opened from a header menu button and "on this page" is a heading
// bar that sticks under the header; from 768px the contents are docked
// beside the article and there is no menu button.
//
// A long page with many headings, so there is something to scroll past.
const PAGE = '/docs/layout-file/'

// Viewport is set per test, independent of the project's own.
// oxlint-disable-next-line no-empty-pattern
test.beforeEach(({}, testInfo) => {
  test.skip(
    testInfo.project.name !== 'desktop',
    'viewport is set explicitly per test',
  )
})

test.describe('390px', () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 })
    await page.goto(PAGE)
    await page.waitForTimeout(150)
  })

  test('the contents are not stacked above the article', async ({ page }) => {
    const header = await page.getByRole('banner').boundingBox()
    const article = await page.getByRole('article').boundingBox()
    expect(header).not.toBeNull()
    expect(article).not.toBeNull()
    // A stacked sidebar would push the article a full screen down.
    expect(article!.y).toBeLessThan(header!.y + header!.height + 300)
    await expect(page.getByRole('navigation', { name: 'Docs' })).toBeHidden()
  })

  test('the menu button opens a drawer with the contents groups', async ({
    page,
  }) => {
    const header = page.getByRole('banner')
    const menu = header.getByRole('button', { name: 'Open menu' })
    await expect(menu).toBeVisible()
    const menuBox = (await menu.boundingBox())!
    // Right end of the header.
    const headerBox = (await header.boundingBox())!
    expect(menuBox.x + menuBox.width).toBeGreaterThanOrEqual(
      headerBox.x + headerBox.width - 24,
    )

    await menu.click()

    const drawer = page.getByRole('dialog')
    await expect(drawer).toBeVisible()
    const docs = drawer.getByRole('navigation', { name: 'Docs' })
    await expect(docs).toBeVisible()
    await expect(docs.getByText('Guide', { exact: true })).toBeVisible()
    await expect(
      docs.getByRole('link', { name: 'The layout file' }),
    ).toBeVisible()

    // The header stays visible behind the scrim and is not covered by the
    // drawer: the logo is still left of it. (Plain locators: an open modal
    // hides the page behind it from the accessibility tree, as it should.)
    await expect(page.locator('header')).toBeVisible()
    const logo = (await page
      .locator('header a[aria-label="tauler, home"]')
      .boundingBox())!
    const drawerBox = (await drawer.boundingBox())!
    expect(drawerBox.x).toBeGreaterThan(logo.x + logo.width)
  })

  test('"on this page" sticks directly under the header once scrolled past the title', async ({
    page,
  }) => {
    const header = page.getByRole('banner')
    const bar = page.getByRole('button', { name: 'On this page' })
    await expect(bar).toBeVisible()

    await page.evaluate(() => window.scrollTo(0, 1500))
    await page.waitForTimeout(250)

    // While stuck, the header drops to a 1px bottom line: 57px tall, and the
    // bar meets it exactly.
    const headerBox = (await header.boundingBox())!
    const barBox = (await bar.boundingBox())!
    expect(headerBox.y).toBe(0)
    expect(headerBox.height).toBe(57)
    expect(barBox.y).toBe(57)
  })
})

test.describe('768px', () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 768, height: 1024 })
    await page.goto(PAGE)
    await page.waitForTimeout(150)
  })

  test('the contents are docked beside the article', async ({ page }) => {
    const contents = (await page
      .getByRole('complementary', { name: 'Contents' })
      .boundingBox())!
    // The article's tile (its title bar included), not the text inside it.
    const article = (await page.getByRole('main').boundingBox())!
    // Same row: tops within a few pixels of each other, contents to the left.
    expect(Math.abs(contents.y - article.y)).toBeLessThan(8)
    expect(contents.x + contents.width).toBeLessThanOrEqual(article.x + 1)
  })

  test('there is no menu button', async ({ page }) => {
    await expect(page.getByRole('button', { name: 'Open menu' })).toBeHidden()
  })
})
