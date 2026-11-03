import type { Metadata } from 'next'
import { notFound } from 'next/navigation'
import { SiteFooter, SiteHeader } from '@/components/site/SiteHeader'
import { Wallpaper } from '@/components/site/Wallpaper'
import { getMDXComponents } from '@/components/mdx'
import { ScreenshotGallery } from '@/components/devtools/ScreenshotGallery'
import { devtools } from '@/lib/source'

// Maintainer-only pages: no sidebar, no search, never linked from the docs.
type Props = { params: Promise<{ slug?: string[] }> }

export default async function Page(props: Props) {
  const { slug } = await props.params
  const page = devtools.getPage(slug)
  if (!page) notFound()
  const MDX = page.data.body
  return (
    <>
      <Wallpaper />
      <SiteHeader current="docs" />
      <div className="mx-auto w-full max-w-[1320px] px-6 pt-7 pb-16">
        <main className="tile on">
          <div className="bar">
            <span className="tt">devtools</span>
          </div>
          <article className="art max-w-none">
            <h1 className="h">{page.data.title}</h1>
            {page.data.description && (
              <p className="lead">{page.data.description}</p>
            )}
            <MDX components={getMDXComponents({ ScreenshotGallery })} />
          </article>
        </main>
      </div>
      <SiteFooter />
    </>
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
