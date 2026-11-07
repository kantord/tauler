import type { LucideIcon, LucideProps } from 'lucide-react'

/**
 * A Lucide icon drawn like the site's rules: a 1.5px stroke whatever its
 * size, with square ends and mitred corners. It is as tall as the text
 * around it; inside a Button the button sizes it. Lucide marks it
 * aria-hidden unless it is given a label.
 */
export function Icon({
  icon: Glyph,
  ...props
}: LucideProps & { icon: LucideIcon }) {
  return (
    <Glyph
      size="1em"
      strokeWidth={1.5}
      nonScalingStroke
      strokeLinecap="square"
      strokeLinejoin="miter"
      {...props}
    />
  )
}
