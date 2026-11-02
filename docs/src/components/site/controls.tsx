'use client'
import { useSearchContext } from 'fumadocs-ui/contexts/search'
import { useTheme } from 'next-themes'
import { useSyncExternalStore } from 'react'

/** Looks like the design's search field; opens Fumadocs' search dialog. */
export function SearchButton() {
  const { setOpenSearch } = useSearchContext()
  return (
    <button
      type="button"
      onClick={() => setOpenSearch(true)}
      className="m flex min-h-10 cursor-pointer items-center gap-2 border-[1.5px] border-(--tu-rule) bg-(--tu-body) px-3 text-[12px] tracking-[.08em] uppercase"
    >
      <span className="text-(--tu-muted)">search</span>
      <span className="hidden w-[180px] text-left tracking-normal text-(--tu-muted) normal-case sm:inline">
        layouts, units, data…
      </span>
    </button>
  )
}

const noopSubscribe = () => () => {}

export function ThemeToggle() {
  const { resolvedTheme, setTheme } = useTheme()
  // The theme is only known in the browser: the server render and the first
  // client render both say "dark", then the real label takes over.
  const mounted = useSyncExternalStore(
    noopSubscribe,
    () => true,
    () => false,
  )
  const next = mounted && resolvedTheme === 'dark' ? 'light' : 'dark'
  return (
    <button
      type="button"
      onClick={() => setTheme(next)}
      aria-label={`Switch to ${next} mode`}
      className="m flex min-h-10 cursor-pointer items-center gap-2 border-[1.5px] border-(--tu-rule) bg-transparent px-3.5 text-[12px] font-semibold tracking-[.08em] text-(--tu-ink) uppercase"
    >
      <span aria-hidden="true" className="half-moon" />
      {next}
    </button>
  )
}
