import { test, expect } from '@playwright/test'

// Exercises `defineUnit` (tauler-web/js/units.js) end-to-end in a real browser: the
// actual wasm module, reconciled through the actual `taulerReconcileUnit` export — not a
// mock of either half. See ADR 0037.
//
// `refreshInterval` is set absurdly high and every sweep after the first is driven by the
// returned handle's `sweep()` directly, so the test is deterministic and fast rather than
// racing a real `setInterval`.
test('a browser Unit enters, updates and exits from a scripted observe()', async ({
  page,
}) => {
  await page.goto('/docs/component-reference/')

  const events = await page.evaluate(async () => {
    // @ts-expect-error — /tauler/*.js is a build product astro check never sees
    const { boot } = await import('/tauler/runtime.js')
    // @ts-expect-error — /tauler/*.js is a build product astro check never sees
    const { defineUnit } = await import('/tauler/units.js')
    await boot('/tauler/tauler_web_bg.wasm')

    const log: unknown[] = []
    // world: absent -> present (on: false) -> present (on: true) -> absent.
    // Sweep 0 (absent) fires on define; the loop below drives sweeps 1-3.
    const world = [[], [{ id: 'a', on: false }], [{ id: 'a', on: true }], []]
    let step = 0

    const unit = defineUnit({
      key: (i: { id: string }) => i.id,
      value: (i: { on: boolean }) => i.on,
      items: () => [{ id: 'a', on: true }], // declared once, throughout
      observe: () => world[step],
      refreshInterval: 1_000_000_000,
      enter: (items: unknown) => log.push(['enter', items]),
      update: (pairs: unknown) => log.push(['update', pairs]),
      exit: (items: unknown) => log.push(['exit', items]),
    })

    for (step = 1; step < world.length; step++) unit.sweep()
    unit.stop()
    return log
  })

  // Sweep 0: world is empty, "a" is declared -> enter.
  expect(events[0]).toEqual(['enter', [{ id: 'a', on: true }]])
  // Sweep 1: world now has "a" with on:false, declared wants on:true -> update.
  expect(events[1]).toEqual([
    'update',
    [{ item: { id: 'a', on: true }, old: { id: 'a', on: false } }],
  ])
  // Sweep 2: world matches declared (on:true) -> no event.
  // Sweep 3: world empty again, "a" still declared -> a fresh enter, not an
  // exit — the Item never stopped being declared.
  expect(events[2]).toEqual(['enter', [{ id: 'a', on: true }]])
  expect(events).toHaveLength(3)
})

test('a Unit may not define both a batch hook and its per-Item spelling', async ({
  page,
}) => {
  await page.goto('/docs/component-reference/')

  const message = await page.evaluate(async () => {
    // @ts-expect-error — /tauler/*.js is a build product astro check never sees
    const { defineUnit } = await import('/tauler/units.js')
    try {
      defineUnit({
        key: (i: { id: string }) => i.id,
        value: () => true,
        items: () => [],
        observe: () => [],
        enter: () => {},
        enterOne: () => {},
      })
      return null
    } catch (error) {
      return error instanceof TypeError ? error.message : String(error)
    }
  })

  expect(message).toBe('A Unit may not define both `enter` and `enterOne`.')
})

test('a per-Item hook written under the batch name throws, not silently does nothing', async ({
  page,
}) => {
  await page.goto('/docs/component-reference/')

  const message = await page.evaluate(async () => {
    // @ts-expect-error — /tauler/*.js is a build product astro check never sees
    const { boot } = await import('/tauler/runtime.js')
    // @ts-expect-error — /tauler/*.js is a build product astro check never sees
    const { defineUnit } = await import('/tauler/units.js')
    await boot('/tauler/tauler_web_bg.wasm')

    try {
      defineUnit({
        key: (i: { id: string }) => i.id,
        value: () => true,
        items: () => [{ id: 'a' }],
        observe: () => [],
        // Wrong: this is `enterOne`'s shape, written under `enter`.
        enter: (light: { id: string }) => light.id,
      })
      return null
    } catch (error) {
      return error instanceof TypeError ? error.message : String(error)
    }
  })

  expect(message).toBe(
    '`enter` receives an array of Items, not one Item. Did you mean `enterOne`?',
  )
})
