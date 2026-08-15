// @ts-check
import starlight from '@astrojs/starlight'
import { defineConfig } from 'astro/config'
import mermaid from 'astro-mermaid'
import starlightThemeFlexoki from 'starlight-theme-flexoki'

// https://astro.build/config
export default defineConfig({
  // Project page on GitHub Pages. Swap `site` and drop `base` if a custom
  // domain is added later; images are referenced relatively so they follow.
  site: 'https://kantord.github.io',
  base: '/tauler',
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
        { slug: 'index' },
        { slug: 'layout-file' },
        { slug: 'elements' },
        { slug: 'data' },
        { slug: 'layout' },
        { slug: 'components' },
      ],
    }),
  ],
})
