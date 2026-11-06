import {
  test,
  expect,
  type PlaywrightWorkerArgs,
  type TestInfo,
} from '@playwright/test'
import { ROOM_EASE_IN } from '../src/components/site/room/room-size'
import type { TaulerTestHooks } from '../src/components/site/test-hooks'

// The room's smoke test, the one room test CI runs. Every other room test
// needs a GPU and skips without one (docs.spec.ts, withConvergedRoom()); CI
// has only software WebGL (SwiftShader), which the room refuses
// (roomSupported() in room.ts). Here the test lets it run there
// (roomSoftwareGl in test-hooks.ts, with wallpaperSoftwareGl for the
// context it shares) in a window so small the image is 64 x 38 cells, and
// stops it at ROOM_EASE_IN samples (roomSamples): the fewest at
// which the room shows at full weight (presence() in room-size.ts), which
// the pixel checks need. The test takes 5-10 s there. That still compiles every
// shader, applies every load-time shader edit (each throws on a missing
// anchor), bakes the leaves, path-traces, denoises and tone-maps: what breaks
// when the room's code does. It does not judge the picture.

/** CSS px: at device pixel ratio 1 and 3 px cells, 64 cells wide. */
const WINDOW = { width: 192, height: 120 }
const SAMPLES = ROOM_EASE_IN

/** A page on SwiftShader in WINDOW, its problems (page errors, console errors and tauler: warnings) collected. */
async function softwarePage(
  playwright: PlaywrightWorkerArgs['playwright'],
  testInfo: TestInfo,
  hooks: Pick<TaulerTestHooks, 'roomBrokenShader'> = {},
) {
  const browser = await playwright.chromium.launch({
    args: ['--use-angle=swiftshader', '--force-device-scale-factor=1'],
  })
  const page = await browser.newPage({
    baseURL: testInfo.project.use.baseURL,
    viewport: WINDOW,
  })
  const problems: string[] = []
  page.on('pageerror', (error) => problems.push(`pageerror: ${error.message}`))
  page.on('console', (message) => {
    // the room reports a failure to load or render as a tauler: warning
    if (
      message.type() === 'error' ||
      (message.type() === 'warning' && message.text().startsWith('tauler:'))
    )
      problems.push(message.text())
  })
  await page.addInitScript(
    ({ samples, hooks }) => {
      localStorage.setItem('theme', 'light')
      window.__taulerTest = {
        wallpaperSoftwareGl: true,
        roomSoftwareGl: true,
        roomSamples: samples,
        // the room's fades into the wallpaper off: the box shows the room alone
        roomFade: {
          L: 1e-4,
          C: 1e-4,
          h: 1e-4,
          depth: 0,
          match: 1e-4,
          detail: 1e-4,
        },
        ...hooks,
      }
    },
    { samples: SAMPLES, hooks },
  )
  return { browser, page, problems }
}

test('the room renders on software WebGL', async ({ playwright }, testInfo) => {
  test.skip(testInfo.project.name !== 'desktop', 'the window is set here')
  const { browser, page, problems } = await softwarePage(playwright, testInfo)
  try {
    await page.goto('/')
    const renderer = await page.evaluate(() => {
      const gl = document.createElement('canvas').getContext('webgl2')!
      const debug = gl.getExtension('WEBGL_debug_renderer_info')!
      return String(gl.getParameter(debug.UNMASKED_RENDERER_WEBGL))
    })
    expect(renderer).toMatch(/swiftshader/i)

    // The room joins the wallpaper only once createRoom() has resolved: every
    // shader compiled and every load-time edit found its anchor.
    // A failure is reported (a tauler: warning) and the room never joins, so
    // stop waiting at the first problem.
    const room = page.locator('.tauler-wallpaper [data-room]')
    await expect
      .poll(async () => problems.length > 0 || (await room.count()) > 0, {
        timeout: 30_000,
      })
      .toBe(true)
    expect(problems).toEqual([])
    await expect(page.locator('.tauler-wallpaper')).toHaveAttribute(
      'data-renderer',
      'webgl2',
    )
    // It reaches its samples and stops.
    await expect(room).toHaveAttribute('data-converged', String(SAMPLES), {
      timeout: 30_000,
    })
    expect(problems).toEqual([])

    // The box shows a room, not a blank, black (NaN) or noise image. The
    // tone-mapping pass reads only the denoise's output, so this is also
    // the check that the denoise ran: without it the box is black, and with
    // only its first pass it is mostly black.
    await page.addStyleTag({
      content: `body * { visibility: hidden }
        .tauler-wallpaper, .tauler-wallpaper * { visibility: visible }`,
    })
    const box = (await room.boundingBox())!
    const png = await page.screenshot({ clip: box })
    const stats = await page.evaluate(async (data) => {
      const image = new Image()
      image.src = `data:image/png;base64,${data}`
      await image.decode()
      const canvas = new OffscreenCanvas(image.width, image.height)
      const context = canvas.getContext('2d')!
      context.drawImage(image, 0, 0)
      const { data: px } = context.getImageData(0, 0, image.width, image.height)
      const lum: number[] = []
      for (let i = 0; i < px.length; i += 4)
        lum.push((px[i] + px[i + 1] + px[i + 2]) / 3)
      const mean = lum.reduce((a, b) => a + b, 0) / lum.length
      const sd = Math.sqrt(
        lum.reduce((a, b) => a + (b - mean) ** 2, 0) / lum.length,
      )
      // in the 3.2:1 box the plant's leaves are dark, left of centre; the
      // right wall is bare and light
      const region = (x0: number, x1: number, y0: number, y1: number) => {
        let sum = 0,
          n = 0
        for (let y = Math.floor(y0 * image.height); y < y1 * image.height; y++)
          for (
            let x = Math.floor(x0 * image.width);
            x < x1 * image.width;
            x++
          ) {
            sum += lum[y * image.width + x]
            n++
          }
        return sum / n
      }
      return {
        mean,
        sd,
        black: lum.filter((l) => l < 8).length / lum.length,
        plant: region(0.13, 0.2, 0.45, 0.72),
        wall: region(0.75, 0.95, 0.1, 0.5),
      }
    }, png.toString('base64'))
    await testInfo.attach('room', { body: png, contentType: 'image/png' })
    const details = JSON.stringify(stats)
    expect(stats.sd, details).toBeGreaterThan(8)
    expect(stats.black, details).toBeLessThan(0.05)
    expect(stats.wall - stats.plant, details).toBeGreaterThan(30)
  } finally {
    await browser.close()
  }
})

// A shader that fails to compile rejects createRoom(): the room never joins,
// and the wallpaper draws alone, as on a GPU the room does not support.
test('a room shader that fails to compile leaves the wallpaper alone', async ({
  playwright,
}, testInfo) => {
  test.skip(testInfo.project.name !== 'desktop', 'the window is set here')
  const { browser, page, problems } = await softwarePage(playwright, testInfo, {
    roomBrokenShader: true,
  })
  try {
    await page.goto('/')
    await expect
      .poll(() => problems.some((p) => p.startsWith('tauler:')), {
        timeout: 30_000,
      })
      .toBe(true)
    expect(problems.filter((p) => p.startsWith('pageerror'))).toEqual([])
    expect(problems.join('\n')).toMatch(
      /tauler: the room is unavailable:.*shader failed to compile/,
    )
    await expect(page.locator('.tauler-wallpaper [data-room]')).toHaveCount(0)
    await expect(page.locator('.tauler-wallpaper')).toHaveAttribute(
      'data-renderer',
      'webgl2',
    )
  } finally {
    await browser.close()
  }
})
