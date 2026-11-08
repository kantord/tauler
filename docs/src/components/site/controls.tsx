'use client'
import { useSearchContext } from 'fumadocs-ui/contexts/search'
import { Moon, Sun } from 'lucide-react'
import { useTheme } from 'next-themes'
import { useSyncExternalStore } from 'react'
import { Button } from '@/components/ui/button'
import { Icon } from '@/components/ui/icon'

/** Looks like the design's search field; opens Fumadocs' search dialog. */
export function SearchButton() {
  const { setOpenSearch } = useSearchContext()
  return (
    <Button
      variant="outline"
      onClick={() => setOpenSearch(true)}
      className="bg-card px-3 font-normal"
    >
      <span className="text-ink-muted">search</span>
      <span className="hidden w-[180px] text-left tracking-normal text-ink-muted normal-case sm:inline">
        layouts, units, data…
      </span>
    </Button>
  )
}

const noopSubscribe = () => () => {}

/** A square key showing the mode it switches to: a moon, or a sun. */
export function ThemeToggle() {
  const { resolvedTheme, setTheme } = useTheme()
  // The theme is only known in the browser: the server render and the first
  // client render both offer "dark", then the real mode takes over.
  const mounted = useSyncExternalStore(
    noopSubscribe,
    () => true,
    () => false,
  )
  const next = mounted && resolvedTheme === 'dark' ? 'light' : 'dark'
  return (
    <Button
      variant="outline"
      size="icon"
      onClick={() => setTheme(next)}
      aria-label={`Switch to ${next} mode`}
    >
      <Icon icon={next === 'dark' ? Moon : Sun} />
    </Button>
  )
}
