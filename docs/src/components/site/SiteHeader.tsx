import Link from 'next/link'
import type { ReactNode } from 'react'
import { buttonVariants } from '@/components/ui/button'
import { SearchButton, ThemeToggle } from './controls'
import { GitHubIcon } from './icons'
import { Logo } from './Logo'

const navLink = buttonVariants({ variant: 'nav', size: 'inline' })

/**
 * The top bar shared by every page: the logo (it stands in for the word
 * "tauler"), site links, search, GitHub and theme. The home page and the
 * slides pass their workspace buttons as `lead`. The slides set no `current`:
 * their workspace button is lit instead. The footer repeats the GitHub link
 * as "Source", next to the changelog; the home page has no footer.
 */
export function SiteHeader({
  current,
  lead,
}: {
  current?: 'home' | 'docs'
  lead?: ReactNode
}) {
  return (
    <header className="m sticky top-0 z-40 border-b-heavy border-rule bg-page text-label tracking-[.08em] uppercase">
      <div className="mx-auto flex max-w-[1320px] flex-wrap items-center justify-between gap-3 px-6 py-2">
        {lead}
        <nav aria-label="Site" className="flex items-center gap-4">
          <Link
            href="/"
            aria-label="tauler, home"
            className="flex min-h-10 items-center no-underline"
          >
            <Logo id="logo-nav" dither={3.6} className="size-9" />
          </Link>
          <span aria-hidden="true" className="h-5 border-l-heavy border-rule" />
          <Link
            href="/"
            aria-current={current === 'home' ? 'page' : undefined}
            className={navLink}
          >
            home
          </Link>
          <Link
            href="/docs/"
            aria-current={current === 'docs' ? 'page' : undefined}
            className={navLink}
          >
            docs
          </Link>
        </nav>
        <span className="flex flex-wrap items-center gap-3">
          <SearchButton />
          <a
            href="https://github.com/kantord/tauler"
            aria-label="tauler on GitHub"
            className={buttonVariants({ variant: 'outline', size: 'icon' })}
          >
            <GitHubIcon />
          </a>
          <ThemeToggle />
        </span>
      </div>
    </header>
  )
}

export function SiteFooter() {
  return (
    <footer className="m relative mt-auto border-t-heavy border-rule bg-page text-label tracking-[.08em] uppercase">
      <div className="mx-auto flex max-w-[1320px] flex-wrap items-center justify-between gap-4 px-6 py-4 text-ink-muted">
        <span className="flex items-center gap-2">
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
