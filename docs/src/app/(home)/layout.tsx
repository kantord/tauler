import type { ReactNode } from 'react'
import { Wallpaper } from '@/components/site/Wallpaper'

// The home page and its workspace slides share the background, the
// wallpaper with the path-traced room, so moving between them keeps the
// room. Each page draws its own header (the slides' carries the "focus: …"
// readout); the slides draw the footer, the home page has none.
export default function Layout({ children }: { children: ReactNode }) {
  return (
    <>
      <Wallpaper edgeToEdge room />
      {children}
    </>
  )
}
