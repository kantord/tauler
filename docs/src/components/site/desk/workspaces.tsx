import Link from 'next/link'

/**
 * The landing content as numbered workspaces, one slide page each. `first` is
 * the title of the window that has focus when the slide opens.
 */
export const WORKSPACES = [
  { n: 1, name: 'overview', href: '/overview/', first: 'about' },
  { n: 2, name: 'principles', href: '/principles/', first: 'principles' },
  { n: 3, name: 'get started', href: '/get-started/', first: 'get started' },
] as const

export type WorkspaceNumber = (typeof WORKSPACES)[number]['n']

/** The header's workspace buttons: one link per slide, `current` lit. */
export function WorkspaceNav({ current }: { current?: WorkspaceNumber }) {
  return (
    <nav aria-label="Workspaces" className="flex gap-1.5">
      {WORKSPACES.map((w) => (
        <Link
          key={w.n}
          href={w.href}
          aria-label={`${w.n}, ${w.name}`}
          aria-current={w.n === current ? 'page' : undefined}
          className={`ws ${w.n === current ? 'cur' : ''}`}
        >
          {w.n}
        </Link>
      ))}
    </nav>
  )
}

/**
 * "← previous / next →" under a slide, styled as the docs pages' pair. The
 * slides run from the home page to the docs: the first slide steps back to
 * `/`, the last one on to `/docs/`.
 */
export function SlideSteps({ n }: { n: WorkspaceNumber }) {
  const i = WORKSPACES.findIndex((w) => w.n === n)
  const previous = WORKSPACES[i - 1] ?? { href: '/', name: 'home' }
  const next = WORKSPACES[i + 1] ?? { href: '/docs/', name: 'docs' }
  return (
    <nav
      aria-label="Slides"
      className="mt-10 grid grid-cols-[repeat(auto-fit,minmax(min(100%,220px),1fr))] gap-2.5"
    >
      <Link href={previous.href} className="pn">
        <small>← previous</small>
        {previous.name}
      </Link>
      <Link href={next.href} className="pn items-end text-right">
        <small>next →</small>
        {next.name}
      </Link>
    </nav>
  )
}
