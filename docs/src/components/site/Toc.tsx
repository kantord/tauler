'use client'
import { ChevronDown, List } from 'lucide-react'
import { useEffect, useRef, useState, type ReactNode } from 'react'
import { buttonVariants } from '@/components/ui/button'
import { Icon } from '@/components/ui/icon'
import { cn } from '@/lib/utils'

export type Heading = { url: string; title: ReactNode; depth: number }

// The solid focus ring, drawn inside the control: the bar and the list rows
// run to the edges of the screen.
const FOCUS =
  'focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-ring focus-visible:outline-solid'

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
    <ul className="m-0 list-none p-0">
      {headings.map((h) => {
        const active = h.url.slice(1) === activeId
        return (
          <li key={h.url} className="m-0">
            <a
              href={h.url}
              onClick={onNavigate}
              aria-current={active ? 'location' : undefined}
              className={cn(
                // The rows' left borders join into the rail. In the article (the
                // dropdown) a heading's inline code stays plain text.
                '[&_code]:border-0 [&_code]:bg-transparent [&_code]:p-0',
                FOCUS,
                'block truncate border-l border-line pr-2 text-sm font-medium text-ink no-underline hover:border-orange hover:text-orange-ink',
                size === 'docked' ? 'leading-8' : 'leading-10',
                h.depth > 2 ? 'pl-7 font-normal text-ink-muted' : 'pl-3',
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
    <nav className="scroll px-3 pt-2 pb-3" aria-label="On this page contents">
      <TocList headings={headings} activeId={id} size="docked" />
    </nav>
  )
}

/**
 * Below 768px, under the page's lead: before it sticks, a keycap button with
 * a popover list over the article, so opening it shifts nothing. Once stuck
 * under the header it is a full-width strip with a progress ring and the
 * heading being read. The bar keeps one fixed height in both states, so
 * sticking moves no content. The negative margins undo the article's padding,
 * so the strip and the list run edge to edge.
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
      {
        rootMargin: `-${getComputedStyle(document.documentElement).getPropertyValue('--tu-header-stuck-h')} 0px 0px 0px`,
      },
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
          'sticky top-[var(--tu-header-stuck-h)] z-30 -mx-4 mb-5 h-[42px] sm:-mx-8 md:hidden',
          stuck ? 'border-b border-rule bg-card' : 'px-4 sm:px-8',
          stuck && open && 'border-b-thin border-line',
        )}
      >
        <button
          type="button"
          aria-label="On this page"
          aria-expanded={open}
          onClick={() => setOpen((o) => !o)}
          className={cn(
            stuck
              ? cn(
                  FOCUS,
                  'flex h-10 w-full cursor-pointer items-center gap-2 px-4 font-mono text-xs font-medium text-ink sm:px-8',
                )
              : buttonVariants({ variant: 'keycap' }),
          )}
        >
          {stuck ? (
            <>
              <svg
                viewBox="0 0 20 20"
                className="size-4 shrink-0 -rotate-90"
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
              <span className="min-w-0 flex-1 truncate text-left">
                {reading}
              </span>
            </>
          ) : (
            <>
              <Icon icon={List} />
              On this page
            </>
          )}
          <Icon icon={ChevronDown} />
        </button>
        {open && (
          <div className="absolute inset-x-0 top-full max-h-[calc(100vh-140px)] overflow-y-auto border-b border-rule bg-card px-4 pt-2 pb-3 sm:px-8">
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
