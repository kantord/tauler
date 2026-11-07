import { docs, devtools as devtoolsCollection } from 'collections/server'
import { llms, loader } from 'fumadocs-core/source'

export const source = loader({
  baseUrl: '/docs',
  source: docs.toFumadocsSource(),
})

// Maintainer-only, unlisted: never in the docs sidebar, search or llms.txt.
export const devtools = loader({
  baseUrl: '/devtools',
  source: devtoolsCollection.toFumadocsSource(),
})

export const docsLlms = llms(source, {
  renderPage: async (page) =>
    `# ${page.data.title} (${page.url})\n\n${await page.data.getText('processed')}`,
})

export function getPageMarkdownUrl(page: { slugs: string[] }) {
  const segments = [...page.slugs, 'content.md']
  return { segments, url: `/llms.mdx/docs/${segments.join('/')}` }
}
