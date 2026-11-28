'use client'
import { Dialog } from '@base-ui/react/dialog'
import { Menu, X } from 'lucide-react'
import { usePathname } from 'next/navigation'
import { useState } from 'react'
import { Button, buttonVariants } from '@/components/ui/button'
import { Icon } from '@/components/ui/icon'
import { ThemeSegment } from './controls'
import { DocsNav, type NavGroup } from './DocsNav'
import { GitHubIcon } from './icons'

const SITE: NavGroup = {
  label: 'Site',
  plain: true,
  items: [
    { url: '/', name: 'Home' },
    { url: '/docs/', name: 'Docs' },
  ],
}

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
        <Dialog.Backdrop className="fixed inset-0 z-50 bg-scrim md:hidden" />
        <Dialog.Popup className="fixed inset-y-0 right-0 z-50 flex w-[min(320px,85%)] flex-col border-l border-rule bg-card outline-none md:hidden">
          <div className="bar relative">
            <Dialog.Title className="tt m-0 text-label font-semibold">
              contents
            </Dialog.Title>
            <Dialog.Close
              render={
                <Button
                  variant="outline"
                  size="icon"
                  aria-label="Close menu"
                  className="absolute right-1 size-7 bg-card"
                />
              }
            >
              <Icon icon={X} />
            </Dialog.Close>
          </div>
          <div className="flex items-center justify-between gap-3 border-b-thin border-line px-5 py-3">
            <a
              href="https://github.com/kantord/tauler"
              className={buttonVariants({ variant: 'outline' })}
            >
              <GitHubIcon />
              Star on GitHub
            </a>
            <ThemeSegment />
          </div>
          <DocsNav
            groups={[SITE, ...groups]}
            url={pathname}
            onNavigate={close}
          />
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  )
}
