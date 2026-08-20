import { test, expect, request as pwRequest } from '@playwright/test'

// Generic, content-agnostic checks for a class of bug that scrollWidth-based
// overflow checks structurally cannot catch: an element that clips its own
// overflow (correctly, by design — that's what stops the whole page from
// scrolling) can still be silently truncating text mid-glyph with no
// ellipsis, and a flex/grid item that refuses to shrink can force a sibling
// into an absurdly narrow column that wraps one character per line instead
// of overflowing. Neither trips a `scrollWidth > innerWidth` check on the
// page — this file catches them by their own signature instead: clipped
// text is fine when it's *indicated* (ellipsis), not when it's silent; a
// paragraph taller than its character count could plausibly justify is a
// squeeze, not real content, regardless of what that content says.
//
// Real bugs this shape caught before it existed: the header wordmark
// clipping to "TAULI" at 375px (silent clip, no ellipsis), and a
// custom-titled aside on /docs/macos/ wrapping its body one character per
// line on mobile. Both were invisible to the existing overflow/a11y sweep.
//
// Routes are discovered from the sitemap, not hardcoded, so a page added
// or removed later doesn't need this file edited to stay in sync.

const REALISTIC_PHONE_WIDTHS = [320, 375, 390, 414, 430]
// Matches playwright.config.ts's `use.baseURL` — fetched here at module
// load, outside any test's `page`/`request` fixture, so routes can be
// turned into one test() per (route, width) pair up front.
const BASE_URL = 'http://127.0.0.1:4321'

async function discoverRoutes(): Promise<string[]> {
  const ctx = await pwRequest.newContext({ baseURL: BASE_URL })
  try {
    const indexXml = await (await ctx.get('/sitemap-index.xml')).text()
    const sitemapLoc = indexXml.match(/<loc>(.*?)<\/loc>/)?.[1]
    const sitemapPath = sitemapLoc
      ? new URL(sitemapLoc).pathname
      : '/sitemap-0.xml'
    const xml = await (await ctx.get(sitemapPath)).text()
    return [...xml.matchAll(/<loc>(.*?)<\/loc>/g)].map(
      (m) => new URL(m[1]).pathname,
    )
  } finally {
    await ctx.dispose()
  }
}

const routes = await discoverRoutes()

for (const route of routes) {
  test.describe(route, () => {
    // Viewport is set per-test below, independent of the project's own —
    // running this under all 7 breakpoint projects would just repeat the
    // same 5 widths seven times for no benefit.
    test.beforeEach(({}, testInfo) => {
      test.skip(
        testInfo.project.name !== 'desktop',
        'viewport is set explicitly per test',
      )
    })

    for (const width of REALISTIC_PHONE_WIDTHS) {
      test(`no unindicated text clipping in the header/sidebar @ ${width}px`, async ({
        page,
      }) => {
        await page.setViewportSize({ width, height: 800 })
        await page.goto(route)
        await page.waitForTimeout(150)

        const offenders = await page.evaluate(() => {
          const bad: string[] = []
          for (const el of document.querySelectorAll<HTMLElement>(
            'header *:not(.sr-only), .sidebar-content a:not(.sr-only)',
          )) {
            if (el.children.length > 0) continue // leaf text-bearing elements only
            const text = el.textContent?.trim()
            if (!text) continue
            const clipped = el.scrollWidth > el.clientWidth + 1
            const indicated = getComputedStyle(el).textOverflow === 'ellipsis'
            // An ellipsis makes clipping *legible* (the user sees it
            // happened) but not automatically fine — "T…" for "TAULER" is
            // still the brand rendering as noise. Only accept an ellipsis
            // that's trimming a minority of the content; a clip that eats
            // most of it is the same bug with a nicer character at the end.
            const trimmedMost = el.clientWidth < el.scrollWidth * 0.5
            if (clipped && (!indicated || trimmedMost)) {
              bad.push(
                `${el.tagName}.${el.className || '(no class)'}: "${text}" — scrollWidth ${el.scrollWidth} > clientWidth ${el.clientWidth}` +
                  (indicated
                    ? ' (ellipsis, but trimmed >50%)'
                    : ' (no ellipsis)'),
              )
            }
          }
          return bad
        })

        expect(offenders).toEqual([])
      })

      test(`no text block wraps into a pathologically tall column @ ${width}px`, async ({
        page,
      }) => {
        await page.setViewportSize({ width, height: 800 })
        await page.goto(route)
        await page.waitForTimeout(150)

        const offenders = await page.evaluate(() => {
          // Real prose, at any column width a phone can offer, never gets
          // remotely close to this many pixels per character — a
          // one-character-per-line collapse (a grid/flex column crushed to
          // near-zero) does, by roughly a line-height's worth per char.
          const PX_PER_CHAR_CEILING = 5
          const MIN_TEXT_LENGTH = 20 // skip short labels/badges — noisy, not the failure mode
          const bad: string[] = []
          for (const el of document.querySelectorAll<HTMLElement>(
            'p, li, dd, blockquote',
          )) {
            const text = el.textContent?.trim() ?? ''
            if (text.length < MIN_TEXT_LENGTH) continue
            const height = el.getBoundingClientRect().height
            const ratio = height / text.length
            if (ratio > PX_PER_CHAR_CEILING) {
              bad.push(
                `${el.tagName}: ${Math.round(height)}px for ${text.length} chars (${ratio.toFixed(1)}px/char) — "${text.slice(0, 50)}..."`,
              )
            }
          }
          return bad
        })

        expect(offenders).toEqual([])
      })
    }
  })
}
