import type { MetadataRoute } from 'next'
import { WORKSPACES } from '@/components/site/desk/workspaces'
import { source } from '@/lib/source'

export const dynamic = 'force-static'

const SITE = 'https://tauler.dev'

export default function sitemap(): MetadataRoute.Sitemap {
  return [
    { url: `${SITE}/` },
    ...WORKSPACES.map((w) => ({ url: `${SITE}${w.href}` })),
    ...source.getPages().map((page) => ({ url: `${SITE}${page.url}/` })),
  ]
}
