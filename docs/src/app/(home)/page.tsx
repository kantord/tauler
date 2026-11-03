import type { CSSProperties } from 'react'
import { Logo } from '@/components/site/Logo'
import { SiteHeader } from '@/components/site/SiteHeader'
import { WorkspaceNav } from '@/components/site/desk/workspaces'
import { ROOM_HEIGHT } from '@/components/site/whiteroom/room-size'

/**
 * The home page is the logo on the background and nothing else; the landing
 * content is on the workspace slides the header's buttons link to.
 */
export default function HomePage() {
  return (
    <>
      <SiteHeader current="home" lead={<WorkspaceNav />} />
      <main className="mx-auto flex w-full max-w-[1320px] flex-1 flex-col px-4 pt-8 pb-4 sm:px-6">
        <Hero />
        <p className="m mt-auto self-start bg-(--tu-page) px-3 py-2 text-[12px] leading-relaxed text-(--tu-muted)">
          Background: <i>White Room</i>, path-traced live with Erich
          Loftis&apos;s{' '}
          <a
            href="https://github.com/erichlof/THREE.js-PathTracing-Renderer"
            className="underline decoration-(--tu-or) underline-offset-2"
          >
            THREE.js PathTracing Renderer
          </a>{' '}
          (CC0).
        </p>
      </main>
    </>
  )
}

/**
 * The design's logo size, clamp(168px, 26vw, 360px), but never so tall that
 * it reaches the render: that is pinned to the bottom of the window,
 * ROOM_HEIGHT high, and the logo, 2rem below the header, stops 1rem above
 * it. Only short, wide windows (a laptop, a landscape phone) need the cap.
 */
const HERO_SIZE = `clamp(64px, min(max(168px, 26vw), 100svh - ${ROOM_HEIGHT} - var(--tu-header-h) - 3rem), 360px)`

/** The strip continues the logo's bottom row: 16.25 of the 95 visible units. */
function Hero() {
  return (
    <div
      className="mb-8 flex items-end"
      style={{ '--hero': HERO_SIZE } as CSSProperties}
    >
      <h1 className="sr-only">tauler</h1>
      <Logo id="logo-hero" dither={2} className="size-(--hero)" />
      <div
        aria-hidden="true"
        className="hero-strip h-[calc(var(--hero)*16.25/95)] flex-1 border-l-0"
      />
    </div>
  )
}
