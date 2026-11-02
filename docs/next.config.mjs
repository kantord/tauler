import { createMDX } from 'fumadocs-mdx/next'

const withMDX = createMDX()

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
}

export default withMDX(config)
