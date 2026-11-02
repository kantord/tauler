import Link from 'next/link'

export default function HomePage() {
  return (
    <div className="mx-auto w-full max-w-[1320px] px-6 pt-7 pb-16">
      <main className="tile on">
        <div className="bar">
          <span className="tt">tauler</span>
        </div>
        <div className="art">
          <h1 className="h">Declare your workspace</h1>
          <p className="lead">
            Status bars, automations, smart-home controls and data dashboards,
            in one file. tauler keeps reality matching it.
          </p>
          <Link href="/docs/" className="pn max-w-[280px]">
            <small>read →</small>
            Docs
          </Link>
        </div>
      </main>
    </div>
  )
}
