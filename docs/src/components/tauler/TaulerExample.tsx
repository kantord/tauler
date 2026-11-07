import type { ReactNode } from 'react'

// One component's example on the component reference page, as tauler-docgen
// emits it. `live` adds the mount point <TaulerExamples /> renders the real
// component into; the children — its screenshot — are the fallback, hidden
// once the live one is up. The class names and data attributes are what
// tauler-web-e2e and the generated stylesheet (`.tauler-mount`) select on.
export function TaulerExample({
  name,
  live = false,
  children,
}: {
  name: string
  live?: boolean
  children: ReactNode
}) {
  return (
    <div className="mb-5">
      {live && <div className="tauler-mount" data-tauler-example={name} />}
      <div className="tauler-fallback" data-tauler-example={name}>
        {children}
      </div>
    </div>
  )
}
