'use client'
import { Dialog } from '@base-ui/react/dialog'
import { Menu, X } from 'lucide-react'
import Link from 'next/link'
import { usePathname } from 'next/navigation'
import { useState } from 'react'
import { Button, buttonVariants } from '@/components/ui/button'
import { Icon } from '@/components/ui/icon'
import { ThemeToggle } from './controls'
import { DocsNav, type NavGroup } from './DocsNav'
import { GitHubIcon } from './icons'

/**
 * Below 768px the contents live here: a menu button at the right end of the
 * header opens a drawer from the right. The scrim covers the page, header
 * included; the drawer starts at the right edge so the logo stays visible.
 */
export function DocsMenu({ groups }: { groups: NavGroup[] }) {
  const [open, setOpen] = useState(false)
  const pathname = usePathname()
  const close = () => setOpen(false)
  return (
    <Dialog.Root open={open} onOpenChange={setOpen}>
      <Dialog.Trigger
        render={
          <Button
            variant="outline"
            size="icon"
            aria-label="Open menu"
            className="md:hidden"
          />
        }
      >
        <Icon icon={Menu} />
      </Dialog.Trigger>
      <Dialog.Portal>
        <Dialog.Backdrop className="fixed inset-0 z-50 bg-ink/40 md:hidden" />
        <Dialog.Popup className="fixed inset-y-0 right-0 z-50 flex w-[min(300px,80vw)] flex-col overflow-y-auto border-l border-rule bg-page outline-none md:hidden">
          <Dialog.Title className="sr-only">Menu</Dialog.Title>
          <div className="flex items-center gap-3 border-b-thin border-line p-3">
            <a
              href="https://github.com/kantord/tauler"
              className={buttonVariants({ variant: 'outline' })}
            >
              <GitHubIcon />
              Star on GitHub
            </a>
            <ThemeToggle />
            <Dialog.Close
              render={
                <Button
                  variant="outline"
                  size="icon"
                  aria-label="Close menu"
                  className="ml-auto"
                />
              }
            >
              <Icon icon={X} />
            </Dialog.Close>
          </div>
          <nav
            aria-label="Site links"
            className="flex gap-3 border-b-thin border-line p-3"
          >
            <Link href="/" className={buttonVariants({ variant: 'nav' })}>
              home
            </Link>
            <Link
              href="/docs/"
              aria-current={pathname.startsWith('/docs') ? 'page' : undefined}
              className={buttonVariants({ variant: 'nav' })}
              onClick={close}
            >
              docs
            </Link>
          </nav>
          <DocsNav groups={groups} url={pathname} onNavigate={close} />
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  )
}
