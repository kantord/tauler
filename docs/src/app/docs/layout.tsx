import type { ReactNode } from 'react'
import { SiteFooter, SiteHeader } from '@/components/site/SiteHeader'
import { Wallpaper } from '@/components/site/Wallpaper'

export default function Layout({ children }: { children: ReactNode }) {
  return (
    <>
      <Wallpaper />
      <SiteHeader current="docs" />
      {children}
      <SiteFooter />
    </>
  )
}
