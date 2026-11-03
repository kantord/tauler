import Link from 'next/link'
import type { ReactNode } from 'react'
import { SearchButton, ThemeToggle } from './controls'
import { Logo } from './Logo'

const linkClass = (on: boolean) =>
  on
    ? 'bg-(--tu-or) px-2.5 py-1 font-semibold text-(--tu-on-or) no-underline'
    : 'no-underline'

/**
 * The top bar shared by every page: the logo (it stands in for the word
 * "tauler"), site links, search and theme. The home page and the slides pass
 * their workspace buttons as `lead`, and the slides their "focus: …" readout
 * as `status`. The slides set no `current`: their workspace button is lit
 * instead.
 */
export function SiteHeader({
  current,
  lead,
  status,
}: {
  current?: 'home' | 'docs'
  lead?: ReactNode
  status?: ReactNode
}) {
  return (
    <header className="m sticky top-0 z-40 border-b-[1.5px] border-(--tu-rule) bg-(--tu-page) text-[12px] tracking-[.08em] uppercase">
      <div className="mx-auto flex max-w-[1320px] flex-wrap items-center justify-between gap-3 px-6 py-2">
        {lead}
        <nav aria-label="Site" className="flex items-center gap-[18px]">
          <Link
            href="/"
            aria-label="tauler, home"
            className="flex min-h-10 items-center no-underline"
          >
            <Logo id="logo-nav" dither={3.6} className="size-9" />
          </Link>
          <span aria-hidden="true" className="h-5 w-[1.5px] bg-(--tu-line)" />
          <Link
            href="/"
            aria-current={current === 'home' ? 'page' : undefined}
            className={linkClass(current === 'home')}
          >
            home
          </Link>
          <Link
            href="/docs/"
            aria-current={current === 'docs' ? 'page' : undefined}
            className={linkClass(current === 'docs')}
          >
            docs
          </Link>
        </nav>
        <span className="flex flex-wrap items-center gap-3">
          {status}
          <SearchButton />
          <ThemeToggle />
        </span>
      </div>
    </header>
  )
}

export function SiteFooter() {
  return (
    <footer className="m relative mt-auto border-t-[1.5px] border-(--tu-rule) bg-(--tu-page) text-[12px] tracking-[.08em] uppercase">
      <div className="mx-auto flex max-w-[1320px] flex-wrap items-center justify-between gap-4 px-6 py-[18px] text-(--tu-muted)">
        <span className="flex items-center gap-2.5">
          <Logo id="logo-foot" dither={4.5} label="tauler" className="size-6" />
          declarative workspace for linux
        </span>
        <span className="flex flex-wrap gap-6">
          <Link href="/">Home</Link>
          <Link href="/docs/">Docs</Link>
          <a href="https://github.com/kantord/tauler">Source</a>
          <a href="https://github.com/kantord/tauler/blob/main/CHANGELOG.md">
            Changelog
          </a>
        </span>
      </div>
    </footer>
  )
}
