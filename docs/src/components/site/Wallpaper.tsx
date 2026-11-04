'use client'
import { useEffect, useRef } from 'react'

/**
 * The dithered wallpaper behind every page. It shows only where no window
 * covers it: the gaps between tiles.
 *
 * It is anchored to the page and scrolls with it: the host covers the whole
 * page (the body is positioned, see src/app/layout.tsx) and
 * wallpaper/page-wallpaper.ts keeps a window-sized canvas drawn where the
 * window is, by a WebGL shader or, without WebGL 2, by the wallpaper module
 * (src/lib/wallpaper) on the CPU. It re-renders on a theme switch by itself
 * (next-themes puts `.dark` on <html>).
 *
 * `room` adds the path-traced room over the bottom of the first screen,
 * drawn with the wallpaper as one image where the GPU allows (see
 * page-wallpaper.ts); `edgeToEdge` drops the scrollbar gutter the site
 * otherwise reserves (see global.css), so the two run to the window's right
 * edge: the body stops at the gutter.
 */
export function Wallpaper({
  edgeToEdge = false,
  room = false,
}: {
  edgeToEdge?: boolean
  room?: boolean
}) {
  const host = useRef<HTMLDivElement>(null)
  useEffect(() => {
    let destroy: (() => void) | undefined
    let cancelled = false
    // fetched alongside the wallpaper rather than after it: three.js and the
    // room's shaders are most of the time to the room's first frame
    if (room) void import('./whiteroom/whiteroom').catch(() => {})
    void import('./wallpaper/page-wallpaper').then(({ mountPageWallpaper }) => {
      if (cancelled || !host.current) return
      destroy = mountPageWallpaper(host.current, { room })
    })
    return () => {
      cancelled = true
      destroy?.()
    }
  }, [room])
  return (
    <div
      ref={host}
      aria-hidden="true"
      data-edge-to-edge={edgeToEdge || undefined}
      className="tauler-wallpaper pointer-events-none absolute inset-0 -z-10 overflow-hidden"
    />
  )
}
