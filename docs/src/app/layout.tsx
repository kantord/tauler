import type { Metadata } from 'next'
import type { ReactNode } from 'react'
import { Archivo, DM_Sans, IBM_Plex_Mono } from 'next/font/google'
import { RootProvider } from 'fumadocs-ui/provider/next'
import SearchDialog from '@/components/search'
import { TaulerImportMap } from '@/components/tauler/TaulerImportMap'
import './global.css'

// Downloaded at build time and served from the site itself.
const display = Archivo({
  subsets: ['latin'],
  axes: ['wdth'],
  variable: '--font-tu-display',
})
const text = DM_Sans({
  subsets: ['latin'],
  weight: ['400', '500', '700'],
  variable: '--font-tu-text',
})
const mono = IBM_Plex_Mono({
  subsets: ['latin'],
  weight: ['400', '500', '600', '700'],
  variable: '--font-tu-mono',
})

export const metadata: Metadata = {
  metadataBase: new URL('https://tauler.dev'),
  title: { template: '%s | tauler', default: 'tauler' },
  description:
    'Declare your Linux workspace — status bars, automations, smart-home controls, data dashboards — in one file, and tauler keeps reality matching it.',
  icons: { icon: { url: '/favicon.svg', type: 'image/svg+xml' } },
}

export default function Layout({ children }: { children: ReactNode }) {
  return (
    <html
      lang="en"
      className={`${display.variable} ${text.variable} ${mono.variable}`}
      suppressHydrationWarning
    >
      <head>
        <TaulerImportMap />
      </head>
      <body className="tu flex min-h-screen flex-col">
        <RootProvider search={{ SearchDialog }}>{children}</RootProvider>
      </body>
    </html>
  )
}
