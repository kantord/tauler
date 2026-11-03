'use client'
import {
  createContext,
  use,
  useState,
  type CSSProperties,
  type ReactNode,
} from 'react'
import { SiteFooter, SiteHeader } from '../SiteHeader'
import {
  SlideSteps,
  WORKSPACES,
  WorkspaceNav,
  type WorkspaceNumber,
} from './workspaces'

/**
 * A slide as a tiling desktop: every block is a window and exactly one window
 * has focus. Clicking a title bar focuses its window; the header's
 * "focus: …" readout follows. A focused sub-window also lights the window
 * around it.
 */

type Focus = { title: string; parent?: string }

const FocusContext = createContext<{
  focus: Focus
  setFocus: (focus: Focus) => void
} | null>(null)

/** The title of the window a sub-window sits in. */
const ParentContext = createContext<string | undefined>(undefined)

export function Desk({
  n,
  layout,
  children,
}: {
  n: WorkspaceNumber
  /** How the workspace is tiled, shown on its label line. */
  layout: string
  children: ReactNode
}) {
  const workspace = WORKSPACES.find((w) => w.n === n)!
  const [focus, setFocus] = useState<Focus>({ title: workspace.first })
  return (
    <FocusContext value={{ focus, setFocus }}>
      <SiteHeader
        lead={<WorkspaceNav current={n} />}
        status={
          <span className="text-(--tu-muted)" aria-live="polite">
            focus: <span className="text-(--tu-ink)">{focus.title}</span>
          </span>
        }
      />
      <main className="on-room mx-auto w-full max-w-[1320px] px-4 pt-8 pb-16 sm:px-6">
        <div className="wslabel mt-0">
          <h1>
            {n} · {workspace.name}
          </h1>
          <span>{layout}</span>
        </div>
        {children}
        <SlideSteps n={n} />
      </main>
      <SiteFooter />
    </FocusContext>
  )
}

/**
 * A window: a title bar (a button, because clicking it focuses the window)
 * over its body. `sub` makes it a sub-window, tiled inside another window.
 */
export function Window({
  title,
  sub,
  style,
  children,
}: {
  title: string
  sub?: boolean
  style?: CSSProperties
  children: ReactNode
}) {
  const { focus, setFocus } = use(FocusContext)!
  const parent = use(ParentContext)
  const on = focus.title === title || focus.parent === title
  return (
    <section
      className={`${sub ? 'sub' : 'tile'} ${on ? 'on' : ''}`}
      style={style}
    >
      <button
        type="button"
        className={sub ? 'sbar' : 'bar'}
        onClick={() => setFocus({ title, parent })}
      >
        {sub ? title : <span className="tt">{title}</span>}
      </button>
      <ParentContext value={title}>{children}</ParentContext>
    </section>
  )
}
