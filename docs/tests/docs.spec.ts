import {
  test,
  expect,
  type Page,
  type PlaywrightWorkerArgs,
  type TestInfo,
} from '@playwright/test'
import AxeBuilder from '@axe-core/playwright'

// Every project in playwright.config.ts is a different device breakpoint —
// these run at all seven, so a layout break at one width doesn't slip
// through untested.
const ROUTES = [
  '/',
  '/overview/',
  '/principles/',
  '/get-started/',
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

test('the header and the side tiles stay in view while the article scrolls', async ({
  page,
}) => {
  const width = page.viewportSize()?.width ?? 0
  test.skip(
    width < 768,
    'narrow screens stack the tiles; only the header sticks',
  )
  await page.goto('/docs/data/')
  await page.evaluate(() => window.scrollTo(0, 3000))
  await expect(page.getByRole('navigation', { name: 'Site' })).toBeInViewport()
  await expect(
    page.getByRole('complementary', { name: 'Contents' }),
  ).toBeInViewport()
})

test('the home page is the logo on the background, and nothing else', async ({
  page,
}) => {
  await page.goto('/')
  await expect(page.getByRole('heading', { level: 1 })).toHaveAccessibleName(
    'tauler',
  )
  await expect(page.locator('.tauler-wallpaper canvas')).toHaveCount(1)
  await expect(
    page.getByRole('link', { name: 'THREE.js PathTracing Renderer' }),
  ).toBeVisible()
  await expect(page.getByRole('contentinfo')).toHaveCount(0)
  await expect(page.locator('.tile')).toHaveCount(0)
  await expect(page.getByText(/^focus:/i)).toHaveCount(0)
  await expect(
    page
      .getByRole('navigation', { name: 'Workspaces' })
      .locator('[aria-current]'),
  ).toHaveCount(0)
})

const SLIDES = [
  { path: '/overview/', ws: '1, overview', windows: ['about', 'file'] },
  {
    path: '/principles/',
    ws: '2, principles',
    windows: ['principles', 'tick', 'streams', 'reload'],
  },
  {
    path: '/get-started/',
    ws: '3, get started',
    windows: ['get started', 'layout.op.mdx', 'install'],
  },
]

for (const slide of SLIDES) {
  test(`${slide.path} is its workspace's slide`, async ({ page }) => {
    await page.goto(slide.path)
    for (const name of slide.windows) {
      await expect(
        page.getByRole('button', { name, exact: true }),
      ).toBeVisible()
    }
    await expect(
      page
        .getByRole('navigation', { name: 'Workspaces' })
        .getByRole('link', { name: slide.ws }),
    ).toHaveAttribute('aria-current', 'page')
    await expect(page.getByText(/^focus:/i)).toHaveText(
      `focus: ${slide.windows[0]}`,
    )
    await expect(page.locator('.tauler-wallpaper canvas')).toHaveCount(1)
    await expect(page.getByRole('contentinfo')).toHaveCount(1)
  })
}

test('clicking a title bar on a slide moves focus to that window', async ({
  page,
}) => {
  await page.goto('/get-started/')
  // Hydration can lag the first paint; retry the click until it takes.
  await expect(async () => {
    await page.getByRole('button', { name: 'install' }).click()
    await expect(page.getByText(/^focus:\s*install$/i)).toBeVisible({
      timeout: 500,
    })
  }).toPass()
  await expect(page.locator('.sub.on')).toHaveCount(1)
  await expect(page.locator('.tile.on')).toHaveCount(1)
})

test('the workspace buttons move between the slides', async ({ page }) => {
  await page.goto('/')
  const workspaces = page.getByRole('navigation', { name: 'Workspaces' })
  await workspaces.getByRole('link', { name: '2, principles' }).click()
  await expect(page).toHaveURL(/\/principles\/$/)
  await expect(
    workspaces.getByRole('link', { name: '2, principles' }),
  ).toHaveAttribute('aria-current', 'page')
  await workspaces.getByRole('link', { name: '1, overview' }).click()
  await expect(page).toHaveURL(/\/overview\/$/)
})

test('previous and next walk the slides from home to the docs', async ({
  page,
}) => {
  await page.goto('/overview/')
  const step = (direction: 'previous' | 'next') =>
    page.getByRole('link', {
      name: new RegExp(
        `^${direction === 'previous' ? '← previous' : 'next →'}`,
      ),
    })
  await step('next').click()
  await expect(page).toHaveURL(/\/principles\/$/)
  await step('next').click()
  await expect(page).toHaveURL(/\/get-started\/$/)
  await step('previous').click()
  await expect(page).toHaveURL(/\/principles\/$/)
  await step('previous').click()
  await expect(page).toHaveURL(/\/overview\/$/)
  await step('previous').click()
  await expect(page).toHaveURL(/\/$/)
  await page.goto('/get-started/')
  await step('next').click()
  await expect(page).toHaveURL(/\/docs\/$/)
})

test('the wallpaper draws behind the page', async ({ page }) => {
  await page.goto('/docs/')
  const canvas = page.locator('.tauler-wallpaper canvas')
  await expect(canvas).toHaveCount(1)
  await expect
    .poll(() => canvas.evaluate((c: HTMLCanvasElement) => c.width))
    .toBeGreaterThan(0)
})

test('without hardware WebGL the home page falls back to the wallpaper', async ({
  page,
}, testInfo) => {
  test.skip(testInfo.project.name !== 'desktop', 'one width is enough')
  // Headless Chromium renders WebGL in software (SwiftShader), which the
  // gallery refuses: a path tracer there would freeze the page.
  await page.goto('/')
  await expect(page.locator('.tauler-wallpaper canvas')).toHaveCount(1)
  await expect(page.locator('[data-room] canvas')).toHaveCount(0)
  // no empty picture frame left where the render would have been
  await expect(page.locator('[data-room]')).toHaveCount(0)
})

type Box = { x: number; y: number; width: number; height: number }

/**
 * Opens `/` in a Chromium with the GPU on, in a window `width` x `height` CSS
 * px at device pixel ratio `dpr`, waits for the render to converge, hides the
 * page over the wallpaper and the render, and hands `check` the render's box
 * and a screenshot of it. Skips where there is no hardware WebGL 2 (CI has no
 * GPU). The ratio is forced on the command line: Playwright's
 * deviceScaleFactor does not reach the device-pixel-content-box sizes the
 * renderer lays its pixels out by.
 */
async function withConvergedRoom(
  playwright: PlaywrightWorkerArgs['playwright'],
  testInfo: TestInfo,
  { width, height, dpr = 1 }: { width: number; height: number; dpr?: number },
  check: (room: { page: Page; box: Box; png: Buffer }) => Promise<void>,
) {
  const browser = await playwright.chromium.launch({
    args: [
      '--use-angle=gl',
      '--enable-gpu',
      '--ignore-gpu-blocklist',
      `--force-device-scale-factor=${dpr}`,
      `--window-size=${width},${height}`,
    ],
  })
  try {
    const page = await browser.newPage({
      baseURL: testInfo.project.use.baseURL,
      viewport: null,
    })
    await page.goto('/')
    const hardware = await page.evaluate(
      () =>
        !!document
          .createElement('canvas')
          .getContext('webgl2', { failIfMajorPerformanceCaveat: true })
          ?.getExtension('EXT_color_buffer_float'),
    )
    test.skip(!hardware, 'no hardware WebGL 2 here (CI has no GPU)')
    const room = page.locator('[data-room] canvas')
    // Shaders compile first; slow while other GPU tests run alongside.
    await expect(room).toHaveCount(1, { timeout: 30_000 })
    // The image converges and the loop stops.
    await expect(page.locator('[data-room][data-converged]')).toHaveCount(1, {
      timeout: 30_000,
    })
    const box = (await room.boundingBox())!
    await page.addStyleTag({
      content: `body * { visibility: hidden }
        .tauler-wallpaper, .tauler-wallpaper *, [data-room], [data-room] * {
          visibility: visible
        }`,
    })
    await check({ page, box, png: await page.screenshot({ clip: box }) })
  } finally {
    await browser.close()
  }
}

/**
 * Mean brightness and mean chroma (max - min channel, 0-255) of rectangles of
 * a PNG, each given as fractions of its size: [x0, y0, x1, y1].
 */
function measure(page: Page, png: Buffer, rects: number[][]) {
  return page.evaluate(
    async ({ data, rects }) => {
      const image = new Image()
      image.src = `data:image/png;base64,${data}`
      await image.decode()
      const canvas = new OffscreenCanvas(image.width, image.height)
      const context = canvas.getContext('2d')!
      context.drawImage(image, 0, 0)
      return rects.map(([x0, y0, x1, y1]) => {
        const x = Math.floor(x0 * image.width),
          y = Math.floor(y0 * image.height)
        const { data: px } = context.getImageData(
          x,
          y,
          Math.max(1, Math.floor(x1 * image.width) - x),
          Math.max(1, Math.floor(y1 * image.height) - y),
        )
        let brightness = 0,
          chroma = 0
        for (let i = 0; i < px.length; i += 4) {
          brightness += (px[i] + px[i + 1] + px[i + 2]) / 3
          chroma +=
            Math.max(px[i], px[i + 1], px[i + 2]) -
            Math.min(px[i], px[i + 1], px[i + 2])
        }
        const n = px.length / 4
        return { brightness: brightness / n, chroma: chroma / n }
      })
    },
    { data: png.toString('base64'), rects },
  )
}

test('with a GPU the home page path-traces the white room, not a black screen', async ({
  playwright,
}, testInfo) => {
  test.skip(testInfo.project.name !== 'desktop', 'one width is enough')
  await withConvergedRoom(
    playwright,
    testInfo,
    { width: 1440, height: 900 },
    async ({ page, box, png }) => {
      // The standard wallpaper fills the window behind the render.
      await expect(page.locator('.tauler-wallpaper canvas')).toHaveCount(1)
      // The render fills the bottom half of the window, edge to edge (the
      // page drops its scrollbar gutter for it), so the wallpaper shows above.
      const window = await page.evaluate(() => ({
        width: innerWidth,
        height: innerHeight,
      }))
      expect(box.x).toBe(0)
      expect(box.width).toBe(window.width)
      expect(Math.abs(box.height - window.height / 2)).toBeLessThanOrEqual(1)
      expect(Math.abs(box.y + box.height - window.height)).toBeLessThan(1)
      // A NaN in the accumulation buffer would keep it black however long it
      // ran. Below the fade-in at its top (ROOM_FADE), where the wallpaper
      // shows through.
      const [unfaded] = await measure(page, png, [[0, 0.2, 1, 1]])
      expect(unfaded.brightness).toBeGreaterThan(30)
    },
  )
})

// The render's dither pixels are the wallpaper's: 3 device px at any device
// pixel ratio, on one grid counted from the window's top-left. So in a
// screenshot of the two (in device px) colour changes only ever happen at
// multiples of 3 from the origin, and at odd multiples too (pixels of 3, not
// 6). Down the fade-in at the render's top (ROOM_FADE) the blend changes every
// row, so rows are checked above and below it. 803 CSS px puts the window's
// bottom edge, where the page's pixel pass counts from, off the grid at each
// of these ratios (803, 1004, 1205, 1606 device px).
for (const dpr of [1, 1.25, 1.5, 2]) {
  test(`with a GPU the render's pixels sit on the wallpaper's grid at ${dpr}x`, async ({
    playwright,
  }, testInfo) => {
    test.skip(testInfo.project.name !== 'desktop', 'one width is enough')
    await withConvergedRoom(
      playwright,
      testInfo,
      { width: 1280, height: 803, dpr },
      async ({ page, box }) => {
        const png = await page.screenshot({ scale: 'device' })
        const grid = await page.evaluate(
          async ({ data, top, fadeEnd }) => {
            const image = new Image()
            image.src = `data:image/png;base64,${data}`
            await image.decode()
            const { width: w, height: h } = image
            const canvas = new OffscreenCanvas(w, h)
            const context = canvas.getContext('2d')!
            context.drawImage(image, 0, 0)
            const px = new Uint32Array(
              context.getImageData(0, 0, w, h).data.buffer,
            )
            let offGrid = 0,
              oddInRender = 0
            for (let y = 0; y < h - 1; y++)
              for (let x = 0; x < w - 1; x++) {
                const p = px[y * w + x]
                if (p !== px[y * w + x + 1]) {
                  if ((x + 1) % 3) offGrid++
                  else if (y > top && (x + 1) % 6 === 3) oddInRender++
                }
                const fading = y + 1 >= top && y < fadeEnd
                if (p !== px[(y + 1) * w + x] && (y + 1) % 3 && !fading)
                  offGrid++
              }
            return { offGrid, oddInRender }
          },
          {
            data: png.toString('base64'),
            top: Math.floor(box.y * dpr),
            fadeEnd: Math.ceil((box.y + box.height * 0.2) * dpr) + 1,
          },
        )
        expect(grid.offGrid).toBe(0)
        expect(grid.oddInRender).toBeGreaterThan(1000)
      },
    )
  })
}

// The scene is a room with no ceiling and no left wall, built for a 3:2
// frame; the frame here runs from about 0.9:1 to 12:1. Where the camera sees
// past the walls, rays escape to the backdrop, a flat near-white (chroma
// under ~20); the walls and floor carry the colour cells (chroma 50+). So
// the frame's edges must be colourful: the side columns below the window
// band on wide frames, the top rows on a tall one, looked at without the
// fade-in that would let the wallpaper through there.
for (const { viewport, edges } of [
  { viewport: { width: 3440, height: 1440 }, edges: 'sides' },
  { viewport: { width: 3840, height: 640 }, edges: 'sides' },
  { viewport: { width: 390, height: 844 }, edges: 'top' },
] as const) {
  test(`with a GPU the room has no empty space at ${viewport.width}x${viewport.height}`, async ({
    playwright,
  }, testInfo) => {
    test.skip(testInfo.project.name !== 'desktop', 'one width is enough')
    await withConvergedRoom(
      playwright,
      testInfo,
      viewport,
      async ({ page, box }) => {
        await page.addStyleTag({
          content: `[data-room] canvas {
            mask-image: none !important;
            -webkit-mask-image: none !important;
          }`,
        })
        const png = await page.screenshot({ clip: box })
        const sx = 8 / box.width,
          sy = 8 / box.height
        const rects =
          edges === 'sides'
            ? [
                [0, 0.4, sx, 1],
                [1 - sx, 0.4, 1, 1],
              ]
            : [[0, 0, 1, sy]]
        for (const edge of await measure(page, png, rects))
          expect(edge.chroma).toBeGreaterThan(35)
      },
    )
  })
}

test('a ratio in prose stays literal text, not a directive', async ({
  page,
}) => {
  // remark-directive reads the `:1` in "3:1" as a text directive, which
  // renders as a <div> inside the <p>: invalid nesting and a hydration error.
  const nestingErrors: string[] = []
  page.on('console', (message) => {
    if (
      message.type() === 'error' &&
      message.text().includes('cannot be a descendant')
    ) {
      nestingErrors.push(message.text())
    }
  })
  await page.goto('/docs/themes/')
  const article = page.locator('article')
  await expect(
    article.locator('p', { hasText: 'aim for at least' }),
  ).toContainText('3:1')
  await expect(article.locator('p div')).toHaveCount(0)
  expect(nestingErrors).toEqual([])
})

test('the sitemap lists the slides', async ({ request }) => {
  const xml = await (await request.get('/sitemap.xml')).text()
  for (const path of ['/overview/', '/principles/', '/get-started/']) {
    expect(xml).toContain(`https://tauler.dev${path}<`)
  }
})
