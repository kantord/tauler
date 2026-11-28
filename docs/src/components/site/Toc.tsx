'use client'
import { ChevronDown, List } from 'lucide-react'
import { useEffect, useRef, useState, type ReactNode } from 'react'
import { buttonVariants } from '@/components/ui/button'
import { Icon } from '@/components/ui/icon'
import { cn } from '@/lib/utils'

export type Heading = { url: string; title: ReactNode; depth: number }

// Where a heading counts as "being read": just under the header and the
// stuck heading bar.
const READING_LINE = 140

/** The id of the heading being read, and how far down the page's `main`
 * the reader is (0 to 1). */
function useReading(headings: Heading[]) {
  const [id, setId] = useState<string>()
  const [progress, setProgress] = useState(0)
  const key = headings.map((h) => h.url).join()
  useEffect(() => {
    const els = key
      .split(',')
      .map((u) => document.getElementById(u.slice(1)))
      .filter((e): e is HTMLElement => e !== null)
    const update = () => {
      let current = els[0]?.id
      for (const el of els) {
        if (el.getBoundingClientRect().top > READING_LINE) break
        current = el.id
      }
      setId(current)
      const rect = document.querySelector('main')?.getBoundingClientRect()
      if (rect) {
        const span = rect.height - window.innerHeight
        setProgress(span > 0 ? Math.min(1, Math.max(0, -rect.top / span)) : 0)
      }
    }
    update()
    window.addEventListener('scroll', update, { passive: true })
    window.addEventListener('resize', update)
    return () => {
      window.removeEventListener('scroll', update)
      window.removeEventListener('resize', update)
    }
  }, [key])
  return { id, progress }
}

/** The designed list: a 2px rail on the left, rows that are real links to
 * the headings, sub-headings stepped in, the heading being read bold with
 * the rail turned orange. */
export function TocList({
  headings,
  activeId,
  size,
  onNavigate,
}: {
  headings: Heading[]
  activeId?: string
  size: 'docked' | 'dropdown'
  onNavigate?: () => void
}) {
  return (
    <ul className="m-0 list-none border-l border-line p-0">
      {headings.map((h) => {
        const active = h.url.slice(1) === activeId
        return (
          <li key={h.url}>
            <a
              href={h.url}
              onClick={onNavigate}
              aria-current={active ? 'location' : undefined}
              className={cn(
                '-ml-[var(--border-width-heavy)] flex items-center border-l border-transparent pr-3 text-ink-muted no-underline hover:text-orange-ink',
                size === 'docked' ? 'min-h-8 text-sm' : 'min-h-10 text-base',
                h.depth > 2 ? 'pl-6' : 'pl-3',
                active && 'border-orange font-bold text-ink',
              )}
            >
              {h.title}
            </a>
          </li>
        )
      })}
    </ul>
  )
}

/** The docked "on this page" window (768px and up). */
export function DockedToc({ headings }: { headings: Heading[] }) {
  const { id } = useReading(headings)
  return (
    <nav className="scroll px-2 py-3" aria-label="On this page contents">
      <TocList headings={headings} activeId={id} size="docked" />
    </nav>
  )
}

/**
 * Below 768px: before it sticks, a keycap button with a popover list over the
 * article, so opening it shifts nothing. Once stuck under the header it is a
 * full-width strip with a progress ring and the heading being read. The bar
 * keeps one fixed height in both states, so sticking moves no content.
 */
export function HeadingBar({ headings }: { headings: Heading[] }) {
  const sentinel = useRef<HTMLDivElement>(null)
  const wrap = useRef<HTMLDivElement>(null)
  const [stuck, setStuck] = useState(false)
  const [open, setOpen] = useState(false)
  const { id, progress } = useReading(headings)

  useEffect(() => {
    const io = new IntersectionObserver(
      ([entry]) => {
        const s = !entry.isIntersecting && entry.boundingClientRect.top < 0
        setStuck(s)
        // The header drops to a 1px line while the bar is stuck.
        document.documentElement.toggleAttribute('data-bar-stuck', s)
      },
      { rootMargin: '-57px 0px 0px 0px' },
    )
    io.observe(sentinel.current!)
    return () => {
      io.disconnect()
      document.documentElement.removeAttribute('data-bar-stuck')
    }
  }, [])

  useEffect(() => {
    if (!open) return
    const onDown = (e: PointerEvent) => {
      if (!wrap.current?.contains(e.target as Node)) setOpen(false)
    }
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && setOpen(false)
    document.addEventListener('pointerdown', onDown)
    document.addEventListener('keydown', onKey)
    return () => {
      document.removeEventListener('pointerdown', onDown)
      document.removeEventListener('keydown', onKey)
    }
  }, [open])

  const reading = headings.find((h) => h.url.slice(1) === id)?.title
  return (
    <>
      <div ref={sentinel} aria-hidden="true" className="h-px md:hidden" />
      <div
        ref={wrap}
        className={cn(
          'sticky top-[57px] z-30 h-12 md:hidden',
          stuck
            ? 'border-y-thin border-line border-b-rule bg-card'
            : 'flex items-center px-4',
        )}
      >
        <button
          type="button"
          aria-label="On this page"
          aria-expanded={open}
          onClick={() => setOpen((o) => !o)}
          className={
            stuck
              ? 'flex h-full w-full cursor-pointer items-center gap-3 px-4 text-left text-sm font-semibold text-ink'
              : buttonVariants({ variant: 'keycap' })
          }
        >
          {stuck ? (
            <>
              <svg
                viewBox="0 0 20 20"
                className="size-5 shrink-0 -rotate-90"
                aria-hidden="true"
              >
                <circle
                  cx="10"
                  cy="10"
                  r="8"
                  fill="none"
                  strokeWidth="2.5"
                  className="stroke-line"
                />
                <circle
                  cx="10"
                  cy="10"
                  r="8"
                  fill="none"
                  strokeWidth="2.5"
                  pathLength={1}
                  strokeDasharray={`${progress} 1`}
                  className="stroke-orange"
                />
              </svg>
              <span className="min-w-0 flex-1 truncate">{reading}</span>
              <Icon icon={ChevronDown} />
            </>
          ) : (
            <>
              <Icon icon={List} />
              On this page
            </>
          )}
        </button>
        {open && (
          <div className="absolute inset-x-0 top-full max-h-[calc(100vh-57px-48px-16px)] overflow-y-auto border-b border-rule bg-card p-3 shadow-none">
            <TocList
              headings={headings}
              activeId={id}
              size="dropdown"
              onNavigate={() => setOpen(false)}
            />
          </div>
        )}
      </div>
    </>
  )
}
