import { siGithub } from 'simple-icons'

// Brand marks come from simple-icons because Lucide has none.
export function GitHubIcon() {
  return (
    <svg
      aria-hidden="true"
      viewBox="0 0 24 24"
      width="1em"
      height="1em"
      fill="currentColor"
    >
      <path d={siGithub.path} />
    </svg>
  )
}
