'use client'
import { useEffect, useRef } from 'react'

/**
 * The dithered wallpaper behind every page (src/lib/wallpaper, vendored from
 * the design unchanged). It shows only where no window covers it: the gaps
 * between tiles.
 *
 * The host is fixed to the viewport rather than the page's full height, so
 * the canvas stays window-sized: a page-sized canvas costs width × height ×
 * dpr² × 4 bytes, and the longest docs pages pass the browser's canvas limit.
 * The price is that the wallpaper does not scroll with the page.
 *
 * The module watches the class on <html> (where next-themes puts `.dark`), so
 * it re-renders on a theme switch by itself.
 *
 * `edgeToEdge` drops the scrollbar gutter the site otherwise reserves (see
 * global.css), so the wallpaper, and anything else fixed to the window, runs
 * to its right edge: Chrome clips fixed elements at the root's gutter, so
 * they cannot run under it.
 */
/**
 * The wallpaper's options. The home page's render (whiteroom.ts) reads its
 * pixel size from them, to draw its dither pixels on the same grid.
 */
export const WALLPAPER_OPTIONS = { intensity: 0.1, seed: 18 } as const

export function Wallpaper({ edgeToEdge = false }: { edgeToEdge?: boolean }) {
  const host = useRef<HTMLDivElement>(null)
  useEffect(() => {
    let destroy: (() => void) | undefined
    let cancelled = false
    void import('@/lib/wallpaper/tauler-wallpaper.js').then(
      ({ mountWallpaper }) => {
        if (cancelled || !host.current) return
        destroy = mountWallpaper(host.current, WALLPAPER_OPTIONS).destroy
      },
    )
    return () => {
      cancelled = true
      destroy?.()
    }
  }, [])
  return (
    <div
      ref={host}
      aria-hidden="true"
      data-edge-to-edge={edgeToEdge || undefined}
      className="pointer-events-none fixed inset-0 -z-10"
    />
  )
}
