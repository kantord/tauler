// The browser half of tauler.
//
// Everything here is glue. The components, the theme layer, the flattening step, the walk
// that produces markup and the globals a layout file is evaluated against all come from
// wasm — this file assigns them into the page's realm and drives the tick.
//
// The one exception is `estoH`, and it is deliberate: see the comment above it.

import init, * as wasm from './tauler_web.js'
import { ESTO_FRAGMENT } from './constants.js'

/**
 * `h`'s dispatch, in JavaScript.
 *
 * The desktop's is `optative_script::runtime::h_fn`, written in Rust because QuickJS
 * needed it there. It cannot come to wasm: its first case calls a JavaScript component
 * function, and doing that from wasm means marshalling every node across the boundary to
 * reach back into the engine the node came from. So this is a twin, and the only one in
 * the project (ADR 0025).
 *
 * What keeps it honest is the geometry gate: if this diverges from `h_fn` anywhere that
 * matters, the box tree differs from takumi's and CI names the node. That is a stricter
 * check than a fixture table, because it exercises `h` through every real component
 * rather than through the cases someone thought to write down.
 *
 * Three cases, in the same order as the Rust:
 *  1. a function tag is called immediately with `{...props, children}`
 *  2. the Fragment singleton returns its flattened children bare
 *  3. anything else becomes inert `{ type, props, children }`
 */
function estoH(type, props, ...children) {
  const kids = []
  flattenChildren(children, kids)

  if (typeof type === 'function') {
    return type({ ...(props ?? {}), children: kids })
  }
  if (type && typeof type === 'object' && ESTO_FRAGMENT in type) {
    return kids
  }
  // Props are nested under their own key rather than spread, so that a caller's `type` or
  // `children` prop cannot collide with this wrapper's fields. `taulerFlattenNode` is what
  // un-nests them, and it applies the collision rule.
  const out = { ...(props ?? {}) }
  delete out.children
  return { type, props: out, children: kids }
}

/** Drops `null`, `undefined` and `false`; splices arrays in place. `true` survives. */
function flattenChildren(values, out) {
  for (const val of values) {
    if (val === null || val === undefined || val === false) continue
    if (Array.isArray(val)) flattenChildren(val, out)
    else out.push(val)
  }
}

/**
 * Record a failure where a test or a developer can find it.
 *
 * A handler that throws inside an event listener disappears into the console, which is
 * exactly where nobody is looking — the desktop reports the same class of failure through
 * `warn_handler_once`, and this is its counterpart.
 */
function reportError(where, error) {
  const message = `${where}: ${(error && error.stack) || error}`
  ;(globalThis.__taulerErrors ??= []).push(message)
  console.error('tauler:', message)
}

let booted = null

/**
 * Load the wasm module and install everything a layout file expects on `globalThis`.
 *
 * Idempotent: several embeds on one page share one module and one realm, which is also
 * what makes `taulerSetStreamValue` reach every embed at once.
 */
export function boot(wasmUrl) {
  if (booted) return booted
  booted = (async () => {
    await init({ module_or_path: wasmUrl })

    // The components. Every `__ui_*` export is one, named exactly as the ES module shim
    // and the QuickJS registration name it.
    for (const [name, value] of Object.entries(wasm)) {
      if (name.startsWith('__ui_')) globalThis[name] = value
    }

    globalThis.useStringStream = wasm.taulerUseStringStream
    globalThis.registerModule = (bin, props) => wasm.taulerRegisterModule(bin, props)
    globalThis.__tauler_flatten_node = wasm.taulerFlattenNode
    globalThis.__esto_h = estoH
    globalThis.Fragment = { [ESTO_FRAGMENT]: true }

    // The shared source: `useJSONStream`, `useEvents`, the handler registry, pointer
    // capture, step rounding. Byte for byte what QuickJS is given.
    ;(0, eval)(wasm.taulerGlobalsJs())

    // `h` is assembled the same way `jsx.rs` assembles it, and for the same reason: each
    // node must be flat by the time a Rust-backed component consumes it as `children`.
    globalThis.h = (type, props, ...children) =>
      globalThis.__tauler_flatten_node(
        estoH(type, globalThis.__tauler_register_handlers(type, props), ...children),
      )

    // The shims, last: each one calls a `__ui_*` global that has to exist already.
    ;(0, eval)(wasm.taulerBootstrapJs())
  })()
  return booted
}

/**
 * One embedded layout, mounted into one element.
 *
 * The element is the Mount node: everything inside it is tauler's, everything outside is
 * the page's.
 */
export class Mount {
  #element
  #render
  #theme

  constructor(element, render, theme = 'dark') {
    this.#element = element
    this.#render = render
    this.#theme = theme
    this.#element.addEventListener('pointerdown', (e) => {
      try {
        this.#onPointerDown(e)
      } catch (error) {
        reportError('pointerdown', error)
      }
    })
  }

  /**
   * One tick: evaluate the layout, resolve the theme, walk it to markup, replace the
   * subtree.
   *
   * Rebuilding everything rather than diffing is ADR 0007 applied unchanged, not a
   * shortcut. It costs focus, selection and CSS transitions, which a documentation embed
   * does not have; a live deployment replaces this line with a keyed diff and nothing
   * else about the pipeline moves.
   */
  tick() {
    const tree = wasm.taulerResolveTheme(this.#render(), this.#theme)
    const out = wasm.taulerRender(tree)
    if (out.kind !== 'dom') {
      throw new Error(`tauler: unknown render output kind ${JSON.stringify(out.kind)}`)
    }
    this.#element.innerHTML = out.dom
  }

  /** The element a render path names, or null. */
  #nodeAt(path) {
    return this.#element.querySelector(`[data-tauler-path="${CSS.escape(path)}"]`)
  }

  /**
   * Delegated hit-testing.
   *
   * The DOM already knows what is under the pointer, so `closest` replaces the scene walk
   * `hit_test.rs` has to do. `data-tauler-on` is on handler-carrying elements only, so the
   * nearest match is the node that should receive the event — the same answer the desktop
   * reaches by taking the last painted node under the point that carries a handler.
   */
  #onPointerDown(event) {
    const target = event.target.closest?.('[data-tauler-on]')
    if (!target) return
    const kinds = (target.getAttribute('data-tauler-on') ?? '').split(' ')
    const rect = target.getBoundingClientRect()
    const press = { x: event.clientX - rect.left, y: event.clientY - rect.top }

    if (kinds.includes('drag')) {
      this.#beginDrag(event, target, rect, press)
      return
    }
    if (kinds.includes('click')) {
      this.#dispatch(target, 'on_click', pointerPayload(rect, press, press))
    }
  }

  /**
   * A drag takes every motion until release, whatever the pointer is over — ADR 0020.
   * `setPointerCapture` is the DOM's name for the same thing, which is why CONTEXT.md's
   * Pointer capture entry points at it.
   */
  #beginDrag(event, target, rect, press) {
    const path = target.getAttribute('data-tauler-path')
    target.setPointerCapture(event.pointerId)
    this.#dispatch(target, 'on_drag', pointerPayload(rect, press, press))

    const move = (e) => {
      const node = this.#nodeAt(path) ?? target
      const box = node.getBoundingClientRect()
      const at = { x: e.clientX - box.left, y: e.clientY - box.top }
      // The press point is reported beside every position the drag produces, so a control
      // can measure displacement without remembering anything — ADR 0022.
      this.#dispatch(node, 'on_drag', pointerPayload(box, at, press))
    }
    const up = () => {
      target.removeEventListener('pointermove', move)
      target.removeEventListener('pointerup', up)
      globalThis.__tauler_release_handler?.()
    }
    target.addEventListener('pointermove', move)
    target.addEventListener('pointerup', up)
  }

  /**
   * Run one node's handler and deliver whatever intents it produced.
   *
   * A handler is either an array of intents or `{$handler: n}` naming a function in the
   * registry — ADR 0021. Both shapes arrive here as data; neither is a callback the DOM
   * knows about.
   */
  #dispatch(node, prop, payload) {
    const path = node.getAttribute('data-tauler-path')
    const handler = this.#handlerFor(path, prop)
    if (!handler) return
    const intents = Array.isArray(handler)
      ? handler
      : globalThis.__tauler_intents(globalThis.__tauler_handlers[handler.$handler](payload))
    for (const intent of intents ?? []) deliver(intent)
    this.tick()
  }

  /**
   * The handler a path carries, read back out of the tree the last tick produced.
   *
   * Read rather than remembered: every tick rebuilds the tree from scratch, so a cached
   * handler would belong to a tree that no longer exists.
   */
  #handlerFor(path, prop) {
    const tree = wasm.taulerResolveTheme(this.#render(), this.#theme)
    return nodeAtPath(tree, path)?.[prop] ?? null
  }
}

function pointerPayload(rect, at, press) {
  return {
    x: at.x,
    y: at.y,
    press_x: press.x,
    press_y: press.y,
    width: rect.width,
    height: rect.height,
  }
}

/** Walk a layout tree by the same child-index path the markup carries. */
function nodeAtPath(tree, path) {
  const root = unwrapDom(tree)
  if (path === '') return root.length === 1 ? root[0] : null
  let node = root.length === 1 ? root[0] : root[Number(path.split('.')[0])]
  const rest = root.length === 1 ? path.split('.') : path.split('.').slice(1)
  for (const index of rest) {
    if (index === '') continue
    const kids = childrenOf(node)
    node = kids[Number(index)]
    if (node === undefined) return null
  }
  return typeof node === 'object' ? node : null
}

function unwrapDom(tree) {
  const dom = tree?.type === 'root' ? childrenOf(tree).find((c) => c?.type === 'dom') : tree
  return childrenOf(dom)
}

function childrenOf(node) {
  const kids = node?.children
  if (kids === undefined) return []
  return Array.isArray(kids) ? kids.flat(Infinity) : [kids]
}

/**
 * Hand an intent to the Transport registered for its channel.
 *
 * A Module only ever sees messages addressed to its own channel and never learns what
 * caused them, so this is a lookup and nothing more. A subprocess does this job on a
 * desktop; in a page whatever registered the channel does — a worker, a socket, an SSH
 * bridge, or the loopback a documentation example uses.
 */
const transports = new Map()

export function registerTransport(channel, handler) {
  transports.set(channel, handler)
}

function deliver(intent) {
  transports.get(intent?.channel)?.(intent.event)
}

/** Push a Stream value in, from anywhere. The Transport's write side. */
export function setStreamValue(bin, script, line) {
  wasm.taulerSetStreamValue(bin, script ?? undefined, String(line))
}
