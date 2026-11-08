import { Button as ButtonPrimitive } from '@base-ui/react/button'
import { cva, type VariantProps } from 'class-variance-authority'
import { cn } from 'cn'

// shadcn's base-lyra Button (`shadcn add button`), restyled to the site's
// controls: a 1.5px rule, square corners, 12px uppercase mono with wide
// tracking. Colours come from the shadcn slots in global.css, which point at
// the design-system tokens in global.css. A link that looks like a button takes `buttonVariants()`
// as its className: Base UI's Button forces button semantics on whatever it
// renders, so it must not render an <a>.
const variants = cva(
  "group/button inline-flex shrink-0 cursor-pointer items-center justify-center gap-2 border-[1.5px] border-transparent font-mono text-[12px] font-semibold tracking-[.08em] whitespace-nowrap uppercase no-underline outline-none select-none focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring focus-visible:outline-solid disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4",
  {
    variants: {
      variant: {
        default: 'border-input bg-primary text-primary-foreground',
        outline:
          'border-input bg-transparent text-foreground hover:text-orange-ink',
        'outline-primary':
          'border-primary bg-transparent text-orange-ink hover:bg-primary hover:text-primary-foreground',
        // Navigation: the current page (aria-current="page") is lit.
        nav: 'border-input bg-card text-foreground not-aria-[current=page]:hover:text-orange-ink aria-[current=page]:border-primary aria-[current=page]:bg-primary aria-[current=page]:text-primary-foreground',
      },
      size: {
        default: 'min-h-10 px-3.5',
        // A call to action: larger, in sentence case.
        lg: 'min-h-[52px] px-6 text-base tracking-normal normal-case',
        // A bare word in a line of text; lit, it becomes a filled tag.
        inline:
          'border-0 bg-transparent p-0 font-normal aria-[current=page]:px-2.5 aria-[current=page]:py-1 aria-[current=page]:font-semibold',
        // A square key, like a workspace number in a bar.
        icon: 'size-10 font-mono text-[16px] font-bold tracking-normal',
      },
    },
    defaultVariants: {
      variant: 'default',
      size: 'default',
    },
  },
)

/** The classes for a variant and size, merged: `size` and `className` win
 * over the base classes they conflict with (`inline` drops the border). */
function buttonVariants(props?: Parameters<typeof variants>[0]) {
  return cn(variants(props))
}

function Button({
  className,
  variant = 'default',
  size = 'default',
  ...props
}: Omit<ButtonPrimitive.Props, 'className'> &
  VariantProps<typeof variants> & { className?: string }) {
  return (
    <ButtonPrimitive
      data-slot="button"
      className={buttonVariants({ variant, size, className })}
      {...props}
    />
  )
}

export { Button, buttonVariants }
