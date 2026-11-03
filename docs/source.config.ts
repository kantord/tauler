import { defineConfig, defineDocs } from 'fumadocs-mdx/config'
import { metaSchema, pageSchema } from 'fumadocs-core/source/schema'
import { remarkDirectiveAdmonition } from 'fumadocs-core/mdx-plugins'
import remarkDirective from 'remark-directive'
import { remarkLiteralDirectives } from './src/lib/remark-literal-directives'
import { taulerCodeTheme } from './src/lib/shiki-theme'

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
    remarkPlugins: [
      remarkDirective,
      // Only container directives are meant; "3:1" in prose must stay text.
      remarkLiteralDirectives,
      [
        remarkDirectiveAdmonition,
        {
          // Starlight's names, which the pages already use.
          types: {
            note: 'info',
            tip: 'success',
            caution: 'warning',
            danger: 'error',
          },
        },
      ],
    ],
    // One dark theme: the design's code panels are dark in light mode too.
    rehypeCodeOptions: {
      themes: { light: taulerCodeTheme, dark: taulerCodeTheme },
      // Lets a code panel label itself ("terminal", "jsx") from its language.
      addLanguageClass: true,
    },
  },
})
