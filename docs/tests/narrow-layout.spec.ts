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

  test('"on this page" starts below the title and lead', async ({ page }) => {
    const lead = (await page.locator('article .lead').boundingBox())!
    const bar = (await page
      .getByRole('button', { name: 'On this page' })
      .boundingBox())!
    expect(bar.y).toBeGreaterThanOrEqual(lead.y + lead.height)
  })

  test('the search dialog sits 12px from the screen edges', async ({
    page,
  }) => {
    await page.getByRole('button', { name: /search/i }).click()
    const dialog = page.getByRole('dialog')
    await expect(dialog.getByRole('button', { name: /close/i })).toBeVisible()
    await expect
      .poll(async () => {
        const box = (await dialog.boundingBox())!
        const width = (await page.locator('header').boundingBox())!.width
        return [Math.round(box.x), Math.round(box.y), width - box.x - box.width]
      })
      .toEqual([12, 12, 12])
  })

  test('search results are a title, a path and a snippet', async ({ page }) => {
    await page.getByRole('button', { name: /search/i }).click()
    await page.getByRole('dialog').getByRole('combobox').fill('theme')
    const options = page.getByRole('dialog').getByRole('option')
    await expect(options.first()).toBeVisible()
    await expect(options.locator('pre, table, code')).toHaveCount(0)
    for (const option of (await options.all()).slice(0, 6))
      expect((await option.boundingBox())!.height).toBeLessThanOrEqual(130)
    await expect(
      options.filter({ hasText: 'Guide / Themes' }).first(),
    ).toBeAttached()
    // The query is marked where it matched.
    await expect(options.first().locator('mark').first()).toHaveText(/theme/i)
  })

  test('the page title scales with the screen', async ({ page }) => {
    // 8vw: a long word in the title fits a phone's column.
    await expect(page.locator('article h1')).toHaveCSS('font-size', '31.2px')
  })

  test('the heading bar is a keycap, with rounded corners', async ({
    page,
  }) => {
    await expect(page.getByRole('button', { name: 'On this page' })).toHaveCSS(
      'border-top-left-radius',
      '4px',
    )
  })

  for (const [what, button] of [
    ['drawer', 'Open menu'],
    ['search dialog', /search/i],
  ] as const) {
    test(`the ${what}'s scrim darkens the page in dark mode too`, async ({
      page,
    }) => {
      await page.emulateMedia({ colorScheme: 'dark' })
      await expect(page.locator('html')).toHaveClass(/dark/)
      await page.getByRole('button', { name: button }).click()
      await expect(page.getByRole('dialog')).toBeVisible()
      // The brightest channel of whatever fills the screen behind the dialog,
      // painted on a canvas so any CSS colour format reads the same.
      const brightest = await page.evaluate(() => {
        const scrim = [...document.querySelectorAll('body *')].find((el) => {
          const style = getComputedStyle(el)
          const box = el.getBoundingClientRect()
          return (
            style.position === 'fixed' &&
            box.height >= innerHeight &&
            box.x === 0 &&
            box.width > innerWidth / 2 &&
            style.backgroundColor !== 'rgba(0, 0, 0, 0)'
          )
        })!
        const canvas = document.createElement('canvas').getContext('2d')!
        canvas.fillStyle = getComputedStyle(scrim).backgroundColor
        canvas.fillRect(0, 0, 1, 1)
        const [r, g, b] = canvas.getImageData(0, 0, 1, 1).data
        return Math.max(r, g, b)
      })
      expect(brightest).toBeLessThan(40)
    })
  }

  test('the heading list has no gaps between its rows', async ({ page }) => {
    await page.getByRole('button', { name: 'On this page' }).click()
    const rows = page.locator('article li a[href^="#"]')
    const first = (await rows.nth(0).boundingBox())!
    const second = (await rows.nth(1).boundingBox())!
    expect(second.y).toBe(first.y + first.height)
    expect(first.x).toBe(16)
  })

  test('the drawer has a contents title, site links and both themes', async ({
    page,
  }) => {
    await page.getByRole('button', { name: 'Open menu' }).click()
    const drawer = page.getByRole('dialog')
    await expect(drawer.getByText('contents', { exact: true })).toBeVisible()
    await expect(drawer.getByText('Site', { exact: true })).toBeVisible()
    await expect(drawer.getByRole('link', { name: 'Home' })).toBeVisible()
    await drawer.getByRole('button', { name: 'Dark mode' }).click()
    await expect(page.locator('html')).toHaveClass(/dark/)
    await drawer.getByRole('button', { name: 'Light mode' }).click()
    await expect(page.locator('html')).toHaveClass(/light/)
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

  test('the "on this page" window gets a usable share of the column', async ({
    page,
  }) => {
    const window = (await page
      .getByRole('complementary', { name: 'On this page' })
      .boundingBox())!
    expect(window.height).toBeGreaterThan(180)
    expect(window.y + window.height).toBeLessThanOrEqual(1024)
  })

  test('the header offers "Star on GitHub" and gains an edge on scroll', async ({
    page,
  }) => {
    const header = page.getByRole('banner')
    await expect(
      header.getByRole('link', { name: 'Star on GitHub' }),
    ).toBeVisible()
    const edge = () =>
      page
        .locator('header')
        .evaluate((el) => getComputedStyle(el, '::after').height)
    expect(await edge()).not.toBe('4px')
    await page.evaluate(() => window.scrollTo(0, 600))
    await expect.poll(edge).toBe('4px')
  })

  test('the search dialog is 600px wide, 80px from the top', async ({
    page,
  }) => {
    await page.getByRole('button', { name: /search/i }).click()
    const dialog = page.getByRole('dialog')
    await expect
      .poll(async () => {
        const box = (await dialog.boundingBox())!
        return [Math.round(box.width), Math.round(box.y)]
      })
      .toEqual([600, 80])
  })

  test('there is no menu button', async ({ page }) => {
    await expect(page.getByRole('button', { name: 'Open menu' })).toBeHidden()
  })
})
