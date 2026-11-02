import type { Metadata } from 'next'
import type { ReactNode } from 'react'
import { RootProvider } from 'fumadocs-ui/provider/next'
import SearchDialog from '@/components/search'
import { TaulerImportMap } from '@/components/tauler/TaulerImportMap'
import './global.css'

export const metadata: Metadata = {
  metadataBase: new URL('https://tauler.dev'),
  title: { template: '%s | tauler', default: 'tauler' },
  description:
    'Declare your Linux workspace — status bars, automations, smart-home controls, data dashboards — in one file, and tauler keeps reality matching it.',
  icons: { icon: { url: '/favicon.svg', type: 'image/svg+xml' } },
}

export default function Layout({ children }: { children: ReactNode }) {
  return (
    <html lang="en" suppressHydrationWarning>
      <head>
        <TaulerImportMap />
      </head>
      <body className="flex min-h-screen flex-col">
        <RootProvider search={{ SearchDialog }}>{children}</RootProvider>
      </body>
    </html>
  )
}
