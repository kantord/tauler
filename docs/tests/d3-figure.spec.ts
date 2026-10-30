import { test, expect } from '@playwright/test'

// A `<D3Figure>` in a page: the same author code as on a desktop, but `draw` runs in the
// page's own realm against a real `<svg>` element instead of becoming a Picture (see
// `src/d3_figure`). Driven through the real wasm module and the real `Mount`, as
// units.spec.ts drives Units.
//
// d3 itself is loaded lazily on the first figure, so the first drawing may arrive after
// `tick()` returns — every test polls for it rather than reading it straight away.

test('a D3Figure mounted in a page is drawn by its draw function into a real svg', async ({
  page,
}) => {
  await page.goto('/docs/component-reference/')

  const width = await page.evaluate(async () => {
    // @ts-expect-error — /tauler/*.js is a build product astro check never sees
    const { boot, Mount } = await import('/tauler/runtime.js')
    await boot('/tauler/tauler_web_bg.wasm')

    const el = document.createElement('div')
    document.body.append(el)
    const D3Figure = (globalThis as any).D3Figure
    new Mount(el, () => ({
      type: 'dom',
      children: [
        D3Figure({
          key: 'k1',
          width: 40,
          height: 20,
          draw: (svg: any) => svg.append('circle').attr('r', 3),
        }),
      ],
    })).tick()

    const deadline = Date.now() + 5_000
    while (!el.querySelector('svg circle[r="3"]')) {
      if (Date.now() > deadline) return null
      await new Promise((resolve) => setTimeout(resolve, 50))
    }
    return el.querySelector('svg')!.getAttribute('width')
  })

  expect(width).toBe('40')
})

test('a D3Figure whose key does not change is drawn once, and a new key draws again', async ({
  page,
}) => {
  await page.goto('/docs/component-reference/')

  const draws = await page.evaluate(async () => {
    // @ts-expect-error — /tauler/*.js is a build product astro check never sees
    const { boot, Mount } = await import('/tauler/runtime.js')
    await boot('/tauler/tauler_web_bg.wasm')

    const el = document.createElement('div')
    document.body.append(el)
    const D3Figure = (globalThis as any).D3Figure

    let key = 'same'
    let count = 0
    const mount = new Mount(el, () => ({
      type: 'dom',
      children: [
        D3Figure({
          key,
          width: 40,
          height: 20,
          draw: (svg: any) => {
            count++
            svg.append('circle').attr('r', 3).attr('data-key', key)
          },
        }),
      ],
    }))

    const drawn = async (k: string) => {
      const deadline = Date.now() + 5_000
      while (!el.querySelector(`svg circle[data-key="${k}"]`)) {
        if (Date.now() > deadline) return false
        await new Promise((resolve) => setTimeout(resolve, 50))
      }
      return true
    }

    mount.tick()
    if (!(await drawn('same')))
      return { error: 'the first drawing never arrived' }
    const afterFirst = count

    mount.tick()
    const sameKeyKept = await drawn('same')
    const afterSameKey = count

    key = 'new'
    mount.tick()
    if (!(await drawn('new'))) return { error: 'the new key was never drawn' }
    return { afterFirst, afterSameKey, sameKeyKept, afterNewKey: count }
  })

  expect(draws).toEqual({
    afterFirst: 1,
    // The same key re-inserts the cached markup instead of drawing again.
    afterSameKey: 1,
    sameKeyKept: true,
    afterNewKey: 2,
  })
})

test('a D3Figure whose draw throws is tried once, not on every tick', async ({
  page,
}) => {
  await page.goto('/docs/component-reference/')

  const count = await page.evaluate(async () => {
    // @ts-expect-error — /tauler/*.js is a build product astro check never sees
    const { boot, Mount } = await import('/tauler/runtime.js')
    await boot('/tauler/tauler_web_bg.wasm')

    const el = document.createElement('div')
    document.body.append(el)
    const D3Figure = (globalThis as any).D3Figure

    let count = 0
    const mount = new Mount(el, () => ({
      type: 'dom',
      children: [
        D3Figure({
          key: 'broken',
          width: 40,
          height: 20,
          draw: () => {
            count++
            throw new Error('bad draw')
          },
        }),
      ],
    }))

    // The first draw waits on d3; give it time to load and fail before ticking again.
    mount.tick()
    const deadline = Date.now() + 5_000
    while (count === 0 && Date.now() < deadline) {
      await new Promise((resolve) => setTimeout(resolve, 50))
    }
    mount.tick()
    mount.tick()
    await new Promise((resolve) => setTimeout(resolve, 300))
    return count
  })

  expect(count).toBe(1)
})

test('a D3Figure built once, outside the render function, is drawn', async ({
  page,
}) => {
  await page.goto('/docs/component-reference/')

  const drawn = await page.evaluate(async () => {
    // @ts-expect-error — /tauler/*.js is a build product astro check never sees
    const { boot, Mount } = await import('/tauler/runtime.js')
    await boot('/tauler/tauler_web_bg.wasm')

    const el = document.createElement('div')
    document.body.append(el)
    const D3Figure = (globalThis as any).D3Figure

    // As a layout file's module scope does: the element exists before the first tick.
    const chart = D3Figure({
      key: 'module-scope',
      width: 40,
      height: 20,
      draw: (svg: any) => svg.append('circle').attr('r', 3),
    })
    const mount = new Mount(el, () => ({ type: 'dom', children: [chart] }))
    mount.tick()
    mount.tick()

    const deadline = Date.now() + 5_000
    while (!el.querySelector('svg circle[r="3"]')) {
      if (Date.now() > deadline) return false
      await new Promise((resolve) => setTimeout(resolve, 50))
    }
    return true
  })

  expect(drawn).toBe(true)
})

test('figures still on screen are not evicted from the cache and drawn again', async ({
  page,
}) => {
  await page.goto('/docs/component-reference/')

  const result = await page.evaluate(async () => {
    // @ts-expect-error — /tauler/*.js is a build product astro check never sees
    const { boot, Mount } = await import('/tauler/runtime.js')
    await boot('/tauler/tauler_web_bg.wasm')

    const el = document.createElement('div')
    document.body.append(el)
    const D3Figure = (globalThis as any).D3Figure

    // More figures than the cache holds of keys no longer on screen.
    const figures = 70
    let count = 0
    const mount = new Mount(el, () => ({
      type: 'dom',
      children: Array.from({ length: figures }, (_, i) =>
        D3Figure({
          key: `many/${i}`,
          width: 4,
          height: 4,
          draw: (svg: any) => {
            count++
            svg.append('circle').attr('r', 1)
          },
        }),
      ),
    }))

    const allDrawn = async () => {
      const deadline = Date.now() + 5_000
      while (el.querySelectorAll('svg circle').length < figures) {
        if (Date.now() > deadline) return false
        await new Promise((resolve) => setTimeout(resolve, 50))
      }
      return true
    }

    mount.tick()
    if (!(await allDrawn()))
      return { error: 'the first drawings never all arrived' }
    const afterFirst = count

    mount.tick()
    const stillAll = await allDrawn()
    await new Promise((resolve) => setTimeout(resolve, 200))
    return { afterFirst, stillAll, afterSecond: count }
  })

  expect(result).toEqual({ afterFirst: 70, stillAll: true, afterSecond: 70 })
})

test('a D3Figure with an object key names the same picture as on a desktop', async ({
  page,
}) => {
  await page.goto('/docs/component-reference/')

  const srcs = await page.evaluate(async () => {
    // @ts-expect-error — /tauler/*.js is a build product astro check never sees
    const { boot, Mount } = await import('/tauler/runtime.js')
    await boot('/tauler/tauler_web_bg.wasm')

    const el = document.createElement('div')
    document.body.append(el)
    const D3Figure = (globalThis as any).D3Figure

    try {
      const figures = [{ a: 1 }, { a: 2 }].map((key) =>
        D3Figure({
          key,
          width: 40,
          height: 20,
          draw: (svg: any) => svg.append('circle').attr('r', 3),
        }),
      )
      new Mount(el, () => ({ type: 'dom', children: figures })).tick()
      return figures.map((figure: any) => figure.src as string)
    } catch (error) {
      return { error: String(error) }
    }
  })

  expect(Array.isArray(srcs), JSON.stringify(srcs)).toBe(true)
  const [first, second] = srcs as string[]
  // The literal the desktop's Rust test pins: the first 16 hex chars of sha256('{"a":1}').
  expect(first.endsWith('json-015abd7f5cc57a2d@40x20'), first).toBe(true)
  expect(second).not.toBe(first)
})

test('draw receives its picture name in a page', async ({ page }) => {
  await page.goto('/docs/component-reference/')

  const texts = await page.evaluate(async () => {
    // @ts-expect-error — /tauler/*.js is a build product astro check never sees
    const { boot, Mount } = await import('/tauler/runtime.js')
    await boot('/tauler/tauler_web_bg.wasm')

    const el = document.createElement('div')
    document.body.append(el)
    const D3Figure = (globalThis as any).D3Figure

    // Two figures in one DOM: ids derived from the name, such as Plot's clip-path, must differ.
    const figures = [{ a: 1 }, 'k1'].map((key) =>
      D3Figure({
        key,
        width: 40,
        height: 20,
        draw: (svg: any, _d3: any, { name }: { name: string }) =>
          svg.append('text').text(name),
      }),
    )
    new Mount(el, () => ({ type: 'dom', children: figures })).tick()

    const deadline = Date.now() + 5_000
    while (el.querySelectorAll('svg text').length < 2) {
      if (Date.now() > deadline) return { error: 'the drawings never arrived' }
      await new Promise((resolve) => setTimeout(resolve, 50))
    }
    return Array.from(
      el.querySelectorAll('svg text'),
      (text) => text.textContent,
    )
  })

  expect(texts).toEqual(['json-015abd7f5cc57a2d@40x20', 'k1@40x20'])
})

test('a layout file that imports d3 at the top uses it in a page', async ({
  page,
}) => {
  await page.goto('/docs/component-reference/')

  const result = await page.evaluate(async () => {
    // @ts-expect-error — /tauler/*.js is a build product astro check never sees
    const { boot, Mount } = await import('/tauler/runtime.js')
    await boot('/tauler/tauler_web_bg.wasm')

    // A layout file as the page loads one: an ES module whose bare `"d3"` the page's
    // import map resolves. d3 is used at module scope, outside any `draw`.
    const layout = `
      import * as d3 from 'd3'
      import { scaleLinear } from 'd3'
      const x = d3.scaleLinear().domain([0, 10]).range([0, 100])
      export default () => ({
        type: 'dom',
        children: [
          { type: 'span', children: [String(x(5)), ' ', String(scaleLinear === d3.scaleLinear)] },
        ],
      })
    `
    const url = URL.createObjectURL(
      new Blob([layout], { type: 'text/javascript' }),
    )
    try {
      const { default: render } = await import(url)
      const el = document.createElement('div')
      document.body.append(el)
      new Mount(el, render).tick()
      return el.querySelector('span')?.textContent ?? null
    } catch (error) {
      return { error: String(error) }
    }
  })

  expect(result).toBe('50 true')
})

test('measureText measures with the browser in a page', async ({ page }) => {
  await page.goto('/docs/component-reference/')

  const result = await page.evaluate(async () => {
    // @ts-expect-error — /tauler/*.js is a build product astro check never sees
    const { boot } = await import('/tauler/runtime.js')
    await boot('/tauler/tauler_web_bg.wasm')

    const measureText = (globalThis as any).measureText
    const hello = measureText('Hello', { family: 'sans-serif', size: 11 })
    let thrown = null
    try {
      measureText('Hello', { family: '', size: 11 })
    } catch (error) {
      thrown = (error as Error).name
    }
    // Neither a generic family nor a missing one is a parse failure.
    const serif = measureText('Hello', { family: 'serif', size: 11 })
    const missing = measureText('Hello', { family: 'No Such Family', size: 11 })
    let unparseable = null
    try {
      measureText('Hello', { family: 'a;b', size: 11 })
    } catch (error) {
      unparseable = [(error as Error).name, (error as Error).message]
    }
    return { hello, thrown, serif, missing, unparseable }
  })

  expect(result.hello.width).toBeGreaterThan(0)
  expect(result.hello.ascent).toBeGreaterThan(0)
  // Rounded to 1/64 px, as on the desktop.
  expect(result.hello.width * 64).toBe(Math.round(result.hello.width * 64))
  expect(result.thrown).toBe('TypeError')
  expect(result.serif.width).toBeGreaterThan(0)
  expect(result.missing.width).toBeGreaterThan(0)
  // As on the desktop: a family that is not a CSS family list throws, naming it.
  expect(result.unparseable?.[0]).toBe('TypeError')
  expect(result.unparseable?.[1]).toContain('measureText')
  expect(result.unparseable?.[1]).toContain('"a;b"')
})
