import type { Metadata } from 'next'
import { notFound } from 'next/navigation'
import { createRelativeLink } from 'fumadocs-ui/mdx'
import { getMDXComponents } from '@/components/mdx'
import { DocsTiles } from '@/components/site/DocsTiles'
import { source } from '@/lib/source'

type Props = { params: Promise<{ slug?: string[] }> }

export default async function Page(props: Props) {
  const { slug } = await props.params
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
  return source.generateParams()
}

export async function generateMetadata(props: Props): Promise<Metadata> {
  const { slug } = await props.params
  const page = source.getPage(slug)
  if (!page) notFound()
  return { title: page.data.title, description: page.data.description }
}
