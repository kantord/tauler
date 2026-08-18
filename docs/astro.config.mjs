// @ts-check
import starlight from '@astrojs/starlight'
import { defineConfig } from 'astro/config'
import mermaid from 'astro-mermaid'

// https://astro.build/config
export default defineConfig({
  // Served from a custom domain at the root, so no `base` prefix.
  site: 'https://tauler.dev',
  integrations: [
    // Must come before starlight so it can transform ```mermaid code blocks
    // before Starlight's syntax highlighting. Renders client-side.
    mermaid({ theme: 'default', autoTheme: true }),
    starlight({
      title: 'tauler',
      head: [
        // Preload the self-hosted fonts so docs pages don't flash the
        // fallback mono before the real families arrive.
        ...[
          '/fonts/martian-mono-latin.woff2',
          '/fonts/dm-mono-400-latin.woff2',
          '/fonts/dm-mono-500-latin.woff2',
        ].map((href) => ({
          tag: 'link',
          attrs: { rel: 'preload', href, as: 'font', type: 'font/woff2', crossorigin: 'anonymous' },
        })),
      ],
      customCss: [
        './src/styles/fonts.css',
        './src/styles/tokens.css',
        './src/styles/docs-theme.css',
      ],
      components: {
        // The design system has no light theme; lock the docs to dark.
        ThemeProvider: './src/components/ThemeProvider.astro',
        ThemeSelect: './src/components/ThemeSelect.astro',
      },
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
