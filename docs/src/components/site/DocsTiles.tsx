import { ArrowLeft, ArrowRight, ChevronRight } from 'lucide-react'
import Link from 'next/link'
import { Icon } from '@/components/ui/icon'
import type { ReactNode } from 'react'
import type * as PageTree from 'fumadocs-core/page-tree'
import { DocsNav, groupsOf, type NavGroup } from './DocsNav'
import { DockedToc, HeadingBar } from './Toc'
import { findNeighbour } from 'fumadocs-core/page-tree'
import type { TOCItemType } from 'fumadocs-core/toc'

/** The label of the group holding the page, for the breadcrumb. */
function groupLabel(groups: NavGroup[], url: string): string | undefined {
  return groups.find((g) => g.items.some((i) => i.url === url))?.label
}

/**
 * A docs page laid out as three tiles — contents, the article, on this page.
 * From 1024px all three sit in a row; from 768px the contents and "on this
 * page" are two sticky tiles in one left column; below 768px the contents are
 * the header's drawer (DocsMenu) and "on this page" is the heading bar under
 * the page's lead. Below 640px the article is frameless.
 */
export function DocsTiles({
  tree,
  url,
  title,
  description,
  toc,
  children,
}: {
  tree: PageTree.Root
  url: string
  title: string
  description?: string
  toc: TOCItemType[]
  children: ReactNode
}) {
  const groups = groupsOf(tree)
  const group = groupLabel(groups, url)
  const { previous, next } = findNeighbour(tree, url)
  const headings = toc.filter((t) => t.depth <= 3)

  return (
    <div className="mx-auto w-full max-w-page pb-16 sm:px-6 sm:pt-3">
      <div className="flex items-start gap-3">
        <div className="sticky top-[calc(var(--tu-header-h)+12px)] flex h-[calc(100vh-var(--tu-header-h)-24px)] w-[clamp(204px,24vw,248px)] shrink-0 flex-col gap-3 max-md:hidden">
          <aside aria-label="Contents" className="tile side min-h-0 flex-auto">
            <div className="bar">
              <span className="tt">contents</span>
            </div>
            <DocsNav groups={groups} url={url} />
          </aside>
          {headings.length > 0 && (
            <aside
              aria-label="On this page"
              className="tile side max-h-1/2 shrink-0 lg:hidden"
            >
              <div className="bar">
                <span className="tt">on this page</span>
              </div>
              <DockedToc headings={headings} />
            </aside>
          )}
        </div>

        <main className="tile on min-w-0 flex-1 max-sm:border-0 max-sm:bg-transparent max-sm:outline-0">
          <div className="bar max-sm:hidden">
            <span className="tt">{title}</span>
          </div>
          <nav aria-label="Breadcrumb" className="crumbs max-md:hidden">
            <Link href="/docs/" className="crumb">
              docs
            </Link>
            {group && (
              <>
                <Icon icon={ChevronRight} className="sep" />
                <span className="crumb">{group}</span>
              </>
            )}
            <Icon icon={ChevronRight} className="sep" />
            <span className="crumb cur" aria-current="page">
              {title}
            </span>
          </nav>
          <article className="art">
            <h1 className="h">{title}</h1>
            {description && <p className="lead">{description}</p>}
            {headings.length > 0 && <HeadingBar headings={headings} />}
            {children}
            <div className="mt-12 grid grid-cols-[repeat(auto-fit,minmax(min(100%,220px),1fr))] gap-3">
              {previous ? (
                <Link href={previous.url} className="pn">
                  <small>
                    <Icon icon={ArrowLeft} />
                    previous
                  </small>
                  {previous.name}
                </Link>
              ) : (
                <span />
              )}
              {next && (
                <Link href={next.url} className="pn items-end text-right">
                  <small>
                    next
                    <Icon icon={ArrowRight} />
                  </small>
                  {next.name}
                </Link>
              )}
            </div>
          </article>
        </main>

        {headings.length > 0 && (
          <aside
            aria-label="On this page"
            className="tile side sticky top-[calc(var(--tu-header-h)+12px)] max-h-[calc(100vh-var(--tu-header-h)-24px)] w-[clamp(200px,20vw,300px)] shrink-0 max-lg:hidden"
          >
            <div className="bar">
              <span className="tt">on this page</span>
            </div>
            <DockedToc headings={headings} />
          </aside>
        )}
      </div>
    </div>
  )
}
