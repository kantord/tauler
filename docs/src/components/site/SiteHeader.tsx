import Link from 'next/link'
import { SearchButton, ThemeToggle } from './controls'

/** The top bar shared by every page: wordmark, site links, search, theme. */
export function SiteHeader({ current }: { current: 'home' | 'docs' }) {
  return (
    <header className="m border-b-[1.5px] border-(--tu-rule) bg-(--tu-page) text-[12px] tracking-[.08em] uppercase">
      <div className="mx-auto flex max-w-[1320px] flex-wrap items-center justify-between gap-3 px-6 py-2">
        <nav aria-label="Site" className="flex items-center gap-[18px]">
          <Link
            href="/"
            className="flex min-h-10 items-center gap-2.5 font-semibold tracking-[.14em] no-underline"
          >
            <span aria-hidden="true" className="mark" />
            tauler
          </Link>
          <span aria-hidden="true" className="h-5 w-[1.5px] bg-(--tu-line)" />
          <Link
            href="/"
            aria-current={current === 'home' ? 'page' : undefined}
            className={
              current === 'home'
                ? 'bg-(--tu-or) px-2.5 py-1 font-semibold text-(--tu-on-or) no-underline'
                : 'text-(--tu-muted) no-underline'
            }
          >
            home
          </Link>
          <Link
            href="/docs/"
            aria-current={current === 'docs' ? 'page' : undefined}
            className={
              current === 'docs'
                ? 'bg-(--tu-or) px-2.5 py-1 font-semibold text-(--tu-on-or) no-underline'
                : 'text-(--tu-muted) no-underline'
            }
          >
            docs
          </Link>
        </nav>
        <span className="flex items-center gap-3">
          <SearchButton />
          <ThemeToggle />
        </span>
      </div>
    </header>
  )
}

export function SiteFooter() {
  return (
    <footer className="m mt-auto border-t-[1.5px] border-(--tu-rule) bg-(--tu-page) text-[12px] tracking-[.08em] uppercase">
      <div className="mx-auto flex max-w-[1320px] flex-wrap justify-between gap-4 px-6 py-[18px] text-(--tu-muted)">
        <span>tauler · declarative workspace for linux</span>
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
