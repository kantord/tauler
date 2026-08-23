// Browser Units.
//
// Native tauler's `unit()` (ADR 0033) says what should be true and reconciles the world
// toward it, running hooks in a second QuickJS runtime because `rquickjs::Ctx` isn't `Send`
// (ADR 0034) — a workaround this file doesn't need: the caller already *is* the browser's
// own JS engine (ADR 0027), so `key`, `value`, `observe` and every hook are plain JS,
// called directly, on a `setInterval` instead of a thread. There is also no shell here —
// `observe` returns synthetic or fetched data, never `sh`. Only the diff itself crosses
// into wasm, via `taulerReconcileUnit`, so it stays byte-identical to what a native Unit
// reconciles (`units_reconcile`, ADR 0037).
//
// Declarative, JSX-declared Units (`<WindowPlacement .../>`) are not implemented here —
// `defineUnit` is imperative: `items` is a function returning the currently-declared Items,
// called fresh each sweep the same way `observe` is.
//
// Requires `boot()` (runtime.js) to have already resolved — this module shares that same
// wasm instance (ES modules cache by URL), it does not initialize its own.

import * as wasm from './tauler_web.js'

const ONE_HOOK_NAME = { enter: 'enterOne', update: 'updateOne', exit: 'exitOne' }

function assertHookPairing(spec) {
  for (const [batch, one] of Object.entries(ONE_HOOK_NAME)) {
    if (typeof spec[batch] === 'function' && typeof spec[one] === 'function') {
      throw new TypeError(`A Unit may not define both \`${batch}\` and \`${one}\`.`)
    }
  }
}

/**
 * Guards against the failure mode ADR 0033 names: `enter: (light) => …`
 * written where `enterOne` was meant. Same arity, same name, same type as a
 * real batch hook — nothing reflection can see is different, and the bug
 * reads `undefined` off an array and silently does nothing. Wrapping the
 * payload in a `Proxy` that throws on any property an array doesn't have
 * turns that into a message naming the fix, the same trick native Units use.
 */
function arrayGuard(name, oneName, items) {
  return new Proxy(items, {
    get(target, prop, receiver) {
      if (prop in target || typeof prop === 'symbol') return Reflect.get(target, prop, receiver)
      throw new TypeError(
        `\`${name}\` receives an array of Items, not one Item. Did you mean \`${oneName}\`?`,
      )
    },
  })
}

/**
 * `enter`/`exit`: batch gets the Items array, per-Item gets one Item each.
 * Nothing is called when there is nothing in the batch — a sweep that found
 * no exits does not tell `exit` so, the same as a Unit that defines no `exit`
 * at all isn't managing what it didn't declare.
 */
function dispatchBatch(spec, name, items) {
  if (items.length === 0) return
  const one = ONE_HOOK_NAME[name]
  if (typeof spec[name] === 'function') spec[name](arrayGuard(name, one, items))
  else if (typeof spec[one] === 'function') for (const item of items) spec[one](item)
}

/** `update`: batch gets `{item, old}` pairs, per-Item gets `(item, old)`. */
function dispatchUpdate(spec, pairs) {
  if (pairs.length === 0) return
  if (typeof spec.update === 'function') spec.update(arrayGuard('update', 'updateOne', pairs))
  else if (typeof spec.updateOne === 'function') {
    for (const { item, old } of pairs) spec.updateOne(item, old)
  }
}

/** `key`/`value`-project a raw Items array into what `taulerReconcileUnit` diffs. */
function project(key, value, rawItems) {
  return rawItems.map((props, order) => ({
    key: String(key(props)),
    value: value(props),
    props,
    order,
  }))
}

/**
 * Declare a Unit: `key`, `value`, `observe`, `items`, and `enter`/`enterOne`,
 * `update`/`updateOne`, `exit`/`exitOne` — the same shape as native `unit()`
 * (ADR 0033), minus the shell `observe` can't have in a browser.
 *
 * Sweeps once immediately, then every `refreshInterval` ms (default 5000, the
 * same as native's default). Returns `{ stop, sweep }`: `stop` clears the
 * Unit's interval; `sweep` runs one convergence immediately, for a caller
 * that already knows the declared world just changed and doesn't want to
 * wait out the interval.
 */
export function defineUnit({
  key,
  value,
  observe,
  items,
  enter,
  enterOne,
  update,
  updateOne,
  exit,
  exitOne,
  refreshInterval = 5000,
}) {
  const spec = { enter, enterOne, update, updateOne, exit, exitOne }
  assertHookPairing(spec)

  function sweep() {
    const desired = project(key, value, items())
    const observed = project(key, value, observe() ?? [])
    const result = wasm.taulerReconcileUnit(desired, observed)
    // Exits first, so a Unit that has to free something before claiming it
    // again — a port, a lock, a single physical device — can (the same order
    // native's sweep_unit dispatches in).
    dispatchBatch(spec, 'exit', result.exit)
    dispatchUpdate(spec, result.update)
    dispatchBatch(spec, 'enter', result.enter)
  }

  sweep()
  const timer = setInterval(sweep, refreshInterval)
  // `sweep` is exposed, not just scheduled: a caller who knows the world just
  // changed (the user flipped a switch) can converge immediately instead of
  // waiting out the interval, and it is what lets a test drive this
  // deterministically instead of racing real timers.
  return { stop: () => clearInterval(timer), sweep }
}
