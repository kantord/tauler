import { defineConfig, defineDocs } from 'fumadocs-mdx/config'
import { metaSchema, pageSchema } from 'fumadocs-core/source/schema'
import { remarkDirectiveAdmonition } from 'fumadocs-core/mdx-plugins'
import remarkDirective from 'remark-directive'

// The published docs: everything a person writing a layout file needs.
export const docs = defineDocs({
  dir: 'content/docs',
  docs: {
    schema: pageSchema,
    postprocess: { includeProcessedMarkdown: true },
  },
  meta: { schema: metaSchema },
})

// Maintainer-only pages (screenshot galleries). Its own collection so it can
// never leak into the published sidebar, search index or llms.txt.
export const devtools = defineDocs({
  dir: 'content/devtools',
  docs: { schema: pageSchema },
  meta: { schema: metaSchema },
})

export default defineConfig({
  mdxOptions: {
    // `:::caution` / `:::danger[Title]` asides, carried over from Starlight.
    remarkPlugins: [remarkDirective, remarkDirectiveAdmonition],
  },
})
