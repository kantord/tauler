import type { Metadata } from 'next'
import { notFound } from 'next/navigation'
import { HomeLayout } from 'fumadocs-ui/layouts/home'
import {
  DocsBody,
  DocsDescription,
  DocsTitle,
} from 'fumadocs-ui/layouts/docs/page'
import { getMDXComponents } from '@/components/mdx'
import { ScreenshotGallery } from '@/components/devtools/ScreenshotGallery'
import { baseOptions } from '@/lib/layout.shared'
import { devtools } from '@/lib/source'

// Maintainer-only pages: no sidebar, no search, never linked from the docs.
type Props = { params: Promise<{ slug?: string[] }> }

export default async function Page(props: Props) {
  const { slug } = await props.params
  const page = devtools.getPage(slug)
  if (!page) notFound()
  const MDX = page.data.body
  return (
    <HomeLayout {...baseOptions()}>
      <main className="mx-auto w-full max-w-[1100px] px-6 py-10">
        <DocsTitle>{page.data.title}</DocsTitle>
        <DocsDescription>{page.data.description}</DocsDescription>
        <DocsBody>
          <MDX components={getMDXComponents({ ScreenshotGallery })} />
        </DocsBody>
      </main>
    </HomeLayout>
  )
}

export function generateStaticParams() {
  return devtools.generateParams()
}

export async function generateMetadata(props: Props): Promise<Metadata> {
  const { slug } = await props.params
  const page = devtools.getPage(slug)
  if (!page) notFound()
  return {
    title: page.data.title,
    description: page.data.description,
    robots: { index: false },
  }
}
