import type { CSSProperties } from 'react'
import { Logo } from '@/components/site/Logo'
import { SiteHeader } from '@/components/site/SiteHeader'
import { WorkspaceNav } from '@/components/site/desk/workspaces'
import { ROOM_HEIGHT } from '@/components/site/room/room-size'

/**
 * The home page is the logo on the background and nothing else. Under
 * `next dev` the header also carries the buttons to the workspace slides,
 * which are drafts and not published.
 */
export default function HomePage() {
  return (
    <>
      <SiteHeader current="home" lead={<WorkspaceNav />} />
      <main className="mx-auto flex w-full max-w-page flex-1 flex-col px-4 pt-8 pb-4 sm:px-6">
        <Hero />
        <Credit />
      </main>
    </>
  )
}

const UNSPLASH_UTM = '?utm_source=tauler&utm_medium=referral'

/**
 * Credit for the background, kept incidental: small muted type in the
 * bottom-right corner, over the room. The scrim is the page colour at 75%,
 * just enough to hold the muted token's contrast (6.2:1 light and dark on
 * the bare page colour) over the room's darker floor.
 */
function Credit() {
  const link =
    'text-inherit underline decoration-current/40 decoration-1 underline-offset-2 hover:text-ink hover:decoration-current'
  return (
    <p className="mt-auto max-w-[44rem] self-end bg-page/75 px-2 py-1 text-right text-caption leading-snug text-ink-muted backdrop-blur-[2px]">
      Room inspired by a{' '}
      <a
        href={`https://unsplash.com/photos/a-white-room-with-yellow-chairs-and-a-table-k_CZ0eldzvM${UNSPLASH_UTM}`}
        className={link}
      >
        photo
      </a>{' '}
      by{' '}
      <a
        href={`https://unsplash.com/@urban_vintage${UNSPLASH_UTM}`}
        className={link}
      >
        Urban Vintage
      </a>{' '}
      on{' '}
      <a href={`https://unsplash.com/${UNSPLASH_UTM}`} className={link}>
        Unsplash
      </a>
      , path-traced live with Erich Loftis&apos;s{' '}
      <a
        href="https://github.com/erichlof/THREE.js-PathTracing-Renderer"
        className={link}
      >
        THREE.js PathTracing Renderer
      </a>{' '}
      (CC0).
    </p>
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
