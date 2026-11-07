import type { Metadata } from 'next'
import { ArrowRight } from 'lucide-react'
import Link from 'next/link'
import { Icon } from '@/components/ui/icon'
import { Desk, Window } from '@/components/site/desk/Desk'
import { buttonVariants } from '@/components/ui/button'

export const metadata: Metadata = {
  title: 'Overview',
  description:
    'Tauler draws status bars and panels on Linux from one layout file.',
}

export default function OverviewSlide() {
  return (
    <Desk n={1} layout="split h">
      <div className="flex flex-wrap gap-3">
        <Window title="about" style={{ flex: '3 1 520px' }}>
          <div className="tb justify-center">
            <h2 className="h m-0 text-[clamp(30px,8.5vw,76px)]">
              Declare your <span className="text-(--tu-or)">workspace.</span>
            </h2>
            <p className="m-0 max-w-[30em] text-[19px] leading-[1.55]">
              Tauler draws status bars and panels on Linux from one layout file.
              The file is JSX: on every tick it reads the latest values from its
              streams and returns the tree to draw. Save it and Tauler reloads
              it, no restart.
            </p>
            <div className="flex flex-wrap gap-3">
              <Link
                href="/get-started/"
                className={buttonVariants({ size: 'lg' })}
              >
                Get started
                <Icon icon={ArrowRight} />
              </Link>
              <Link
                href="/docs/"
                className={buttonVariants({ variant: 'outline', size: 'lg' })}
              >
                Read the docs
              </Link>
            </div>
          </div>
        </Window>
        <Window title="file" style={{ flex: '2 1 320px' }}>
          <div className="tb">
            <Row k="<panel>" v="a desktop window of its own" />
            <Row k="useStringStream" v="a subprocess's latest line" />
            <Row k="useJSONStream" v="the same, parsed as JSON" />
            <Row k="<Card />" v="a component: a plain function" />
            <Row k="theme:" v="a shipped theme, in frontmatter" last />
            <Link
              href="/docs/layout-file/"
              className="m flex items-center gap-1.5 border-t border-(--tu-line) pt-3.5 pb-2.5 text-[13px] font-semibold underline decoration-(--tu-or) decoration-[1.5px] underline-offset-3"
            >
              the layout file
              <Icon icon={ArrowRight} />
            </Link>
          </div>
        </Window>
      </div>
    </Desk>
  )
}

function Row({ k, v, last }: { k: string; v: string; last?: boolean }) {
  return (
    <div
      className={`flex items-center justify-between gap-3 py-3.5 ${last ? '' : 'border-b border-(--tu-line)'}`}
    >
      <code className="ic">{k}</code>
      <span className="m text-right text-[13px] text-(--tu-muted)">{v}</span>
    </div>
  )
}
