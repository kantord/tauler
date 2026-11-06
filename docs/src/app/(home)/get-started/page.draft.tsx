import type { Metadata } from 'next'
import type { ReactNode } from 'react'
import { codeToHtml } from 'shiki'
import { Desk, Window } from '@/components/site/desk/Desk'
import { taulerCodeTheme } from '@/lib/shiki-theme'
import { Terminal } from './Terminal'

export const metadata: Metadata = {
  title: 'Get started',
  description:
    'Install Tauler, write ~/.config/tauler/layout.op.mdx, and run tauler.',
}

// Built only from APIs the docs describe: the layout file's shape
// (layout-file.md) and useStringStream (data.md).
const EXAMPLE = `// ~/.config/tauler/layout.op.mdx
export default function render() {
  const time = useStringStream("/usr/bin/bash", \`
    while true; do date +"%H:%M"; sleep 1; done
  \`);
  return (
    <root>
      <panel anchor="left" width={272} height={ctx.screen_height}>
        <div class="flex flex-col h-full w-full px-4 py-4">
          <span class="text-[18px] text-foreground">{time}</span>
        </div>
      </panel>
    </root>
  );
}`

export default async function GetStartedSlide() {
  const example = await codeToHtml(EXAMPLE, {
    lang: 'jsx',
    theme: taulerCodeTheme,
  })
  return (
    <Desk n={3} layout="one container, split h">
      <Window title="get started">
        <div className="subs">
          <Window title="layout.op.mdx" sub>
            <div
              className="home-code flex-1"
              // Highlighted at build time by shiki, from a constant.
              dangerouslySetInnerHTML={{ __html: example }}
            />
          </Window>
          <Window title="install" sub>
            <div className="tb">
              <h2 className="h m-0 text-[clamp(32px,4vw,52px)]">
                Three steps.
              </h2>
              <ol className="m-0 flex list-none flex-col gap-3.5 p-0 text-base leading-normal">
                <Step n={1}>Install Tauler from the terminal.</Step>
                <Step n={2}>
                  Write your layout in{' '}
                  <span className="m [overflow-wrap:anywhere]">
                    ~/.config/tauler/layout.op.mdx
                  </span>
                  , or start from the example.
                </Step>
                <Step n={3}>
                  Run <span className="m">tauler</span>. Tauler reloads the file
                  on every save.
                </Step>
              </ol>
              <Terminal />
            </div>
          </Window>
        </div>
      </Window>
    </Desk>
  )
}

function Step({ n, children }: { n: number; children: ReactNode }) {
  return (
    <li className="flex gap-3.5">
      <span className="m font-semibold text-(--tu-or)">{n}</span>
      <span>{children}</span>
    </li>
  )
}
