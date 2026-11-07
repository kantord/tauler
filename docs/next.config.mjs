import { fileURLToPath } from 'node:url'
import { createMDX } from 'fumadocs-mdx/next'

const withMDX = createMDX()

// Draft pages (`page.draft.tsx`) are routes only under `next dev`: the build
// never sees them as pages, so the export has no trace of them. The same
// test as `DRAFTS` in src/lib/drafts.ts, which hides the links to them.
const drafts = process.env.NODE_ENV === 'development'

// The home room's GLSL ships without its comments (src/components/site/room/room-shader.ts).
const stripGlsl = fileURLToPath(
  new URL('./scripts/strip-glsl-comments.cjs', import.meta.url),
)

/** @type {import('next').NextConfig} */
const config = {
  // A static site on GitHub Pages: no server, every route prerendered.
  output: 'export',
  // `dist/`, not Next's default `out/`: tauler-web-e2e and the Playwright
  // suite serve this directory, and the name predates the move to Next.
  distDir: 'dist',
  // Published links all end in a slash (`/docs/layout-file/`); keep them
  // resolving to `…/index.html` on a static host.
  trailingSlash: true,
  reactStrictMode: true,
  images: { unoptimized: true },
  turbopack: {
    rules: {
      '*.glsl': { loaders: [stripGlsl], type: 'raw' },
      '**/site/room/vendor/PathTracingCommon.js': {
        loaders: [stripGlsl],
      },
    },
  },
  pageExtensions: [
    ...(drafts ? ['draft.tsx'] : []),
    'mdx',
    'md',
    'jsx',
    'js',
    'tsx',
    'ts',
  ],
}

export default withMDX(config)
