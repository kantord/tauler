import * as React from 'react'
import { cva, type VariantProps } from 'class-variance-authority'
import { cn } from '@/lib/utils'

// shadcn's base-lyra Alert (`shadcn add alert`), restyled as the design's
// callout: a sub-window with an orange frame, its title in the sub-window
// title bar (`.sbar` in global.css, shared with the code panels) lit orange.
const alertVariants = cva(
  'group/alert relative flex w-full min-w-0 flex-col border-thin',
  {
    variants: {
      variant: {
        default: 'border-primary bg-card text-card-foreground',
      },
    },
    defaultVariants: {
      variant: 'default',
    },
  },
)

function Alert({
  className,
  variant,
  ...props
}: React.ComponentProps<'div'> & VariantProps<typeof alertVariants>) {
  return (
    <div
      data-slot="alert"
      role="alert"
      className={cn(alertVariants({ variant }), className)}
      {...props}
    />
  )
}

function AlertTitle({ className, ...props }: React.ComponentProps<'div'>) {
  return (
    <div
      data-slot="alert-title"
      className={cn(
        'sbar border-b-primary font-semibold text-orange-ink [background:repeating-conic-gradient(var(--primary)_0_25%,transparent_0_50%)_left_bottom/4px_4px_repeat-x,var(--card)]',
        className,
      )}
      {...props}
    />
  )
}

function AlertDescription({
  className,
  ...props
}: React.ComponentProps<'div'>) {
  return (
    <div
      data-slot="alert-description"
      className={cn(
        'px-4 py-4 text-sm leading-[1.6] [&_p]:text-sm [&_p]:leading-[1.6] [&>:last-child]:mb-0',
        className,
      )}
      {...props}
    />
  )
}

export { Alert, AlertTitle, AlertDescription }
