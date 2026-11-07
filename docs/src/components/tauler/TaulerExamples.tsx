'use client'
import { useEffect } from 'react'

declare global {
  // Read by tauler-web-e2e to tell a page that fell back from one that ran.
  var __taulerErrors: string[] | undefined
}

// The runtime is a build product served from /tauler/, not a dependency of
// this app, so the bundler must never try to resolve it.
const load = (url: string) => import(/* webpackIgnore: true */ url)

/**
 * Boots the wasm module once and mounts every live example on the page.
 *
 * Nothing here decides anything — `boot` and `Mount` are tauler-web's. If the
 * module fails to load (a checkout without a Rust toolchain, most often),
 * every example keeps its screenshot and the page is stale rather than empty.
 *
 * Needs the import map that <TaulerImportMap /> puts in the <head>.
 */
export function TaulerExamples() {
  useEffect(() => {
    globalThis.__taulerErrors = []
    const mounts = [
      ...document.querySelectorAll<HTMLElement>('.tauler-mount'),
    ].filter((el) => el.childElementCount === 0)
    if (mounts.length === 0) return

    void (async () => {
      try {
        const { boot, Mount, registerTransport, setStreamValue } =
          await load('/tauler/runtime.js')
        await boot('/tauler/tauler_web_bg.wasm')

        // A Module, implemented in the page instead of as a subprocess.
        //
        // It is what makes the Slider example on this page work, and it is
        // the whole demonstration: the layout file says
        // `useEvents("tauler-demo-volume")` and
        // `useStringStream("tauler-demo-volume")` and cannot tell whether the
        // other end is a process, a worker, an SSH connection or these four
        // lines. A Transport is whatever fills the map and takes the intents.
        registerTransport(
          'tauler-demo-volume',
          (event: { type?: string; value?: unknown } | null) => {
            if (event?.type === 'set')
              setStreamValue('tauler-demo-volume', null, event.value)
          },
        )

        for (const el of mounts) {
          const name = el.dataset.taulerExample
          const { default: render } = await load(`/tauler/examples/${name}.js`)
          new Mount(el, render).tick()
          document
            .querySelector(`.tauler-fallback[data-tauler-example="${name}"]`)
            ?.setAttribute('hidden', '')
        }
      } catch (e) {
        globalThis.__taulerErrors!.push(
          String((e instanceof Error && e.stack) || e),
        )
        console.warn('tauler: falling back to screenshots', e)
      }
    })()
  }, [])

  // The examples' compiled Tailwind. React hoists it into the <head>, on a
  // full load and on a client-side navigation alike.
  // A build product of `just docs` served from public/, not a module the
  // bundler can import.
  // oxlint-disable-next-line nextjs/no-css-tags
  return <link rel="stylesheet" href="/tauler/tauler.css" precedence="tauler" />
}
