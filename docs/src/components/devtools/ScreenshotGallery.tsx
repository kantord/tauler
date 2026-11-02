import { readdirSync } from 'node:fs'
import { join } from 'node:path'

// A devtools screenshot gallery: lists whatever PNGs are committed under
// public/devtools/<suite>/ at build time, so a suite gaining or losing a
// screenshot never needs a matching edit here.
export function ScreenshotGallery({ suite }: { suite: string }) {
  const dir = join(process.cwd(), 'public', 'devtools', suite)
  let names: string[] = []
  try {
    names = readdirSync(dir)
      .filter((n) => n.endsWith('.png'))
      .sort()
  } catch {
    // No directory yet: nothing committed for this suite.
  }

  if (names.length === 0)
    return (
      <p>
        <em>No screenshots committed yet.</em>
      </p>
    )

  return (
    <div className="not-prose flex flex-wrap gap-4">
      {names.map((name) => (
        <figure key={name} className="m-0 max-w-[320px]">
          {/* A static export serves images unoptimised anyway; next/image
              would only add a wrapper. */}
          {/* oxlint-disable-next-line nextjs/no-img-element */}
          <img
            src={`/devtools/${suite}/${name}`}
            alt={name.replace(/\.png$/, '')}
            className="block h-auto max-w-full"
          />
          <figcaption className="mt-1 text-xs break-words opacity-70">
            {name.replace(/\.png$/, '')}
          </figcaption>
        </figure>
      ))}
    </div>
  )
}
