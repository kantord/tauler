import type { CSSProperties } from 'react'
import { LOGO_LETTERS } from './logo-letters'

/**
 * The tauler logo, inline so it follows the theme: the ground is `--tu-ink`,
 * the letters, rules and light dither squares are `--tu-page`, and the
 * focused cell is `--tu-or`. In dark mode the whole mark inverts.
 *
 * `dither` is the side of one dither square in viewBox units. It should come
 * out at about one screen pixel (100 ÷ the rendered size): 3.6 at 36px, 4.5
 * at 24px, 2 at 160px and up. `id` must be unique on the page, because the
 * dither is an SVG pattern.
 */
export function Logo({
  id,
  dither,
  label,
  className,
  style,
}: {
  id: string
  dither: number
  /** Present: the logo is an image with this name. Absent: decoration. */
  label?: string
  className?: string
  style?: CSSProperties
}) {
  const cell = dither * 2
  const a11y = label
    ? { role: 'img', 'aria-label': label }
    : { 'aria-hidden': true, focusable: false }
  return (
    <svg
      viewBox="2.5 2.5 95 95"
      className={className}
      style={{ display: 'block', flex: 'none', ...style }}
      {...a11y}
    >
      <defs>
        <pattern
          id={id}
          width={cell}
          height={cell}
          patternUnits="userSpaceOnUse"
        >
          <rect width={cell} height={cell} fill="var(--tu-ink)" />
          <rect width={dither} height={dither} fill="var(--tu-page)" />
          <rect
            x={dither}
            y={dither}
            width={dither}
            height={dither}
            fill="var(--tu-page)"
          />
        </pattern>
      </defs>
      <rect width="100" height="100" fill="var(--tu-ink)" />
      <rect y="80" width="66.667" height="20" fill={`url(#${id})`} />
      <rect x="66.667" y="80" width="33.333" height="20" fill="var(--tu-or)" />
      <path fill="var(--tu-page)" d={LOGO_LETTERS} />
      <g fill="var(--tu-page)">
        <rect x="32.083" width="2.5" height="100" />
        <rect x="65.417" width="2.5" height="100" />
        <rect y="38.75" width="100" height="2.5" />
        <rect y="78.75" width="100" height="2.5" />
      </g>
    </svg>
  )
}
