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
