'use client'
import { useSearchContext } from 'fumadocs-ui/contexts/search'
import { useTheme } from 'next-themes'
import { useSyncExternalStore } from 'react'
import { Button } from '@/components/ui/button'

/** Looks like the design's search field; opens Fumadocs' search dialog. */
export function SearchButton() {
  const { setOpenSearch } = useSearchContext()
  return (
    <Button
      variant="outline"
      onClick={() => setOpenSearch(true)}
      className="bg-card px-3 font-normal"
    >
      <span className="text-(--tu-muted)">search</span>
      <span className="hidden w-[180px] text-left tracking-normal text-(--tu-muted) normal-case sm:inline">
        layouts, units, data…
      </span>
    </Button>
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
    <Button
      variant="outline"
      onClick={() => setTheme(next)}
      aria-label={`Switch to ${next} mode`}
    >
      <span aria-hidden="true" className="half-moon" />
      {next}
    </Button>
  )
}
