import { notFound } from 'next/navigation'
import { docsLlms, getPageMarkdownUrl, source } from '@/lib/source'

export const revalidate = false

type Context = { params: Promise<{ slug?: string[] }> }

export async function GET(_req: Request, { params }: Context) {
  const { slug } = await params
  // Drop the trailing "content.md" segment.
  const page = source.getPage(slug?.slice(0, -1))
  if (!page) notFound()
  return new Response(await docsLlms.page(page), {
    headers: { 'Content-Type': 'text/markdown' },
  })
}

export function generateStaticParams() {
  return source
    .getPages()
    .map((page) => ({ slug: getPageMarkdownUrl(page).segments }))
}
