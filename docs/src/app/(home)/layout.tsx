import type { ReactNode } from 'react'
import { RoomBackground } from '@/components/site/whiteroom/RoomBackground'

// The home page and its workspace slides share the background, so moving
// between them keeps the room. Each page draws its own header (the slides'
// carries the "focus: …" readout); the slides draw the footer, the home page
// has none.
export default function Layout({ children }: { children: ReactNode }) {
  return (
    <>
      <RoomBackground />
      {children}
    </>
  )
}
