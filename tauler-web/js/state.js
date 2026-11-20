// The page's half of `store(...)`.
//
// The shared globals (`tauler-core/src/globals.rs`) turn a Store into a Proxy over the
// host functions and one clock. On a desktop Rust supplies them over its `StateStore`
// (`src/state.rs`); a page has no evaluator to hand one to, so it supplies them here,
// over a Map. The contract is the comment "What the host supplies" in the shared
// globals, and `tests/page_state_test.rs` runs this file against them.
//
// State lives in this module, so it lasts as long as the page and is shared by every
// Mount on it. Nothing is saved: a reload starts every Store from its default.
//
// A local Store (`tauler-core/src/local_store.rs`) is kept here too, under an identity
// that starts with its Mount (`runtime.js`), and dropped by `__tauler_state_drop` when
// its Mount's render no longer declares it. A Mount that is thrown away is not told
// to this module, so its local Stores stay until the page goes.
//
// A value is kept as its JSON text. That is the copy the Proxy must not be able to reach
// (it hands the parsed value to the layout file, which may change it in place), and it
// is what "did this write change anything" compares. Two objects that differ only in
// key order are therefore different here and equal on a desktop; the cost is one
// version, never a wrong value.

/** identity -> key -> { json, version } */
const states = new Map()
/** Moves whenever any value changes; a version is the clock at that change. */
let clock = 0
/**
 * Every change since `withState` began, or null: `[identity, key, slot before]` for a
 * key, and `[identity]` for a state a declaration made (`Undo` in `src/state.rs`).
 */
let journal = null

function entryOf(identity) {
  let entry = states.get(identity)
  if (!entry) states.set(identity, (entry = new Map()))
  return entry
}

/** Write one key as JSON text; `undefined` deletes it. An equal value changes nothing. */
function put(identity, key, json) {
  const entry = entryOf(identity)
  const slot = entry.get(key)
  if (slot ? slot.json === json : json === undefined) return
  journal?.push([identity, key, slot])
  clock += 1
  if (json === undefined) entry.delete(key)
  else entry.set(key, { json, version: clock })
}

function jsonOf(value) {
  if (value === undefined) return undefined
  const json = JSON.stringify(value)
  if (json === undefined) throw new TypeError('state holds only JSON values')
  return json
}

/** Assign the host functions onto `target`, under the names the shared globals call. */
export function installStateHost(target = globalThis) {
  // The third argument is the compile pass's token. A desktop refuses a declaration
  // without it so that no layout file can choose an identity; a page runs only the
  // examples `tauler-docgen` compiled, so there is nobody to refuse.
  target.__tauler_state_declare = (identity, initial) => {
    if (initial === null || typeof initial !== 'object' || Array.isArray(initial)) {
      throw new TypeError('store(): the default must be a JSON object')
    }
    if (!states.has(identity)) journal?.push([identity])
    const entry = entryOf(identity)
    for (const key of Object.keys(initial)) {
      if (!entry.has(key)) put(identity, key, jsonOf(initial[key]))
    }
  }
  target.__tauler_state_get = (identity, key, have, same) => {
    const slot = states.get(identity)?.get(key)
    if (!slot) return undefined
    const nested = slot.json[0] === '{' || slot.json[0] === '['
    return nested && slot.version === have ? same : JSON.parse(slot.json)
  }
  target.__tauler_state_version = (identity, key) => states.get(identity)?.get(key)?.version ?? 0
  // A local Store that is not held is not written: `LOCAL_IDENTITY_PREFIX` in
  // `src/state.rs` is the same rule, with its reason. Change the two together.
  target.__tauler_state_set = (identity, key, value) => {
    if (!states.has(identity) && identity.startsWith('local://')) return 0
    put(identity, key, jsonOf(value))
    return states.get(identity)?.get(key)?.version ?? 0
  }
  target.__tauler_state_keys = (identity) => [...(states.get(identity)?.keys() ?? [])].sort()
  // One whole state. Nothing undoes it: it is the last thing a render does.
  target.__tauler_state_drop = (identity) => {
    if (states.delete(identity)) clock += 1
  }
}

/**
 * Run one render or one handler as the desktop does (`render_in_transaction` and
 * `invoke_handler_outcome` in `src/jsx.rs`): tell the shared globals where the clock
 * stands, write back the nested values that were changed in place when it returns, and
 * put every Store back as it was when it throws.
 */
export function withState(run, target = globalThis) {
  journal = []
  target.__tauler_state_now = clock
  try {
    const out = run()
    target.__tauler_state_flush()
    return out
  } catch (error) {
    target.__tauler_state_abort()
    // A restored key gets a new version, not its old one: the Proxy may hold the value
    // that is being thrown away under a version in between.
    // A state that was declared in the run stops being held, so a later write to a
    // local Store among them is ignored (`__tauler_state_set`).
    // The desktop has the same rules a second time, in `rollback` in `src/state.rs`.
    // Change the two together.
    for (const [identity, key, slot] of journal.reverse()) {
      clock += 1
      if (key === undefined) states.delete(identity)
      else if (slot) entryOf(identity).set(key, { json: slot.json, version: clock })
      else entryOf(identity).delete(key)
    }
    throw error
  } finally {
    journal = null
  }
}
