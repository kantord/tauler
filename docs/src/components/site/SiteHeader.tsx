import Link from 'next/link'
import type { ReactNode } from 'react'
import { buttonVariants } from '@/components/ui/button'
import { cn } from '@/lib/utils'
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
  menu,
}: {
  current?: 'home' | 'docs'
  lead?: ReactNode
  /** The phone menu button, at the right end. With it, the site links and
   * the theme button move into its drawer below 768px. */
  menu?: ReactNode
}) {
  return (
    <header className="m sticky top-0 z-40 border-b border-rule bg-page text-label uppercase [html[data-bar-stuck]_&]:max-md:border-b-thin">
      <div className="mx-auto flex max-w-page flex-wrap items-center justify-between gap-3 px-6 py-2">
        {lead}
        <nav aria-label="Site" className="flex items-center gap-4">
          <Link
            href="/"
            aria-label="tauler, home"
            className="flex min-h-10 items-center no-underline"
          >
            <Logo id="logo-nav" dither={3.6} className="size-9" />
          </Link>
          <span
            aria-hidden="true"
            className={cn('h-5 border-l border-rule', menu && 'max-md:hidden')}
          />
          <Link
            href="/"
            aria-current={current === 'home' ? 'page' : undefined}
            className={cn(navLink, menu && 'max-md:hidden')}
          >
            home
          </Link>
          <Link
            href="/docs/"
            aria-current={current === 'docs' ? 'page' : undefined}
            className={cn(navLink, menu && 'max-md:hidden')}
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
          <ThemeToggle className={cn(menu && 'max-md:hidden')} />
          {menu}
        </span>
      </div>
    </header>
  )
}

export function SiteFooter() {
  return (
    <footer className="m relative mt-auto border-t border-rule bg-page text-label uppercase">
      <div className="mx-auto flex max-w-page flex-wrap items-center justify-between gap-4 px-6 py-4 text-ink-muted">
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
