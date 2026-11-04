import { readFileSync } from 'node:fs'
import {
  test,
  expect,
  type Page,
  type PlaywrightWorkerArgs,
  type TestInfo,
} from '@playwright/test'
import AxeBuilder from '@axe-core/playwright'
import {
  oklchToRgb,
  rgbToOklab,
} from '../src/lib/wallpaper/tauler-wallpaper.js'
import { WALLPAPER } from '../src/components/site/wallpaper/options'
import {
  ROOM_EASE_IN,
  ROOM_WAVE,
} from '../src/components/site/whiteroom/room-size'

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
  // room refuses: a path tracer there would freeze the page. The wallpaper
  // alone is drawn, on the CPU (page-wallpaper.ts), and nothing is left
  // where the room would have been.
  await page.goto('/')
  await expect(page.locator('.tauler-wallpaper canvas')).toHaveCount(1)
  await expect(page.locator('.tauler-wallpaper')).toHaveAttribute(
    'data-renderer',
    '2d',
  )
  await expect(page.locator('[data-room]')).toHaveCount(0)
})

type Box = { x: number; y: number; width: number; height: number }

/**
 * Opens `path` (the home page by default) in a Chromium with the GPU on, in a
 * window `width` x `height` CSS px at device pixel ratio `dpr`, in the
 * site's `theme`, waits for the room to converge, hides the page over the
 * background, and hands `check` the room's box (its visible part, the
 * bottom of the first screen) and a screenshot of it. Skips where there is
 * no hardware WebGL 2 (CI has no GPU). The ratio is forced on the command
 * line: Playwright's deviceScaleFactor does not reach the
 * device-pixel-content-box sizes the wallpaper lays its pixels out by.
 * `fade: false` turns the room's fades into the wallpaper off
 * (window.taulerRoomFade), so a test sees the room's own edges. `samples`
 * stops the image at that many samples (window.taulerRoomSamples), so a
 * test sees the room as it fades in.
 */
async function withConvergedRoom(
  playwright: PlaywrightWorkerArgs['playwright'],
  testInfo: TestInfo,
  {
    width,
    height,
    dpr = 1,
    theme = 'light',
    fade = true,
    path = '/',
    samples,
  }: {
    width: number
    height: number
    dpr?: number
    theme?: 'light' | 'dark'
    fade?: boolean
    path?: string
    samples?: number
  },
  check: (room: { page: Page; box: Box; png: Buffer }) => Promise<void>,
) {
  // the room's shaders compile in seconds on a cold cache, and the checks
  // read whole screens of pixels
  test.setTimeout(120_000)
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
    // next-themes reads the theme from localStorage before the first paint
    await page.addInitScript(
      ({ theme, fade, samples }) => {
        localStorage.setItem('theme', theme)
        if (samples !== undefined) window.taulerRoomSamples = samples
        if (!fade)
          window.taulerRoomFade = {
            L: 1e-4,
            C: 1e-4,
            h: 1e-4,
            depth: 0,
            match: 1e-4,
            detail: 1e-4,
          }
      },
      { theme, fade, samples },
    )
    await page.goto(path)
    const hardware = await page.evaluate(
      () =>
        !!document
          .createElement('canvas')
          .getContext('webgl2', { failIfMajorPerformanceCaveat: true })
          ?.getExtension('EXT_color_buffer_float'),
    )
    test.skip(!hardware, 'no hardware WebGL 2 here (CI has no GPU)')
    const room = page.locator('.tauler-wallpaper [data-room]')
    // Shaders compile first; slow while other GPU tests run alongside.
    await expect(room).toHaveCount(1, { timeout: 30_000 })
    // The image converges and the loop stops.
    await expect(room).toHaveAttribute('data-converged', /\d+/, {
      timeout: 30_000,
    })
    const box = (await room.boundingBox())!
    await page.addStyleTag({
      content: `body * { visibility: hidden }
        .tauler-wallpaper, .tauler-wallpaper * { visibility: visible }`,
    })
    await check({ page, box, png: await page.screenshot({ clip: box }) })
  } finally {
    await browser.close()
  }
}

/**
 * Runs `analyse` in the page on the pixels of each PNG (decoded there: an
 * array of millions of bytes is slow to hand back to the test), with `args`,
 * and returns what it returns. `analyse` is a function's source, called as
 * (images, args) with images [{ width, height, px: Uint32Array }] (RGBA
 * bytes, one 32-bit value per pixel).
 */
function inPage<T>(
  page: Page,
  pngs: Buffer[],
  analyse: string,
  args: unknown = null,
): Promise<T> {
  return page.evaluate(
    async ({ data, analyse, args }) => {
      const images = await Promise.all(
        data.map(async (src) => {
          const image = new Image()
          image.src = `data:image/png;base64,${src}`
          await image.decode()
          const canvas = new OffscreenCanvas(image.width, image.height)
          const context = canvas.getContext('2d')!
          context.drawImage(image, 0, 0)
          const { data: rgba } = context.getImageData(
            0,
            0,
            image.width,
            image.height,
          )
          return {
            width: image.width,
            height: image.height,
            px: new Uint32Array(rgba.buffer),
          }
        }),
      )
      // eslint-disable-next-line @typescript-eslint/no-implied-eval
      return new Function(`return (${analyse})`)()(images, args)
    },
    { data: pngs.map((png) => png.toString('base64')), analyse, args },
  ) as Promise<T>
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
      // One canvas draws the wallpaper and the room.
      await expect(page.locator('.tauler-wallpaper canvas')).toHaveCount(1)
      // The room fills the bottom half of the window, edge to edge (the
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
      // ran. Below the fade into the wallpaper at its top (ROOM_FADE).
      const [unfaded] = await measure(page, png, [[0, 0.6, 1, 1]])
      expect(unfaded.brightness).toBeGreaterThan(30)
    },
  )
})

// The room's cells are the wallpaper's: 3 device px at any device pixel
// ratio, on the page's grid, because the wallpaper's shader draws both. So
// in a screenshot (in device px) colour changes only ever happen at
// multiples of 3 from the page's top-left, and at odd multiples too (cells
// of 3, not 6), across the room's fade into the wallpaper as everywhere else.
// 803 CSS px puts the window's bottom edge off the grid at each of these
// ratios (803, 1004, 1205, 1606 device px).
for (const dpr of [1, 1.25, 1.5, 2]) {
  test(`with a GPU the room's pixels sit on the wallpaper's grid at ${dpr}x`, async ({
    playwright,
  }, testInfo) => {
    test.skip(testInfo.project.name !== 'desktop', 'one width is enough')
    await withConvergedRoom(
      playwright,
      testInfo,
      { width: 1280, height: 803, dpr },
      async ({ page, box }) => {
        const grid = await onGrid(page, Math.floor(box.y * dpr))
        expect(grid.offGrid).toBe(0)
        expect(grid.oddInRoom).toBeGreaterThan(1000)
      },
    )
  })
}

/**
 * Colour changes between neighbouring device pixels of the window that are
 * not on the page's 3-px grid, and changes at odd multiples of 3 below
 * `top` (device px).
 */
async function onGrid(page: Page, top: number) {
  const scroll = await page.evaluate(() =>
    Math.round(scrollY * devicePixelRatio),
  )
  return inPage<{ offGrid: number; oddInRoom: number }>(
    page,
    [await page.screenshot({ scale: 'device' })],
    `([{ width: w, height: h, px }], { top, scroll }) => {
      let offGrid = 0, oddInRoom = 0
      for (let y = 0; y < h - 1; y++)
        for (let x = 0; x < w - 1; x++) {
          const p = px[y * w + x]
          if (p !== px[y * w + x + 1]) {
            if ((x + 1) % 3) offGrid++
            else if (y > top && (x + 1) % 6 === 3) oddInRoom++
          }
          if (p !== px[(y + 1) * w + x] && (scroll + y + 1) % 3) offGrid++
        }
      return { offGrid, oddInRoom }
    }`,
    { top, scroll },
  )
}

// The room is anchored to the page like the wallpaper: on a slide that
// scrolls, it scrolls with the content, cell for cell. With the page's
// content hidden, the window shows the same page rows of background
// wherever it is scrolled to, byte for byte, the room's included, and
// everything stays on the page's grid at a fractional ratio too.
for (const { dpr, scroll } of [
  { dpr: 1, scroll: 211 },
  { dpr: 1.25, scroll: 317.6 },
]) {
  test(`with a GPU the room scrolls with a slide, at ${dpr}x`, async ({
    playwright,
  }, testInfo) => {
    test.skip(testInfo.project.name !== 'desktop', 'one width is enough')
    await withConvergedRoom(
      playwright,
      testInfo,
      { width: 390, height: 844, dpr, theme: 'dark', path: '/overview/' },
      async ({ page }) => {
        const at = () =>
          page.evaluate(() => Math.round(scrollY * devicePixelRatio))
        const before = {
          at: await at(),
          png: await page.screenshot({ scale: 'device' }),
        }
        await page.evaluate((y) => window.scrollTo(0, y), scroll)
        // until the wallpaper is drawn where the window now is, and shown
        await expect
          .poll(() =>
            page.evaluate(() => {
              const host =
                document.querySelector<HTMLElement>('.tauler-wallpaper')!
              const [top, height] = (host.dataset.drawn ?? '')
                .split(' ')
                .map(Number)
              const view = scrollY * devicePixelRatio
              return (
                scrollY > 0 &&
                top <= view &&
                top + height >= view + innerHeight * devicePixelRatio
              )
            }),
          )
          .toBe(true)
        await page.evaluate(
          () =>
            new Promise((done) =>
              requestAnimationFrame(() => requestAnimationFrame(done)),
            ),
        )
        const after = {
          at: await at(),
          png: await page.screenshot({ scale: 'device' }),
        }
        // the same page rows, before and after
        const shift = after.at - before.at
        expect(shift).toBeGreaterThan(200 * dpr)
        const differ = await inPage<number>(
          page,
          [before.png, after.png],
          `([a, b], shift) => {
            let differ = 0
            for (let y = shift; y < a.height; y++)
              for (let x = 0; x < a.width; x++)
                if (a.px[y * a.width + x] !== b.px[(y - shift) * a.width + x]) differ++
            return differ
          }`,
          shift,
        )
        expect(differ).toBe(0)
        const room = (await page
          .locator('.tauler-wallpaper [data-room]')
          .boundingBox())!
        const grid = await onGrid(page, Math.floor(room.y * dpr))
        expect(grid.offGrid).toBe(0)
      },
    )
  })
}

// The camera never looks so high as to see the window (a light, near white in
// both themes; at the room's top it was a hard bright band in the dark
// theme) and the scene, built for a 3:2 frame, runs on far enough for frames
// from about 0.9:1 to 12:1 (scripts/whiteroom-scene.patch). Where the camera
// saw past the walls, rays would escape to the sky, near white in both
// themes, while the walls and floor take the page colour. So in the dark
// theme the room's edges must be dark: the side columns on wide frames, the
// top rows on all of them, looked at without the fade that would put the
// wallpaper there. (Measuring the colour cells instead would test the
// backdrop preset, not the geometry.)
for (const viewport of [
  { width: 3440, height: 1440 },
  { width: 3840, height: 640 },
  { width: 1440, height: 900 },
  { width: 390, height: 844 },
]) {
  test(`with a GPU the room has no empty space or window band at ${viewport.width}x${viewport.height}`, async ({
    playwright,
  }, testInfo) => {
    test.skip(testInfo.project.name !== 'desktop', 'one width is enough')
    await withConvergedRoom(
      playwright,
      testInfo,
      { ...viewport, theme: 'dark', fade: false },
      async ({ page, box, png }) => {
        const sx = 8 / box.width,
          sy = 8 / box.height
        const rects = [
          [0, 0.4, sx, 1],
          [1 - sx, 0.4, 1, 1],
          // the top tenth, a few rows at a time
          ...Array.from({ length: 6 }, (_, i) => [0, i * sy, 1, (i + 1) * sy]),
        ]
        for (const edge of await measure(page, png, rects))
          expect(edge.brightness).toBeLessThan(SKY_BRIGHTNESS)
      },
    )
  })
}

/**
 * Brightness the dark theme's room never reaches over a band of walls or
 * floor, and the sky and the window (near white) always pass.
 */
const SKY_BRIGHTNESS = 128

const MODULE = readFileSync(
  new URL('../src/lib/wallpaper/tauler-wallpaper.js', import.meta.url),
  'utf8',
)

// The room's fades start along a wave (ROOM_WAVE): above its highest point
// every channel is the wallpaper's, so those cells are the wallpaper's own
// computation, and the screen there is computeWallpaper()'s pixels for the
// same rows, byte for byte (the room's edge is the wallpaper by
// construction, one shader drawing both). Below it, where the room first
// shows in each column is not a straight line: across the width it moves by
// a good share of the room's height.
for (const { theme, width, height, dpr } of [
  { theme: 'light', width: 1280, height: 803, dpr: 1.25 },
  { theme: 'dark', width: 1440, height: 900, dpr: 1 },
  { theme: 'dark', width: 390, height: 844, dpr: 1 },
] as const) {
  test(`with a GPU the room starts along a wave, the wallpaper's own pixels above it, at ${width}x${height} in the ${theme} theme`, async ({
    playwright,
  }, testInfo) => {
    test.skip(testInfo.project.name !== 'desktop', 'one width is enough')
    await withConvergedRoom(
      playwright,
      testInfo,
      { width, height, dpr, theme },
      async ({ page, box }) => {
        const png = await page.screenshot({ scale: 'device' })
        await page.addScriptTag({
          type: 'module',
          content: `${MODULE}\nObject.assign(window, { computeWallpaper, readCssColors })`,
        })
        await page.waitForFunction(() => 'computeWallpaper' in window)
        const result = await page.evaluate(
          async ({ data, options, box }) => {
            const image = new Image()
            image.src = `data:image/png;base64,${data}`
            await image.decode()
            const canvas = new OffscreenCanvas(image.width, image.height)
            const context = canvas.getContext('2d')!
            context.drawImage(image, 0, 0)
            const shot = new Uint32Array(
              context.getImageData(0, 0, image.width, image.height).data.buffer,
            )
            const w = window as unknown as {
              computeWallpaper: typeof import('../src/lib/wallpaper/tauler-wallpaper.js').computeWallpaper
              readCssColors: typeof import('../src/lib/wallpaper/tauler-wallpaper.js').readCssColors
            }
            const host =
              document.querySelector<HTMLElement>('.tauler-wallpaper')!
            const dpr = devicePixelRatio
            const devP = 3
            const height = Math.floor((box.y + box.height) * dpr)
            const expected = w.computeWallpaper({
              ...options,
              ...w.readCssColors(host),
              dpr,
              width: image.width / dpr,
              height: height / dpr,
              deviceWidth: image.width,
              deviceHeight: height,
            })
            // each column's first device row that is not the wallpaper's
            const first = Array.from({ length: image.width }, () => height)
            for (let x = 0; x < image.width; x++)
              for (let y = 0; y < height; y++)
                if (
                  shot[y * image.width + x] !==
                  expected.pixels[
                    ((y / devP) | 0) * expected.cols + ((x / devP) | 0)
                  ]
                ) {
                  first[x] = y
                  break
                }
            // the earliest of each 10 cells, across the width
            const blocks: number[] = []
            for (let x = 0; x < image.width; x += 10 * devP)
              blocks.push(Math.min(...first.slice(x, x + 10 * devP)))
            return {
              highest: Math.min(...first),
              spread: Math.max(...blocks) - Math.min(...blocks),
              room: { top: box.y * dpr, height: box.height * dpr },
            }
          },
          { data: png.toString('base64'), options: WALLPAPER, box },
        )
        const { top, height: h } = result.room
        // nothing but the wallpaper above the wave's highest point
        expect(result.highest).toBeGreaterThanOrEqual(
          Math.floor(top - ROOM_WAVE.depth * ROOM_WAVE.rise * h) - 3,
        )
        expect(result.highest).toBeGreaterThan(h / 2)
        // and no straight line: the room shows up at depths a tenth of
        // its height or more apart
        expect(result.spread).toBeGreaterThan(h / 10)
      },
    )
  })
}

/**
 * Compares the screen from the top down to the bottom of the room's box
 * (`box`, CSS px) with computeWallpaper()'s pixels for the same rows: how
 * many device pixels differ, and how far the room has come from the
 * wallpaper, as the mean OKLab distance between the two over blocks of 8 x 8
 * cells (the mean colour of a block, not its dither or grain).
 */
async function fromWallpaper(page: Page, box: Box) {
  const png = await page.screenshot({ scale: 'device' })
  await page.addScriptTag({
    type: 'module',
    content: `${MODULE}\nObject.assign(window, { computeWallpaper, readCssColors })`,
  })
  await page.waitForFunction(() => 'computeWallpaper' in window)
  return page.evaluate(
    async ({ data, options, box }) => {
      const image = new Image()
      image.src = `data:image/png;base64,${data}`
      await image.decode()
      const canvas = new OffscreenCanvas(image.width, image.height)
      const context = canvas.getContext('2d')!
      context.drawImage(image, 0, 0)
      const shot = new Uint32Array(
        context.getImageData(0, 0, image.width, image.height).data.buffer,
      )
      const w = window as unknown as {
        computeWallpaper: typeof import('../src/lib/wallpaper/tauler-wallpaper.js').computeWallpaper
        readCssColors: typeof import('../src/lib/wallpaper/tauler-wallpaper.js').readCssColors
      }
      const host = document.querySelector<HTMLElement>('.tauler-wallpaper')!
      const dpr = devicePixelRatio
      const devP = 3
      const height = Math.floor((box.y + box.height) * dpr)
      const expected = w.computeWallpaper({
        ...options,
        ...w.readCssColors(host),
        dpr,
        width: image.width / dpr,
        height: height / dpr,
        deviceWidth: image.width,
        deviceHeight: height,
      })
      let differ = 0
      for (let y = 0; y < height; y++)
        for (let x = 0; x < image.width; x++)
          if (
            shot[y * image.width + x] !==
            expected.pixels[((y / devP) | 0) * expected.cols + ((x / devP) | 0)]
          )
            differ++
      // sRGB bytes to OKLab (tauler-wallpaper.js rgbToOklab())
      const lin = (c: number) =>
        (c /= 255) <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4
      const oklab = (v: number) => {
        const r = lin(v & 255),
          g = lin((v >> 8) & 255),
          b = lin((v >> 16) & 255)
        const l = Math.cbrt(
          0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b,
        )
        const m = Math.cbrt(
          0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b,
        )
        const s = Math.cbrt(
          0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b,
        )
        return [
          0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
          1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
          0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s,
        ]
      }
      // blocks of 8 x 8 cells over the room's box
      const top = Math.ceil((box.y * dpr) / devP),
        rows = Math.floor(height / devP)
      let distance = 0,
        blocks = 0
      for (let by = top; by + 8 <= rows; by += 8)
        for (let bx = 0; bx + 8 <= expected.cols; bx += 8, blocks++) {
          const a = [0, 0, 0],
            b = [0, 0, 0]
          for (let y = by; y < by + 8; y++)
            for (let x = bx; x < bx + 8; x++) {
              const p = oklab(shot[(y * devP + 1) * image.width + x * devP + 1])
              const q = oklab(expected.pixels[y * expected.cols + x])
              for (let c = 0; c < 3; c++) {
                a[c] += p[c] / 64
                b[c] += q[c] / 64
              }
            }
          distance += Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2])
        }
      return { differ, distance: distance / blocks }
    },
    { data: png.toString('base64'), options: WALLPAPER, box },
  )
}

// The room fades in from the wallpaper as its samples gather
// (ROOM_EASE_IN): with none, its box is the wallpaper's own pixels, byte for
// byte (the first samples, mostly noise, many cells near black, once flashed
// dark over it); with each step it moves further from the wallpaper toward
// the converged image.
test('with a GPU the room fades in from the wallpaper as its samples gather', async ({
  playwright,
}, testInfo) => {
  test.skip(testInfo.project.name !== 'desktop', 'one width is enough')
  const distances: number[] = []
  for (const samples of [0, 6, 12, 20, ROOM_EASE_IN]) {
    await withConvergedRoom(
      playwright,
      testInfo,
      { width: 1440, height: 900, samples },
      async ({ page, box }) => {
        await expect(page.locator('[data-room]')).toHaveAttribute(
          'data-converged',
          String(samples),
        )
        const { differ, distance } = await fromWallpaper(page, box)
        if (samples === 0) expect(differ).toBe(0)
        else expect(differ).toBeGreaterThan(0)
        distances.push(distance)
      },
    )
  }
  for (let i = 1; i < distances.length; i++)
    expect(distances[i]).toBeGreaterThan(distances[i - 1])
})

// Fireflies: lone cells far brighter than everything around them, which 48
// samples never average away; they came in clusters around the candle lamp
// and the right-hand chair (the wall behind them, the glossy floor below).
// Counted in that part of the converged room: cells whose luminance is more
// than FIREFLY_STEP above the median of the 5 x 5 cells around them, with at
// most one such neighbour (so the edges of the chairs' frames and the
// furniture, lines of bright cells, do not count). Before the room's
// reflected and indirect light was clamped (clampFireflies() in
// whiteroom.ts) this counted 99 in the light theme and 144 in the dark one;
// after, 27 and 79 (on an RTX 4060 Ti; the sampling is seeded per frame).
const FIREFLY_STEP = 45
for (const { theme, most } of [
  { theme: 'light', most: 50 },
  { theme: 'dark', most: 110 },
] as const) {
  test(`with a GPU the room has no clusters of fireflies, in the ${theme} theme`, async ({
    playwright,
  }, testInfo) => {
    test.skip(testInfo.project.name !== 'desktop', 'one width is enough')
    await withConvergedRoom(
      playwright,
      testInfo,
      { width: 1440, height: 900, theme },
      async ({ page, box }) => {
        const fireflies = await inPage<number>(
          page,
          [await page.screenshot({ scale: 'device' })],
          `([{ width: w, px }], { x0, x1, y0, y1, step }) => {
            const lum = (x, y) => {
              const v = px[(y * 3 + 1) * w + x * 3 + 1]
              return 0.2126 * (v & 255) + 0.7152 * ((v >> 8) & 255) + 0.0722 * ((v >> 16) & 255)
            }
            const hot = (x, y) => {
              const around = []
              for (let dy = -2; dy <= 2; dy++)
                for (let dx = -2; dx <= 2; dx++)
                  if (dx || dy) around.push(lum(x + dx, y + dy))
              around.sort((a, b) => a - b)
              return lum(x, y) - (around[11] + around[12]) / 2 > step
            }
            let count = 0
            for (let y = y0; y < y1; y++)
              for (let x = x0; x < x1; x++) {
                if (!hot(x, y)) continue
                let neighbours = 0
                for (let dy = -1; dy <= 1; dy++)
                  for (let dx = -1; dx <= 1; dx++)
                    if ((dx || dy) && hot(x + dx, y + dy)) neighbours++
                if (neighbours <= 1) count++
              }
            return count
          }`,
          {
            // around the candle lamp and the right-hand chair, below the fade
            x0: Math.floor((0.53 * box.width) / 3),
            x1: Math.floor((0.68 * box.width) / 3),
            y0: Math.floor((box.y + 0.25 * box.height) / 3),
            y1: Math.floor((box.y + box.height) / 3) - 3,
            step: FIREFLY_STEP,
          },
        )
        expect(fireflies).toBeLessThan(most)
      },
    )
  })
}

// Seamless: the fade adds no edge. Down the screen, the mean OKLab colour of
// each cell row across the window's width (smoothed over 8 rows, one period
// of the dither matrix) changes by no more than SEAM_STEP from one row to
// the next, from above the highest point of the wave the fade starts along
// (ROOM_WAVE) to just below its lowest. That is a
// gentle gradient: the hard band the window drew at the room's top before
// it was framed out measured 0.006 here, and the wallpaper's own rows
// 0.0002-0.0008 (it is smooth across a whole row). Further in, the fade
// uncovers the room's own content (the lamp's head, the walls' shadows),
// which is not the seam.
const SEAM_STEP = 0.002
for (const { theme, width, height } of [
  { theme: 'light', width: 1440, height: 900 },
  { theme: 'dark', width: 1440, height: 900 },
  { theme: 'light', width: 2560, height: 1440 },
  { theme: 'dark', width: 390, height: 844 },
] as const) {
  test(`with a GPU the room meets the wallpaper without a seam, at ${width}x${height} in the ${theme} theme`, async ({
    playwright,
  }, testInfo) => {
    test.skip(testInfo.project.name !== 'desktop', 'one width is enough')
    await withConvergedRoom(
      playwright,
      testInfo,
      { width, height, theme },
      async ({ page, box }) => {
        const worst = await inPage<number>(
          page,
          [await page.screenshot({ scale: 'device' })],
          `([{ width: w, height: h, px }], { top, height, rise, dip }) => {
            // sRGB bytes to OKLab (tauler-wallpaper.js rgbToOklab())
            const lin = (c) => ((c /= 255) <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4)
            const oklab = (v) => {
              const r = lin(v & 255), g = lin((v >> 8) & 255), b = lin((v >> 16) & 255)
              const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b)
              const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b)
              const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b)
              return [0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
                      1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
                      0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s]
            }
            // each cell row's mean across the width, then over 8 rows
            const rows = []
            for (let y = 0; y + 3 <= h; y += 3) {
              const sum = [0, 0, 0]
              let n = 0
              for (let x = 0; x < w; x += 3, n++) {
                const lab = oklab(px[y * w + x])
                for (let c = 0; c < 3; c++) sum[c] += lab[c]
              }
              rows.push(sum.map((v) => v / n))
            }
            const smooth = []
            for (let r = 0; r + 8 <= rows.length; r++)
              smooth.push([0, 1, 2].map((c) => rows.slice(r, r + 8).reduce((s, v) => s + v[c], 0) / 8))
            // from above the wave's highest point (less the smoothing) to just below its lowest
            const start = Math.ceil(top / 3)
            let worst = 0
            for (let r = start - 8 - Math.round((rise * height) / 3); r < start + Math.round(((dip + 0.02) * height) / 3); r++)
              worst = Math.max(worst, Math.hypot(...[0, 1, 2].map((c) => smooth[r + 1][c] - smooth[r][c])))
            return worst
          }`,
          {
            top: box.y,
            height: box.height,
            rise: ROOM_WAVE.depth * ROOM_WAVE.rise,
            dip: ROOM_WAVE.depth * (1 - ROOM_WAVE.rise),
          },
        )
        expect(worst).toBeLessThan(SEAM_STEP)
      },
    )
  })
}

// The room's palette is the wallpaper's: every colour it shows is a point
// of the wallpaper's grid (tauler-wallpaper.js: OKLCH lightness, chroma and
// hue in equal steps measured from the background colour, whose own level
// is the background colour itself). The grid here is built from the same
// source as the shader's, WALLPAPER (wallpaper/options.ts), and the
// screenshot's colours are matched against it in sRGB bytes rather than
// converted to OKLCH: near grey, a byte of rounding moves the hue by tens
// of degrees, so a hue check in OKLCH would need a tolerance as wide as a
// step. (The shader takes its colours from a palette the module's own
// oklchToRgb() fills, so they match exactly; a byte of slack stays.) The
// wallpaper above the room is checked the same way, so the grid built here
// is the wallpaper's own and not just the room's.
for (const theme of ['light', 'dark'] as const) {
  test(`with a GPU the room's colours are the wallpaper's palette, in the ${theme} theme`, async ({
    playwright,
  }, testInfo) => {
    test.skip(testInfo.project.name !== 'desktop', 'one width is enough')
    await withConvergedRoom(
      playwright,
      testInfo,
      { width: 1440, height: 900, theme },
      async ({ page, box }) => {
        const { background, wallpaper, room } = await page.evaluate(
          async ({ data, top }) => {
            const probe = new OffscreenCanvas(1, 1).getContext('2d')!
            probe.fillStyle = getComputedStyle(document.documentElement)
              .getPropertyValue('--wallpaper-bg')
              .trim()
            probe.fillRect(0, 0, 1, 1)
            const image = new Image()
            image.src = `data:image/png;base64,${data}`
            await image.decode()
            const canvas = new OffscreenCanvas(image.width, image.height)
            const context = canvas.getContext('2d')!
            context.drawImage(image, 0, 0)
            const px = context.getImageData(
              0,
              0,
              image.width,
              image.height,
            ).data
            const colours = (y0: number, y1: number) => {
              const seen = new Set<number>()
              for (
                let i = y0 * image.width * 4;
                i < y1 * image.width * 4;
                i += 4
              )
                seen.add((px[i] << 16) | (px[i + 1] << 8) | px[i + 2])
              return [...seen].map((c) => [c >> 16, (c >> 8) & 255, c & 255])
            }
            return {
              background: Array.from(probe.getImageData(0, 0, 1, 1).data),
              wallpaper: colours(0, top),
              room: colours(top, image.height),
            }
          },
          {
            data: (await page.screenshot()).toString('base64'),
            top: Math.floor(box.y),
          },
        )
        const palette = wallpaperPalette(background.slice(0, 3) as RGB)
        const offPalette = (colours: number[][]) =>
          colours.filter(
            (c) =>
              !palette.some((p) => p.every((v, i) => Math.abs(v - c[i]) <= 1)),
          )
        expect(wallpaper.length).toBeGreaterThan(4)
        expect(offPalette(wallpaper)).toEqual([])
        expect(room.length).toBeGreaterThan(50)
        expect(offPalette(room)).toEqual([])
      },
    )
  })
}

type RGB = [number, number, number]

/**
 * Every colour of the wallpaper's grid on `background`, as the wallpaper
 * module draws it, over the whole range of lightness and the chroma any sRGB
 * colour reaches.
 */
function wallpaperPalette(background: RGB): RGB[] {
  const { stepL, stepC, stepH } = WALLPAPER
  const [L, a, b] = rgbToOklab(background)
  const C = Math.hypot(a, b)
  const hues = Math.max(1, Math.round(360 / Math.max(0.5, stepH)))
  const palette: RGB[] = []
  for (let qL = Math.ceil(-L / stepL); qL <= (1 - L) / stepL; qL++)
    for (let qC = 0; qC * stepC <= 0.4; qC++)
      for (let qH = 0; qH < (qL || qC ? hues : 1); qH++)
        palette.push(
          qL || qC
            ? oklchToRgb(
                L + qL * stepL,
                C + qC * stepC,
                (qH * 2 * Math.PI) / hues,
              )
            : background,
        )
  return palette
}

// The walls and floor start from the page colour of the theme that is on: a
// switch to the dark theme starts the image over, much darker. (The light,
// open sky and window, stays the same; see whiteroom.ts.)
test('with a GPU the room follows a theme switch', async ({
  playwright,
}, testInfo) => {
  test.skip(testInfo.project.name !== 'desktop', 'one width is enough')
  await withConvergedRoom(
    playwright,
    testInfo,
    { width: 1440, height: 900 },
    async ({ page, box, png }) => {
      // the back wall left of the furniture, below the fade
      const wall = [[0.05, 0.6, 0.25, 0.75]]
      const [light] = await measure(page, png, wall)
      await page.evaluate(() => {
        document.documentElement.classList.replace('light', 'dark')
        document.documentElement.classList.add('dark')
      })
      const room = page.locator('[data-room]')
      await expect(room).not.toHaveAttribute('data-converged')
      await expect(room).toHaveAttribute('data-converged', /\d+/, {
        timeout: 30_000,
      })
      const [dark] = await measure(
        page,
        await page.screenshot({ clip: box }),
        wall,
      )
      expect(dark.brightness).toBeLessThan(light.brightness / 2)
    },
  )
})

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
