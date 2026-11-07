/**
 * The wallpaper behind a page, anchored to the page: it scrolls with the
 * content, and a long page shows more of the pattern, never the same window
 * of it.
 *
 * The host covers the whole page (absolute, inset 0, in a positioned body)
 * and clips. In it sits one canvas, the window's width and its height plus
 * OVERSCAN above and below, placed at a page position: the browser scrolls it
 * with everything else, on the compositor, so it cannot lag behind the
 * content. Only when the visible part of the page comes within a quarter of
 * OVERSCAN of the canvas's edge (checked once a frame while scrolling) is the
 * canvas moved and drawn again, around the window. (A canvas fixed to the
 * window and redrawn on scroll would be drawn from the main thread's scroll
 * position, a frame or more behind the compositor's on a fast scroll, so the
 * pattern would slide against the content.)
 *
 * Every dither pixel is whole device pixels on the page's grid. The canvas's
 * backing store is its exact device-pixel size (ResizeObserver,
 * device-pixel-content-box), so it shows 1:1; its top is put on a device
 * pixel and read back from layout, and the draw counts cells from the page's
 * top whatever device row the canvas starts on. Chrome scrolls by whole
 * device pixels (scrollY is a multiple of 1/dpr), so the canvas stays on the
 * grid while scrolling, at fractional ratios too.
 *
 * What draws it: the WebGL 2 shader (wallpaper-gl.ts) on a GPU. Without
 * one (no WebGL 2, software WebGL, a lost context) the wallpaper module's
 * computeWallpaper() does, on the CPU, into a fresh 2D canvas placed the same
 * way; both draw the same pixels. Software WebGL (SwiftShader, what headless
 * Chromium and so CI have) is refused because it is slower than the CPU
 * path: measured while scrolling, its redraws held frames for 100 ms and more
 * at 1440 x 900, the CPU path's mostly for none. Tests turn it back on
 * (wallpaperSoftwareGl in test-hooks.ts) before the page loads, to check
 * the shader where there is no GPU.
 *
 * The room (`room: true`, the home page and its slides): the path-traced
 * room of room.ts joins the shader as a layer, rendered in the same
 * WebGL context, over the bottom ROOM_HEIGHT of the page's first screen
 * (the host's [data-room] box, added once the room runs). It needs hardware
 * WebGL 2; anywhere else, or if it fails to load, the wallpaper is drawn
 * alone, as on every other page.
 */
import { computeWallpaper } from '@/lib/wallpaper/tauler-wallpaper.js'
import {
  WALLPAPER,
  ditherPixel,
  wallpaperColours,
  type WallpaperColours,
} from './options'
import {
  createGlRenderer,
  type GlSliceRenderer,
  type Slice,
  type SliceRenderer,
} from './wallpaper-gl'
import { ROOM_HEIGHT } from '../room/room-size'
import type { RoomLayer } from '../room/room'
import { testHooks } from '../test-hooks'

/** CSS px drawn above and below the window: half its height, 200-400. */
function overscan() {
  return Math.min(400, Math.max(200, Math.round(innerHeight / 2)))
}

/** The CPU fallback: computeWallpaper() for the slice, enlarged by devP. */
function create2dRenderer(canvas: HTMLCanvasElement): SliceRenderer {
  const scratch = document.createElement('canvas')
  let pixels: Uint32Array | undefined
  return {
    kind: '2d',
    draw(slice, colours) {
      const devP = ditherPixel(slice.dpr)
      const row0 = Math.floor(slice.top / devP)
      const shift = slice.top - row0 * devP
      const result = computeWallpaper(
        {
          ...WALLPAPER,
          ...colours,
          dpr: slice.dpr,
          width: slice.width / slice.dpr,
          height: (slice.height + shift) / slice.dpr,
          deviceWidth: slice.width,
          deviceHeight: slice.height + shift,
          row0,
        },
        pixels,
      )
      pixels = result.pixels
      scratch.width = result.cols
      scratch.height = result.rows
      scratch
        .getContext('2d')!
        .putImageData(
          new ImageData(
            result.data as ImageDataArray,
            result.cols,
            result.rows,
          ),
          0,
          0,
        )
      const context = canvas.getContext('2d')!
      context.imageSmoothingEnabled = false
      context.drawImage(
        scratch,
        0,
        -shift,
        result.cols * devP,
        result.rows * devP,
      )
    },
    destroy() {},
  }
}

/**
 * Draws the wallpaper into `host`, which must cover the page, and keeps it
 * drawn; with `room`, the room too where it can run. Returns the cleanup.
 * The host's data-renderer says what draws it ("webgl2" or "2d") and
 * data-drawn the slice last drawn ("top height", device px), for tests.
 */
export function mountPageWallpaper(
  host: HTMLElement,
  { room: withRoom = false } = {},
): () => void {
  let canvas: HTMLCanvasElement
  let renderer: SliceRenderer
  let size: [number, number] | null = null
  let colours: WallpaperColours = wallpaperColours(host)
  let placed: { top: number; height: number; dpr: number } | null = null
  let destroyed = false
  let room: { layer: RoomLayer; box: HTMLElement } | null = null
  const abort = new AbortController()

  const resizes = new ResizeObserver((entries) => {
    const entry = entries.find((e) => e.target === canvas)
    if (entry) {
      const box = entry.devicePixelContentBoxSize?.[0]
      size = box
        ? [box.inlineSize, box.blockSize]
        : [
            Math.round(entry.contentRect.width * devicePixelRatio),
            Math.round(entry.contentRect.height * devicePixelRatio),
          ]
    }
    // the canvas's size changed (or the device pixel ratio), or the page's
    place(!!entry)
  })

  function startCanvas(gl: boolean) {
    canvas?.remove()
    canvas = document.createElement('canvas')
    canvas.style.cssText =
      'position:absolute;left:0;top:0;width:100%;height:0;display:block'
    const glRenderer = gl
      ? createGlRenderer(canvas, {
          software: !!testHooks().wallpaperSoftwareGl,
          powerPreference: withRoom ? 'default' : 'low-power',
        })
      : null
    if (gl && !glRenderer) return startCanvas(false)
    renderer = glRenderer ?? create2dRenderer(canvas)
    if (glRenderer && withRoom) void startRoom(glRenderer)
    host.dataset.renderer = renderer.kind
    canvas.addEventListener('webglcontextlost', onLost)
    host.append(canvas)
    size = null
    placed = null
    try {
      resizes.observe(canvas, { box: 'device-pixel-content-box' })
    } catch {
      resizes.observe(canvas) // Safari: CSS size x devicePixelRatio
    }
  }
  function onLost(event: Event) {
    event.preventDefault()
    resizes.unobserve(canvas)
    stopRoom()
    renderer.destroy()
    startCanvas(false)
  }

  /** Adds the room to the shader's context, if it can run there. */
  async function startRoom(glRenderer: GlSliceRenderer) {
    try {
      const { roomSupported, createRoom } = await import('../room/room')
      if (renderer !== glRenderer || !roomSupported(glRenderer.gl)) return
      const box = document.createElement('div')
      box.style.cssText = `position:absolute;left:0;right:0;top:calc(100svh - ${ROOM_HEIGHT});height:${ROOM_HEIGHT}`
      const layer = await createRoom(glRenderer.gl, {
        signal: abort.signal,
        element: box,
        onFrame: draw,
      })
      if (renderer !== glRenderer) return layer.destroy()
      box.dataset.room = ''
      host.append(box)
      room = { layer, box }
      draw()
    } catch (error) {
      if (!abort.signal.aborted)
        console.warn('tauler: the room is unavailable:', error)
    }
  }
  function stopRoom() {
    room?.layer.destroy()
    room?.box.remove()
    room = null
  }
  /** The room's box on the page, in device px; it lays the room out. */
  function layOutRoom(dpr: number) {
    if (!room) return null
    const box = room.box.getBoundingClientRect()
    const top = box.top - host.getBoundingClientRect().top
    try {
      room.layer.layout(
        {
          top: Math.round(top * dpr),
          height: Math.round(box.height * dpr),
          width: size![0],
          dpr,
        },
        colours,
      )
      return room.layer.cells
    } catch (error) {
      console.warn(
        'tauler: the room failed, drawing the wallpaper alone:',
        error,
      )
      stopRoom()
      return null
    }
  }

  function draw() {
    if (!size || !placed) return
    const dpr = devicePixelRatio || 1
    if (canvas.width !== size[0]) canvas.width = size[0]
    if (canvas.height !== size[1]) canvas.height = size[1]
    // where the canvas's top really is, in device px from the page's top
    const top = Math.round(
      (canvas.getBoundingClientRect().top - host.getBoundingClientRect().top) *
        dpr,
    )
    const slice: Slice = { top, width: size[0], height: size[1], dpr }
    try {
      renderer.draw(slice, colours, layOutRoom(dpr))
    } catch (error) {
      if (renderer.kind === '2d') throw error
      console.warn(
        'tauler: wallpaper shader failed, drawing on the CPU:',
        error,
      )
      resizes.unobserve(canvas)
      stopRoom()
      renderer.destroy()
      startCanvas(false)
      return
    }
    host.dataset.drawn = `${top} ${size[1]}`
  }

  /**
   * Moves the canvas around the window if the window nears its edge (or on
   * `force`), and draws it if it moved or changed size.
   */
  function place(force = false) {
    if (destroyed) return
    const pad = overscan()
    const pageTop = -host.getBoundingClientRect().top
    const view = [pageTop, pageTop + innerHeight]
    const height = innerHeight + 2 * pad
    const margin = pad / 4
    const dpr = devicePixelRatio || 1
    const stale =
      !placed ||
      placed.height !== height ||
      placed.dpr !== dpr ||
      (view[0] < placed.top + margin && placed.top > 0) ||
      view[1] > placed.top + placed.height - margin
    if (stale) {
      // top and bottom on whole device px, so the canvas's device size is
      // the same wherever it is placed
      const devP = ditherPixel(dpr)
      const top = Math.floor((Math.max(0, view[0] - pad) * dpr) / devP) * devP
      const devHeight = Math.round(height * dpr)
      placed = { top: top / dpr, height, dpr }
      canvas.style.top = `${top / dpr}px`
      canvas.style.height = `${devHeight / dpr}px`
      // a new size reaches draw() through the ResizeObserver
      if (size?.[1] === devHeight) draw()
    } else if (force) draw()
  }

  let frame = 0
  const onScroll = () => {
    if (!frame)
      frame = requestAnimationFrame(() => {
        frame = 0
        place()
      })
  }
  const onResize = () => place()
  const recolour = () => {
    const next = wallpaperColours(host)
    if (JSON.stringify(next) === JSON.stringify(colours)) return
    colours = next
    draw()
  }

  startCanvas(true)
  addEventListener('scroll', onScroll, { passive: true })
  addEventListener('resize', onResize)
  resizes.observe(host) // the page grows or shrinks
  // the theme: next-themes puts .dark on <html>
  const themes = new MutationObserver(recolour)
  themes.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ['class', 'style', 'data-theme'],
  })
  const scheme = matchMedia('(prefers-color-scheme: dark)')
  scheme.addEventListener('change', recolour)
  place()

  return () => {
    destroyed = true
    abort.abort()
    stopRoom()
    cancelAnimationFrame(frame)
    removeEventListener('scroll', onScroll)
    removeEventListener('resize', onResize)
    resizes.disconnect()
    themes.disconnect()
    scheme.removeEventListener('change', recolour)
    renderer.destroy()
    canvas.remove()
  }
}
