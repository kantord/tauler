import type { BaseLayoutProps } from 'fumadocs-ui/layouts/shared'

export const GITHUB_URL = 'https://github.com/kantord/tauler'

export function baseOptions(): BaseLayoutProps {
  return {
    nav: { title: 'tauler' },
    githubUrl: GITHUB_URL,
  }
}
