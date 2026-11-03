import type { Metadata } from 'next'
import type { ReactNode } from 'react'
import { Desk, Window } from '@/components/site/desk/Desk'

export const metadata: Metadata = {
  title: 'Principles',
  description:
    'A function per tick, streams instead of effects, and a clean reload on every save.',
}

export default function PrinciplesSlide() {
  return (
    <Desk n={2} layout="one container, split h">
      <Window title="principles">
        <div className="subs">
          <Principle title="tick" n="1" heading="A function per tick">
            The render function runs top to bottom on every tick and returns a
            fresh tree. No component state, no effects, no lifecycle: closer to
            a spreadsheet than to React.
          </Principle>
          <Principle title="streams" n="2" heading="Streams, not effects">
            A stream names a program and hands you its latest output. tauler
            owns the subprocess, so there is no subscription or cleanup code to
            write.
          </Principle>
          <Principle title="reload" n="3" heading="Save, and it reloads">
            The file is watched. A reload restarts every subprocess and clears
            every stream: each save is a clean start, never a half-updated bar.
          </Principle>
        </div>
      </Window>
    </Desk>
  )
}

function Principle({
  title,
  n,
  heading,
  children,
}: {
  title: string
  n: '1' | '2' | '3'
  heading: string
  children: ReactNode
}) {
  return (
    <Window title={title} sub>
      <div className="tb">
        <div
          aria-hidden="true"
          className={`h text-[88px] ${n === '1' ? 'text-(--tu-or)' : ''} ${n === '3' ? 'outline-num' : ''}`}
        >
          {n}
        </div>
        <h2 className="h m-0 text-[26px]">{heading}</h2>
        <p className="m-0 text-base leading-[1.6] text-(--tu-muted)">
          {children}
        </p>
      </div>
    </Window>
  )
}
