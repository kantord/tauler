import type { ReactNode } from 'react'
import { SiteFooter, SiteHeader } from '@/components/site/SiteHeader'
import { DocsMenu } from '@/components/site/DocsMenu'
import { groupsOf } from '@/components/site/DocsNav'
import { Wallpaper } from '@/components/site/Wallpaper'
import { source } from '@/lib/source'

export default function Layout({ children }: { children: ReactNode }) {
  return (
    <>
      <Wallpaper phoneBand />
      <SiteHeader
        current="docs"
        menu={<DocsMenu groups={groupsOf(source.getPageTree())} />}
      />
      {children}
      <SiteFooter />
    </>
  )
}
