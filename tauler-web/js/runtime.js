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
 * `h`'s dispatch, in JavaScript — a twin of `optative_script::runtime::h_fn`, and the only
 * one in the project (ADR 0027). It cannot come to wasm: its first case calls a JavaScript
 * component function.
 *
 * The geometry gate keeps it honest: diverge from `h_fn` and the box tree differs from
 * takumi's, so CI names the node.
 *
 * Three cases, in the Rust's order: a function tag is called immediately; the Fragment
 * singleton returns its children bare; anything else becomes inert `{type, props, children}`.
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
  // Nested rather than spread, so a caller's `type` or `children` cannot collide with this
  // wrapper's own. `taulerFlattenNode` un-nests them and applies the collision rule.
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

/** A handler that throws inside a listener otherwise disappears into the console. */
function reportError(where, error) {
  const message = `${where}: ${(error && error.stack) || error}`
  ;(globalThis.__taulerErrors ??= []).push(message)
  console.error('tauler:', message)
}

let booted = null

/**
 * Each D3Figure's latest registration, by key. A page runs `draw` itself, so this is where a
 * tick finds the function for a placeholder that has no drawing yet.
 *
 * Not cleared per tick: an element can be built before the tick that shows it — at a layout
 * file's module scope, once — so its registration must outlive that tick. Instead each tick
 * drops the keys its Mount no longer shows (`#placePictures`), which bounds this by the
 * figures on screen.
 *
 * Module scope rather than per Mount because the global `registerFigure` cannot tell which
 * Mount is evaluating. A Mount places its figures in the tick that evaluated them, so another
 * Mount's later pruning only costs a figure that leaves the screen, falls out of the cache
 * and comes back without being built again.
 */
const figures = new Map()

/** How many drawings of keys no longer on screen each Mount keeps. A key is a figure's whole
 * identity, so an evicted one is only drawn again — the bound is memory, not correctness.
 * Keys on screen are never evicted: they would be drawn again on the very next tick. */
const PICTURE_CACHE_SIZE = 64

let d3Loaded = null

/**
 * d3, loaded on the first figure and never again. A page with no D3Figure never downloads
 * it — the same rule the desktop's draw runtime follows. The bundle is a UMD script that
 * sets `globalThis.d3`; importing it as a module runs it for that side effect.
 */
function loadD3() {
  d3Loaded ??= import('/tauler/d3.min.js').then(() => globalThis.d3)
  return d3Loaded
}

/**
 * Load the wasm module and install everything a layout file expects on `globalThis`.
 *
 * Idempotent: every mount on a page shares one module and one realm, which is also what
 * lets `setStreamValue` reach them all at once.
 */
export function boot(wasmUrl) {
  if (booted) return booted
  booted = (async () => {
    await init({ module_or_path: wasmUrl })

    // Every `__ui_*` export is a component, named as the ES module and QuickJS name it.
    for (const [name, value] of Object.entries(wasm)) {
      if (name.startsWith('__ui_')) globalThis[name] = value
    }

    globalThis.useStringStream = wasm.taulerUseStringStream
    globalThis.registerModule = (bin, props) => wasm.taulerRegisterModule(bin, props)
    globalThis.registerFigure = (key, width, height, draw) =>
      figures.set(key, { width, height, draw })
    globalThis.__tauler_theme_color = wasm.taulerThemeColor
    globalThis.__esto_h = estoH
    globalThis.Fragment = { [ESTO_FRAGMENT]: true }

    // The shared source, byte for byte what QuickJS is given.
    ;(0, eval)(wasm.taulerGlobalsJs())

    // Assembled as `jsx.rs` assembles it, against the same `__tauler_flatten_node` the
    // shared globals just defined: each node must be flat by the time a Rust component
    // consumes it as `children`.
    globalThis.h = (type, props, ...children) =>
      globalThis.__tauler_flatten_node(
        estoH(type, globalThis.__tauler_register_handlers(type, props), ...children),
      )

    // Last: each shim calls a `__ui_*` global that has to exist already.
    ;(0, eval)(wasm.taulerBootstrapJs())
  })()
  return booted
}

/** One layout, in one Mount node: everything inside it is tauler's. */
export class Mount {
  #element
  #render
  #theme
  /** Finished `<svg>` markup by figure key, least recently used first. Per Mount: two
   * layouts on one page may well use the same key for different figures. */
  #pictures = new Map()
  /** The figure keys the latest tick placed: never evicted from `#pictures`. */
  #onScreen = new Set()
  /** Keys whose drawing is waiting on d3, so ticks in the meantime do not draw them again. */
  #drawing = new Set()
  /** Keys whose `draw` threw. Not retried: a tick can come many times a second, and the
   * desktop gives a failing key one warning too (`src/d3_figure/draw.rs`). A new key — the
   * author's fix, or new data — is a new figure and gets drawn. */
  #failed = new Set()

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
   * One tick: evaluate, resolve the theme, walk to markup, replace the subtree.
   *
   * Rebuilding rather than diffing is ADR 0007 unchanged, not a shortcut. It costs focus,
   * selection and CSS transitions — which a documentation preview does not have. A live
   * deployment swaps this line for a keyed diff and nothing else moves.
   */
  tick() {
    const tree = wasm.taulerResolveTheme(this.#evaluate(), this.#theme)
    const out = wasm.taulerRender(tree)
    if (out.kind !== 'dom') {
      throw new Error(`tauler: unknown render output kind ${JSON.stringify(out.kind)}`)
    }
    this.#element.innerHTML = out.dom
    this.#placePictures()
  }

  /**
   * Swap each Picture placeholder (`data-tauler-picture`, see `dom.rs`) for its figure's
   * drawing: from the cache when this key was drawn before, otherwise once d3 has loaded.
   *
   * A key that does not change is never drawn twice — that is what `key` promises an author
   * — so a rebuild re-inserts the cached markup rather than running `draw` again.
   *
   * Then forgets the registrations of keys this Mount no longer shows (see `figures`).
   */
  #placePictures() {
    const onScreen = new Set()
    for (const el of this.#element.querySelectorAll('[data-tauler-picture]')) {
      const key = el.getAttribute('data-tauler-picture')
      onScreen.add(key)
      const markup = this.#pictures.get(key)
      if (markup !== undefined) {
        // Re-inserted as most recently used.
        this.#pictures.delete(key)
        this.#pictures.set(key, markup)
        el.outerHTML = markup
        continue
      }
      const figure = figures.get(key)
      if (figure && !this.#drawing.has(key) && !this.#failed.has(key)) this.#draw(key, figure)
    }
    this.#onScreen = onScreen
    for (const key of figures.keys()) if (!onScreen.has(key)) figures.delete(key)
    this.#evictOffScreen()
  }

  /** Drop the least recently used drawings beyond `PICTURE_CACHE_SIZE` of the keys not on
   * screen. */
  #evictOffScreen() {
    let excess = -PICTURE_CACHE_SIZE
    for (const key of this.#pictures.keys()) if (!this.#onScreen.has(key)) excess++
    for (const key of this.#pictures.keys()) {
      if (excess <= 0) break
      if (!this.#onScreen.has(key)) {
        this.#pictures.delete(key)
        excess--
      }
    }
  }

  /**
   * Run one figure's `draw` against a real `<svg>`, keep the markup, and fill the
   * placeholders. A failing `draw` leaves its placeholder empty, as a failed Picture does on
   * a desktop, and its key is not drawn again.
   */
  async #draw(key, { width, height, draw }) {
    this.#drawing.add(key)
    try {
      const d3 = await loadD3()
      const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg')
      svg.setAttribute('xmlns', 'http://www.w3.org/2000/svg')
      svg.setAttribute('width', String(width))
      svg.setAttribute('height', String(height))
      svg.setAttribute('viewBox', `0 0 ${width} ${height}`)
      // The same root attributes the desktop's draw runtime gives its svg (`jsx.rs`), so
      // one `draw` produces the same figure in both places.
      svg.setAttribute('font-family', 'sans-serif')
      draw(d3.select(svg), d3)
      const markup = svg.outerHTML

      this.#pictures.set(key, markup)
      this.#evictOffScreen()
      // Looked up now, not when the draw was requested: every tick since replaced the
      // subtree, so the element that asked may be gone and others with the key may exist.
      const selector = `[data-tauler-picture="${CSS.escape(key)}"]`
      for (const el of this.#element.querySelectorAll(selector)) el.outerHTML = markup
    } catch (error) {
      console.warn(`tauler: <D3Figure key=${JSON.stringify(key)}> failed to draw:`, error)
      this.#failed.add(key)
      if (this.#failed.size > PICTURE_CACHE_SIZE) {
        this.#failed.delete(this.#failed.keys().next().value)
      }
    } finally {
      this.#drawing.delete(key)
    }
  }

  /**
   * Run the layout file with this Mount's theme as `ctx.theme`.
   *
   * Every Mount shares one realm, so `ctx` is set just before each evaluation rather than
   * once: evaluation is synchronous, so no other Mount can run in between.
   */
  #evaluate() {
    globalThis.ctx = { ...globalThis.ctx, theme: wasm.taulerThemeCtx(this.#theme) }
    return this.#render()
  }

  /** The element a render path names, or null. */
  #nodeAt(path) {
    return this.#element.querySelector(`[data-tauler-path="${CSS.escape(path)}"]`)
  }

  /**
   * Delegated hit-testing. The DOM knows what is under the pointer, so `closest` replaces
   * the scene walk `hit_test.rs` does; `data-tauler-on` marks only handler-carrying
   * elements, so the nearest match is the node that should receive the event.
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

  /** A drag takes every motion until release (ADR 0020); `setPointerCapture` is the DOM's
   * name for it. */
  #beginDrag(event, target, rect, press) {
    const path = target.getAttribute('data-tauler-path')
    target.setPointerCapture(event.pointerId)
    this.#dispatch(target, 'on_drag', pointerPayload(rect, press, press))

    const move = (e) => {
      const node = this.#nodeAt(path) ?? target
      const box = node.getBoundingClientRect()
      const at = { x: e.clientX - box.left, y: e.clientY - box.top }
      // The press point rides along with every position, so a control can measure
      // displacement without remembering anything (ADR 0022).
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
   * Run one node's handler and deliver its intents. A handler is an array of intents or
   * `{$handler: n}` naming a registry function (ADR 0021) — data either way.
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

  /** Read rather than remembered: every tick rebuilds the tree, so a cached handler would
   * belong to one that no longer exists. */
  #handlerFor(path, prop) {
    const tree = wasm.taulerResolveTheme(this.#evaluate(), this.#theme)
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
 * Hand an intent to the Transport registered for its channel — a lookup and nothing more.
 * A subprocess does this on a desktop; in a page, whatever registered the channel does.
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
