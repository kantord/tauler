// @ts-check
import starlight from '@astrojs/starlight'
import { defineConfig } from 'astro/config'
import mermaid from 'astro-mermaid'
import starlightThemeFlexoki from 'starlight-theme-flexoki'

// https://astro.build/config
export default defineConfig({
  // Served from a custom domain at the root, so no `base` prefix.
  site: 'https://tauler.dev',
  integrations: [
    // Must come before starlight so it can transform ```mermaid code blocks
    // before Starlight's syntax highlighting. Renders client-side.
    mermaid({ theme: 'default', autoTheme: true }),
    starlight({
      plugins: [starlightThemeFlexoki()],
      title: 'tauler',
      social: [
        {
          icon: 'github',
          label: 'GitHub',
          href: 'https://github.com/kantord/tauler',
        },
      ],
      sidebar: [
        { slug: 'docs' },
        { slug: 'docs/layout-file' },
        { slug: 'docs/elements' },
        { slug: 'docs/data' },
        { slug: 'docs/layout' },
        { slug: 'docs/components' },
        { slug: 'docs/macos' },
      ],
    }),
  ],
})
