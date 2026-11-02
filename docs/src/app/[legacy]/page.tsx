// The docs moved from the site root to /docs when the landing page took over
// `/`; published links to the old URLs must keep resolving. GitHub Pages has
// no server-side redirects, so each old URL is a static meta-refresh page.
const MOVED = [
  'layout-file',
  'elements',
  'data',
  'layout',
  'components',
  'macos',
]

export const dynamicParams = false

export function generateStaticParams() {
  return MOVED.map((legacy) => ({ legacy }))
}

export default async function Page(props: {
  params: Promise<{ legacy: string }>
}) {
  const { legacy } = await props.params
  const target = `/docs/${legacy}/`
  return (
    <>
      <meta httpEquiv="refresh" content={`0;url=${target}`} />
      <meta name="robots" content="noindex" />
      <link rel="canonical" href={`https://tauler.dev${target}`} />
      <p className="p-4">
        Moved to <a href={target}>{target}</a>.
      </p>
    </>
  )
}
