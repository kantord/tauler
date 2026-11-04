import { readFileSync } from 'node:fs'
import {
  test,
  expect,
  type Page,
  type PlaywrightWorkerArgs,
  type TestInfo,
} from '@playwright/test'
import { WALLPAPER } from '../src/components/site/wallpaper/options'

// The wallpaper is drawn by a WebGL shader (wallpaper-gl.ts) on a GPU, and
// otherwise by the wallpaper module on the CPU; the module is the reference.
// These tests compare what is on screen, behind the page's (hidden) content,
// with computeWallpaper()'s pixels for the same part of the page, colours
// and device pixel ratio. CI has no GPU, and its headless Chromium has only
// software WebGL (SwiftShader), which the site refuses for the wallpaper
// (too slow, see page-wallpaper.ts): the shader tests turn it back on, so CI
// checks the shader on SwiftShader, and the CPU path as every other test
// there sees it. The GPU test checks the shader on hardware, and skips where
// there is none.

const MODULE = readFileSync(
  new URL('../src/lib/wallpaper/tauler-wallpaper.js', import.meta.url),
  'utf8',
)

// Each test starts its own browser and compares whole screens of pixels:
// slow next to the rest of the suite when it all runs in parallel.
test.describe.configure({ timeout: 90_000 })

const GPU = ['--use-angle=gl', '--enable-gpu', '--ignore-gpu-blocklist']

type Options = {
  width: number
  height: number
  dpr?: number
  theme?: 'light' | 'dark'
  args?: string[]
  /** Hides WebGL 2 from the page, for the CPU fallback. */
  noWebgl2?: boolean
  /** Lets the shader run on software WebGL (window.taulerWallpaperSoftwareGl). */
  softwareGl?: boolean
}

/**
 * Opens a Chromium with a window `width` x `height` CSS px at `dpr` (forced
 * on the command line: Playwright's deviceScaleFactor does not reach the
 * device-pixel-content-box sizes the wallpaper lays its canvas out by), in
 * the site's `theme`, and hands `check` a page on which only the wallpaper
 * shows.
 */
async function withWallpaper(
  playwright: PlaywrightWorkerArgs['playwright'],
  testInfo: TestInfo,
  {
    width,
    height,
    dpr = 1,
    theme = 'light',
    args = [],
    noWebgl2,
    softwareGl,
  }: Options,
  path: string,
  check: (page: Page) => Promise<void>,
) {
  const browser = await playwright.chromium.launch({
    args: [
      ...args,
      `--force-device-scale-factor=${dpr}`,
      `--window-size=${width},${height}`,
    ],
  })
  try {
    const page = await browser.newPage({
      baseURL: testInfo.project.use.baseURL,
      viewport: null,
    })
    await page.addInitScript(
      ({ theme, noWebgl2, softwareGl }) => {
        localStorage.setItem('theme', theme)
        window.taulerWallpaperSoftwareGl = softwareGl
        if (noWebgl2) {
          const getContext = HTMLCanvasElement.prototype.getContext
          // @ts-expect-error -- the overloads cannot be restated here
          HTMLCanvasElement.prototype.getContext = function (kind, ...rest) {
            return kind === 'webgl2'
              ? null
              : getContext.call(this, kind, ...rest)
          }
        }
      },
      { theme, noWebgl2, softwareGl },
    )
    await page.goto(path)
    await page.addStyleTag({
      content: `body * { visibility: hidden !important }
        .tauler-wallpaper, .tauler-wallpaper * { visibility: visible !important }`,
    })
    await page.addScriptTag({
      type: 'module',
      content: `${MODULE}\nObject.assign(window, { computeWallpaper, readCssColors, oklchToRgb, rgbToOklab })`,
    })
    await check(page)
  } finally {
    await browser.close()
  }
}

/** Scrolls to `y` and waits until the wallpaper is drawn there. */
async function scrollTo(page: Page, y: number) {
  await page.evaluate((y) => window.scrollTo(0, y), y)
  await expect
    .poll(() =>
      page.evaluate(() => {
        const host = document.querySelector<HTMLElement>('.tauler-wallpaper')!
        const [top, height] = (host.dataset.drawn ?? '').split(' ').map(Number)
        const view = scrollY * devicePixelRatio
        return (
          top <= view && top + height >= view + innerHeight * devicePixelRatio
        )
      }),
    )
    .toBe(true)
  // the frame that shows it
  await page.evaluate(
    () =>
      new Promise((done) =>
        requestAnimationFrame(() => requestAnimationFrame(done)),
      ),
  )
}

/**
 * What the window shows of the wallpaper, against computeWallpaper()'s
 * pixels for the same page area: the cells (dither pixels) that differ, and
 * how many were compared.
 */
async function compare(page: Page) {
  // a screenshot can move the window: retry until it did not
  const at = () => page.evaluate(() => scrollY)
  let png: Buffer
  for (let tries = 0; ; tries++) {
    const before = await at()
    png = await page.screenshot({ scale: 'device' })
    if ((await at()) === before) break
    expect(tries, 'the window keeps moving').toBeLessThan(3)
  }
  return page.evaluate(
    async ({ data, options }) => {
      const image = new Image()
      image.src = `data:image/png;base64,${data}`
      await image.decode()
      const canvas = new OffscreenCanvas(image.width, image.height)
      const context = canvas.getContext('2d')!
      context.drawImage(image, 0, 0)
      const shot = new Uint32Array(
        context.getImageData(0, 0, image.width, image.height).data.buffer,
      )
      const host = document.querySelector<HTMLElement>('.tauler-wallpaper')!
      const dpr = devicePixelRatio
      // the page's device row at the window's top, and the host's width
      const top = Math.round(-host.getBoundingClientRect().top * dpr)
      const width = Math.min(
        image.width,
        Math.round(host.getBoundingClientRect().width * dpr),
      )
      const height = Math.min(image.height, Math.round(innerHeight * dpr))
      const w = window as unknown as {
        computeWallpaper: typeof import('../src/lib/wallpaper/tauler-wallpaper.js').computeWallpaper
        readCssColors: typeof import('../src/lib/wallpaper/tauler-wallpaper.js').readCssColors
      }
      const probe = w.computeWallpaper({ ...options, width: 1, height: 1, dpr })
      const devP = probe.devP
      const row0 = Math.floor(top / devP),
        shift = top - row0 * devP
      const expected = w.computeWallpaper({
        ...options,
        ...w.readCssColors(host),
        dpr,
        width: width / dpr,
        height: (height + shift) / dpr,
        deviceWidth: width,
        deviceHeight: height + shift,
        row0,
      })
      const differ = new Set<number>()
      for (let y = 0; y < height; y++)
        for (let x = 0; x < width; x++) {
          const cell =
            (((y + shift) / devP) | 0) * expected.cols + ((x / devP) | 0)
          if (shot[y * image.width + x] !== expected.pixels[cell])
            differ.add(cell)
        }
      return {
        renderer: host.dataset.renderer,
        cells: Math.ceil(width / devP) * Math.ceil(height / devP),
        differ: differ.size,
        examples: [...differ].slice(0, 3).map((cell) => ({
          cell,
          expected: expected.pixels[cell].toString(16),
        })),
      }
    },
    { data: png.toString('base64'), options: WALLPAPER },
  )
}

const CASES = [
  {
    path: '/docs/',
    width: 1440,
    height: 900,
    dpr: 1,
    theme: 'light',
    scroll: [0, 333],
  },
  {
    path: '/docs/',
    width: 1366,
    height: 768,
    dpr: 1.25,
    theme: 'dark',
    scroll: [0, 517.77],
  },
  {
    path: '/docs/component-reference/',
    width: 1440,
    height: 900,
    dpr: 2,
    theme: 'light',
    scroll: [1001.1, 'deep'],
  },
  {
    path: '/docs/component-reference/',
    width: 1920,
    height: 1080,
    dpr: 1.25,
    theme: 'light',
    scroll: ['deep'],
  },
  {
    path: '/docs/layout-file/',
    width: 390,
    height: 844,
    dpr: 2,
    theme: 'dark',
    scroll: [0, 2345.6],
  },
] as const

for (const { path, scroll, ...options } of CASES) {
  test(`the shader draws the module's pixels: ${path} at ${options.width}x${options.height}, ${options.dpr}x, ${options.theme}`, async ({
    playwright,
  }, testInfo) => {
    test.skip(testInfo.project.name !== 'desktop', 'the window is set here')
    const shader = { ...options, softwareGl: true }
    await withWallpaper(playwright, testInfo, shader, path, async (page) => {
      for (const at of scroll) {
        const y =
          at === 'deep'
            ? await page.evaluate(
                () => document.documentElement.scrollHeight - innerHeight - 300,
              )
            : at
        await scrollTo(page, y)
        const result = await compare(page)
        expect(result.renderer).toBe('webgl2')
        expect(result.differ, JSON.stringify({ y, ...result })).toBe(0)
      }
    })
  })
}

test("with a GPU the shader draws the module's pixels", async ({
  playwright,
}, testInfo) => {
  test.skip(testInfo.project.name !== 'desktop', 'the window is set here')
  for (const dpr of [1, 1.25, 2])
    for (const theme of ['light', 'dark'] as const)
      await withWallpaper(
        playwright,
        testInfo,
        { width: 1440, height: 900, dpr, theme, args: GPU },
        '/docs/component-reference/',
        async (page) => {
          const hardware = await page.evaluate(
            () =>
              !!document
                .createElement('canvas')
                .getContext('webgl2', { failIfMajorPerformanceCaveat: true }),
          )
          test.skip(!hardware, 'no hardware WebGL 2 here (CI has no GPU)')
          for (const y of [0, 4321.3]) {
            await scrollTo(page, y)
            const result = await compare(page)
            expect(result.renderer).toBe('webgl2')
            expect(
              result.differ,
              JSON.stringify({ dpr, theme, y, ...result }),
            ).toBe(0)
          }
        },
      )
})

test('on software WebGL the module draws the wallpaper, not the shader', async ({
  playwright,
}, testInfo) => {
  test.skip(testInfo.project.name !== 'desktop', 'the window is set here')
  await withWallpaper(
    playwright,
    testInfo,
    { width: 1440, height: 900 },
    '/docs/',
    async (page) => {
      const software = await page.evaluate(() => {
        const gl = document.createElement('canvas').getContext('webgl2')
        const debug = gl?.getExtension('WEBGL_debug_renderer_info')
        return (
          !!debug &&
          /swiftshader/i.test(gl!.getParameter(debug.UNMASKED_RENDERER_WEBGL))
        )
      })
      test.skip(!software, 'this Chromium has no SwiftShader WebGL')
      await scrollTo(page, 0)
      const result = await compare(page)
      expect(result.renderer).toBe('2d')
      expect(result.differ, JSON.stringify(result)).toBe(0)
    },
  )
})

test('without WebGL 2 the module draws the wallpaper, and it scrolls too', async ({
  playwright,
}, testInfo) => {
  test.skip(testInfo.project.name !== 'desktop', 'the window is set here')
  await withWallpaper(
    playwright,
    testInfo,
    { width: 1366, height: 768, dpr: 1.25, theme: 'dark', noWebgl2: true },
    '/docs/component-reference/',
    async (page) => {
      for (const y of [0, 3210.4]) {
        await scrollTo(page, y)
        const result = await compare(page)
        expect(result.renderer).toBe('2d')
        expect(result.differ, JSON.stringify(result)).toBe(0)
      }
    },
  )
})

// The wallpaper belongs to the page: a part of the page shows the same
// pixels wherever it is in the window, and a scrolled window shows another
// part of the pattern.
test('the wallpaper scrolls with the page', async ({
  playwright,
}, testInfo) => {
  test.skip(testInfo.project.name !== 'desktop', 'the window is set here')
  await withWallpaper(
    playwright,
    testInfo,
    { width: 1280, height: 800, dpr: 1.25 },
    '/docs/component-reference/',
    async (page) => {
      /** The window's rows, from page row `from` (device px), `rows` of them. */
      const rows = async (from: number, count: number) => {
        const png = await page.screenshot({ scale: 'device' })
        return page.evaluate(
          async ({ data, from, count }) => {
            const image = new Image()
            image.src = `data:image/png;base64,${data}`
            await image.decode()
            const canvas = new OffscreenCanvas(image.width, image.height)
            const context = canvas.getContext('2d')!
            context.drawImage(image, 0, 0)
            const top = Math.round(scrollY * devicePixelRatio)
            return Array.from(
              context.getImageData(0, from - top, 1000, count).data,
            )
          },
          { data: png.toString('base64'), from, count },
        )
      }
      await scrollTo(page, 1500)
      // page rows 2200-2400 device px: in the window at 1500 and at 1700
      const before = await rows(2200, 200)
      const window0 = await rows(1875, 300)
      await scrollTo(page, 1700)
      expect(await rows(2200, 200)).toEqual(before)
      expect(await rows(2125, 300)).not.toEqual(window0)
    },
  )
})

// The wallpaper's background level is the page colour itself, in both
// themes. The designer's module drew it at the background's lightness and
// chroma but hue 0, a pinkish grey (rgb(233, 226, 228) on #e6e4de), which
// must not show anywhere.
for (const theme of ['light', 'dark'] as const) {
  test(`the wallpaper's background is the page colour, in the ${theme} theme`, async ({
    playwright,
  }, testInfo) => {
    test.skip(testInfo.project.name !== 'desktop', 'the window is set here')
    await withWallpaper(
      playwright,
      testInfo,
      { width: 1440, height: 900, theme },
      '/docs/',
      async (page) => {
        await scrollTo(page, 0)
        const png = await page.screenshot()
        const { share, hueZero } = await page.evaluate(async (data) => {
          const w = window as unknown as {
            oklchToRgb: (L: number, C: number, h: number) => number[]
            rgbToOklab: (rgb: number[]) => number[]
          }
          const probe = new OffscreenCanvas(1, 1).getContext('2d')!
          probe.fillStyle = getComputedStyle(document.body).backgroundColor
          probe.fillRect(0, 0, 1, 1)
          const colour = Array.from(probe.getImageData(0, 0, 1, 1).data)
          const [L, a, b] = w.rgbToOklab(colour.slice(0, 3))
          const old = w.oklchToRgb(L, Math.hypot(a, b), 0)
          const image = new Image()
          image.src = `data:image/png;base64,${data}`
          await image.decode()
          const canvas = new OffscreenCanvas(image.width, image.height)
          const context = canvas.getContext('2d')!
          context.drawImage(image, 0, 0)
          const px = context.getImageData(0, 0, image.width, image.height).data
          let page = 0,
            hueZero = 0
          for (let i = 0; i < px.length; i += 4) {
            if (colour.every((v, k) => k === 3 || px[i + k] === v)) page++
            else if (old.every((v, k) => px[i + k] === v)) hueZero++
          }
          return { share: page / (px.length / 4), hueZero }
        }, png.toString('base64'))
        expect(share).toBeGreaterThan(0.05)
        expect(hueZero).toBe(0)
      },
    )
  })
}

// A theme switch (next-themes puts .dark on <html>) redraws the wallpaper in
// the other theme's colours, from either renderer.
for (const softwareGl of [true, false]) {
  test(`the wallpaper follows a theme switch${softwareGl ? ', from the shader' : ''}`, async ({
    playwright,
  }, testInfo) => {
    test.skip(testInfo.project.name !== 'desktop', 'the window is set here')
    await withWallpaper(
      playwright,
      testInfo,
      { width: 1280, height: 800, softwareGl },
      '/docs/',
      async (page) => {
        await scrollTo(page, 200)
        expect((await compare(page)).differ).toBe(0)
        const before = await page.screenshot()
        await page.evaluate(() => {
          document.documentElement.classList.remove('light')
          document.documentElement.classList.add('dark')
        })
        await expect.poll(async () => (await compare(page)).differ).toBe(0)
        expect(await page.screenshot()).not.toEqual(before)
      },
    )
  })
}
