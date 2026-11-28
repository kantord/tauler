'use client'
import { useSearchContext } from 'fumadocs-ui/contexts/search'
import { Moon, Search, Sun } from 'lucide-react'
import { useTheme } from 'next-themes'
import { useSyncExternalStore } from 'react'
import { Button } from '@/components/ui/button'
import { Icon } from '@/components/ui/icon'
import { cn } from '@/lib/utils'

/** Looks like the design's search field; opens Fumadocs' search dialog. */
export function SearchButton() {
  const { setOpenSearch } = useSearchContext()
  return (
    <Button
      variant="outline"
      onClick={() => setOpenSearch(true)}
      className="bg-card px-3 font-normal max-md:size-10 max-md:p-0"
    >
      <Icon icon={Search} />
      <span className="sr-only">search</span>
      <span className="hidden w-[150px] text-left tracking-normal text-ink-muted normal-case md:inline lg:w-[180px]">
        layouts, units, data…
      </span>
    </Button>
  )
}

const noopSubscribe = () => () => {}

/** The theme in use. It is only known in the browser, so the server render
 * and the first client render have none. */
function useMountedTheme() {
  const { resolvedTheme, setTheme } = useTheme()
  const mounted = useSyncExternalStore(
    noopSubscribe,
    () => true,
    () => false,
  )
  return { theme: mounted ? resolvedTheme : undefined, setTheme }
}

/** A square key showing the mode it switches to: a moon, or a sun. */
export function ThemeToggle({ className }: { className?: string }) {
  const { theme, setTheme } = useMountedTheme()
  const next = theme === 'dark' ? 'light' : 'dark'
  return (
    <Button
      variant="outline"
      size="icon"
      onClick={() => setTheme(next)}
      aria-label={`Switch to ${next} mode`}
      className={cn(className)}
    >
      <Icon icon={next === 'dark' ? Moon : Sun} />
    </Button>
  )
}

/** Both modes side by side, the one in use marked: the drawer's theme
 * control. */
export function ThemeSegment() {
  const { theme, setTheme } = useMountedTheme()
  return (
    <span className="inline-flex border border-input">
      {(['light', 'dark'] as const).map((mode) => (
        <Button
          key={mode}
          variant="segment"
          size="segment"
          aria-label={mode === 'light' ? 'Light mode' : 'Dark mode'}
          aria-pressed={theme === mode}
          onClick={() => setTheme(mode)}
        >
          <Icon icon={mode === 'light' ? Sun : Moon} />
        </Button>
      ))}
    </span>
  )
}
