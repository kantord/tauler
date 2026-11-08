import type { Metadata } from 'next'
import type { ReactNode } from 'react'
import { RootProvider } from 'fumadocs-ui/provider/next'
import SearchDialog from '@/components/search'
import { TaulerImportMap } from '@/components/tauler/TaulerImportMap'
import './global.css'

// Fonts come from npm (ADR 0032), not from next/font/google: Google
// sometimes serves DM Sans from fonts.gstatic.com/l/font?kit=…&skey=…, and
// Next.js 16's font loader fails the build on a URL with `&` in it. The
// --font-tu-* variables are set in global.css.
import '@fontsource-variable/archivo/wdth.css'
import '@fontsource-variable/dm-sans/index.css'
import '@fontsource/ibm-plex-mono/400.css'
import '@fontsource/ibm-plex-mono/500.css'
import '@fontsource/ibm-plex-mono/600.css'
import '@fontsource/ibm-plex-mono/700.css'

export const metadata: Metadata = {
  metadataBase: new URL('https://tauler.dev'),
  title: { template: '%s | tauler', default: 'tauler' },
  description:
    'Declare your Linux workspace — status bars, automations, smart-home controls, data dashboards — in one file, and Tauler keeps reality matching it.',
  icons: { icon: { url: '/favicon.svg', type: 'image/svg+xml' } },
}

// The body is positioned so the wallpaper's host (<Wallpaper />) can cover
// the whole page, not just the window, and scroll with it.
export default function Layout({ children }: { children: ReactNode }) {
  return (
    <html lang="en" suppressHydrationWarning>
      <head>
        <TaulerImportMap />
      </head>
      <body className="tu relative flex min-h-screen flex-col">
        <RootProvider search={{ SearchDialog }}>{children}</RootProvider>
      </body>
    </html>
  )
}
