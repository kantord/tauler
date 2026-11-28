'use client'
import { useEffect, useRef } from 'react'
import { cn } from '@/lib/utils'

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
 * drawn with the wallpaper as one image where the GPU allows, its code
 * fetched only there and only after the wallpaper's first paint (see
 * page-wallpaper.ts); `edgeToEdge` drops the scrollbar gutter the site
 * otherwise reserves (see global.css), so the two run to the window's right
 * edge: the body stops at the gutter.
 */
export function Wallpaper({
  edgeToEdge = false,
  room = false,
  phoneBand = false,
}: {
  edgeToEdge?: boolean
  room?: boolean
  /** Below 768px, paint only a short band at the top that fades out, and
   * leave the page its flat colour below it. */
  phoneBand?: boolean
}) {
  const host = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const abort = new AbortController()
    let destroy: (() => void) | undefined
    void import('./wallpaper/page-wallpaper').then(({ mountPageWallpaper }) => {
      if (abort.signal.aborted || !host.current) return
      destroy = mountPageWallpaper(host.current, { room })
    })
    return () => {
      abort.abort()
      destroy?.()
    }
  }, [room])
  return (
    <div
      ref={host}
      aria-hidden="true"
      data-edge-to-edge={edgeToEdge || undefined}
      className={cn(
        'tauler-wallpaper pointer-events-none absolute inset-0 -z-10 overflow-hidden',
        phoneBand && 'tu-wall-band',
      )}
    />
  )
}
