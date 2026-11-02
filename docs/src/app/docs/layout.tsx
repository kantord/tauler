import type { ReactNode } from 'react'
import { SiteFooter, SiteHeader } from '@/components/site/SiteHeader'

export default function Layout({ children }: { children: ReactNode }) {
  return (
    <>
      <SiteHeader current="docs" />
      {children}
      <SiteFooter />
    </>
  )
}
