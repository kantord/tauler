import type { Metadata } from 'next'
import { notFound } from 'next/navigation'
import { createRelativeLink } from 'fumadocs-ui/mdx'
import { getMDXComponents } from '@/components/mdx'
import { DocsTiles } from '@/components/site/DocsTiles'
import { Moved } from '@/components/site/Moved'
import { source } from '@/lib/source'

type Props = { params: Promise<{ slug?: string[] }> }

// Pages that moved inside /docs. Published links to the old URLs must keep
// resolving, so each old URL is a static page that sends the reader on.
const MOVED: Record<string, string> = {
  'modules/tauler-i3': '/docs/data/tauler-i3/',
  'modules/tauler-notify': '/docs/data/tauler-notify/',
  'builtin-names': '/docs/data/built-in-streams/',
}
const movedTo = (slug?: string[]) => MOVED[(slug ?? []).join('/')]

export default async function Page(props: Props) {
  const { slug } = await props.params
  const target = movedTo(slug)
  if (target) return <Moved target={target} />
  const page = source.getPage(slug)
  if (!page) notFound()
  const MDX = page.data.body

  return (
    <DocsTiles
      tree={source.getPageTree()}
      url={page.url}
      title={page.data.title}
      description={page.data.description}
      toc={page.data.toc}
    >
      <MDX
        components={getMDXComponents({ a: createRelativeLink(source, page) })}
      />
    </DocsTiles>
  )
}

export function generateStaticParams() {
  return [
    ...source.generateParams(),
    ...Object.keys(MOVED).map((old) => ({ slug: old.split('/') })),
  ]
}

export async function generateMetadata(props: Props): Promise<Metadata> {
  const { slug } = await props.params
  if (movedTo(slug)) return { title: 'Moved' }
  const page = source.getPage(slug)
  if (!page) notFound()
  return { title: page.data.title, description: page.data.description }
}
