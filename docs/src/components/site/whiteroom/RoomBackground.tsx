'use client'
import { useEffect, useRef, useState } from 'react'
import { Wallpaper } from '../Wallpaper'

/**
 * The home page's background: the standard dithered wallpaper fills the
 * window, and on top of it "White Room, Pixel Traced", path-traced live (see
 * whiteroom.ts), fills the bottom half of the window edge to edge, sized by
 * room-size.ts. Its host covers the whole window, as the wallpaper's does,
 * so the render can put its dither pixels on the wallpaper's grid.
 * Where the renderer cannot run (no WebGL 2 or float render targets, assets
 * that fail to load, a lost GPU context) only the wallpaper is left.
 */
export function RoomBackground() {
  const host = useRef<HTMLDivElement>(null)
  const [failed, setFailed] = useState(false)

  useEffect(() => {
    if (failed || !host.current) return
    const container = host.current
    const abort = new AbortController()
    let destroy: (() => void) | undefined
    void import('./whiteroom')
      .then(async ({ roomSupported, mountRoom }) => {
        if (!roomSupported()) throw new Error('no hardware WebGL 2')
        const room = await mountRoom(container, {
          signal: abort.signal,
          onLost: () => setFailed(true),
        })
        if (abort.signal.aborted) room.destroy()
        else destroy = room.destroy
      })
      .catch((error: unknown) => {
        if (abort.signal.aborted) return
        console.warn('tauler: home background unavailable:', error)
        setFailed(true)
      })
    return () => {
      abort.abort()
      destroy?.()
    }
  }, [failed])

  return (
    <>
      <Wallpaper edgeToEdge />
      {failed ? null : (
        <div
          ref={host}
          aria-hidden="true"
          data-room
          className="pointer-events-none fixed inset-0 -z-10"
        />
      )}
    </>
  )
}
