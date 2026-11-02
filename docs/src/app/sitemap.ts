import type { MetadataRoute } from 'next'
import { source } from '@/lib/source'

export const dynamic = 'force-static'

const SITE = 'https://tauler.dev'

export default function sitemap(): MetadataRoute.Sitemap {
  return [
    { url: `${SITE}/` },
    ...source.getPages().map((page) => ({ url: `${SITE}${page.url}/` })),
  ]
}
