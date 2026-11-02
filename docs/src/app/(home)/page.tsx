import Link from 'next/link'

export default function HomePage() {
  return (
    <div className="flex flex-1 flex-col items-center justify-center px-4 text-center">
      <h1 className="mb-2 text-3xl font-bold">tauler</h1>
      <p className="mb-6 max-w-xl text-fd-muted-foreground">
        Declare your workspace. tauler keeps it that way.
      </p>
      <Link
        href="/docs/"
        className="rounded-full bg-fd-primary px-4 py-2.5 text-sm font-medium text-fd-primary-foreground"
      >
        DOCS
      </Link>
    </div>
  )
}
