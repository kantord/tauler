import { test, expect } from '@playwright/test'
import AxeBuilder from '@axe-core/playwright'

// Covers the docs revamp's currently-restored features: the layer-order
// fix, layout variables, the slab/wallpaper frame, and the header matching
// the landing page. Section-identity (per-group accents), the OG-image
// endpoint, and the docs-index content rewrite are separate decisions not
// yet made — their tests live in the git stash alongside that code, ready
// to come back together if those decisions are.

// Every project in playwright.config.ts is a different device breakpoint —
// these run at all seven, same pattern as landing.spec.ts, so a docs-page
// layout break at one specific width doesn't slip through untested.
const ALL_COVERAGE_ROUTES = [
  '/docs/',
  '/docs/layout-file/',
  '/docs/elements/',
  '/docs/data/',
  '/docs/layout/',
  '/docs/components/',
  '/docs/macos/',
]

test.describe('cross-device layout', () => {
  for (const route of ALL_COVERAGE_ROUTES) {
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
        // Expressive Code's own scrollable-block tabindex plugin applies
        // asynchronously (a debounced ResizeObserver per block) — real and
        // correct, not a bug, but axe run immediately after navigation
        // would race it. Wait for the actual condition instead of a
        // duration (a fixed wait flaked under load on content-heavy pages).
        await page.waitForFunction(() =>
          [...document.querySelectorAll('.expressive-code pre')].every(
            (pre) =>
              pre.scrollWidth <= pre.clientWidth ||
              pre.hasAttribute('tabindex'),
          ),
        )
        const results = await new AxeBuilder({ page }).analyze()
        expect(
          results.violations,
          JSON.stringify(results.violations, null, 2),
        ).toEqual([])
      })

      test('sidebar content is reachable', async ({ page }) => {
        await page.goto(route)
        // Below 50em (Starlight's own breakpoint) the sidebar hides behind
        // the menu button; above it, it's on screen without interaction.
        const width = page.viewportSize()?.width ?? 0
        if (width < 800) {
          await page.locator('starlight-menu-button button').click()
        }
        await expect(page.locator('#starlight__sidebar')).toBeVisible()
      })
    })
  }
})

test.describe('layer order', () => {
  test('no computed border-radius other than 0px on a docs page', async ({
    page,
  }) => {
    await page.goto('/docs/layout-file/')
    await page.evaluate(() => document.fonts.ready)

    const offenders = await page.evaluate(() => {
      const bad: string[] = []
      for (const el of document.querySelectorAll('*')) {
        const radius = getComputedStyle(el).borderRadius
        if (radius !== '0px' && !/^(0px\s*)+$/.test(radius)) {
          bad.push(`${el.tagName}.${el.className || ''}: ${radius}`)
        }
      }
      return bad
    })
    expect(offenders).toEqual([])
  })

  test('search modal has no rounded corners when open', async ({ page }) => {
    await page.goto('/docs/layout-file/')
    await page.getByLabel('Search', { exact: true }).first().click()
    const dialog = page.locator('site-search dialog')
    await expect(dialog).toBeVisible()
    const radius = await dialog.evaluate(
      (el) => getComputedStyle(el).borderRadius,
    )
    expect(radius).toBe('0px')
  })
})

test.describe('the desktop canvas', () => {
  test('draws above 1700px and stays undrawn below it on a regular docs page', async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1440, height: 900 })
    await page.goto('/docs/layout-file/')
    await page.waitForTimeout(100)
    const narrowWidth = await page
      .locator('[data-desktop-canvas]')
      .evaluate((el: HTMLCanvasElement) => el.width)
    // Never painted: stays at the element's intrinsic default.
    expect(narrowWidth).toBeLessThanOrEqual(300)

    await page.setViewportSize({ width: 1920, height: 1080 })
    await page.evaluate(() => window.dispatchEvent(new Event('resize')))
    await page.waitForTimeout(350)
    const wideWidth = await page
      .locator('[data-desktop-canvas]')
      .evaluate((el: HTMLCanvasElement) => el.width)
    expect(wideWidth).toBeGreaterThan(300)
  })
})

test.describe('the header', () => {
  test('hairline spans the full viewport above 1600px', async ({ page }) => {
    await page.setViewportSize({ width: 1920, height: 1080 })
    await page.goto('/docs/layout-file/')
    const box = await page.locator('.header').first().boundingBox()
    expect(box?.width).toBe(1920)
  })

  test('theme stays dark', async ({ page }) => {
    await page.goto('/docs/layout-file/')
    const theme = await page.evaluate(
      () => document.documentElement.dataset.theme,
    )
    expect(theme).toBe('dark')
  })
})

test.describe('the sidebar on a large monitor', () => {
  // Starlight's own .sidebar-pane is `position: fixed; inset-inline-start:
  // 0` — hardcoded to the true viewport edge, not the slab's. .page's
  // max-width+auto-margin centering (docs-shell.css) never reaches a
  // fixed-position descendant, so above the 1600px cap the sidebar used to
  // hug the real screen edge while the rest of the page centered around
  // it — a real, previously-uncaught bug on any monitor wider than 1600px.
  test('stays aligned with the centered slab, not the true viewport edge', async ({
    page,
  }) => {
    await page.setViewportSize({ width: 2560, height: 1080 })
    await page.goto('/docs/layout-file/')
    const pageLeft = await page
      .locator('.page')
      .evaluate((el) => el.getBoundingClientRect().x)
    const sidebarLeft = await page
      .locator('.sidebar-pane')
      .evaluate((el) => el.getBoundingClientRect().x)
    expect(pageLeft).toBeGreaterThan(0) // sanity: the slab really is centered here
    expect(sidebarLeft).toBe(pageLeft)
  })
})

test.describe('narrow viewports', () => {
  test.use({ viewport: { width: 390, height: 844 } })

  // Sidebar reachability itself is covered generically for every
  // breakpoint by "cross-device layout" above; this is specifically the
  // mobile search affordance.
  test('search opens', async ({ page }) => {
    await page.goto('/docs/layout-file/')
    await page.getByLabel('Search', { exact: true }).first().click()
    await expect(page.locator('site-search dialog')).toBeVisible()
  })
})
