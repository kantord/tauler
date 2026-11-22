// A page that moved. GitHub Pages has no server-side redirects, so each old
// URL is a static meta-refresh page that names the new one.
export function Moved({ target }: { target: string }) {
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
