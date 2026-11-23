import { ArrowLeft, ArrowRight, ChevronRight } from 'lucide-react'
import Link from 'next/link'
import { Icon } from '@/components/ui/icon'
import type { ReactNode } from 'react'
import type * as PageTree from 'fumadocs-core/page-tree'
import { findNeighbour } from 'fumadocs-core/page-tree'
import type { TOCItemType } from 'fumadocs-core/toc'

type Group = { label?: string; items: PageTree.Item[] }

/** The sidebar tree as the design groups it: separators become labels. */
function groupsOf(tree: PageTree.Root): Group[] {
  const groups: Group[] = [{ items: [] }]
  for (const node of tree.children) {
    if (node.type === 'separator')
      groups.push({ label: String(node.name ?? ''), items: [] })
    else if (node.type === 'page') groups.at(-1)!.items.push(node)
  }
  return groups.filter((g) => g.items.length > 0)
}

/** The label of the group holding the page, for the breadcrumb. */
function groupLabel(groups: Group[], url: string): string | undefined {
  return groups.find((g) => g.items.some((i) => i.url === url))?.label
}

/**
 * A docs page laid out as three tiles — contents, the article, on this page —
 * under a breadcrumb. The design's flex-wrap is its responsive behaviour:
 * the side tiles drop below or above the article as the viewport narrows.
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
    <div className="mx-auto w-full max-w-page px-6 pt-7 pb-16">
      <div className="flex flex-wrap items-start gap-3">
        <aside
          aria-label="Contents"
          className="tile side max-w-[280px] flex-[1_1_230px] max-md:max-w-none"
        >
          <div className="bar">
            <span className="tt">contents</span>
          </div>
          <nav aria-label="Docs" className="scroll px-2 pb-4">
            {groups.map((g, i) => (
              <div key={g.label ?? i}>
                {g.label && <div className="grp">{g.label}</div>}
                {g.items.map((item) => (
                  <Link
                    key={item.url}
                    href={item.url}
                    className="nav"
                    aria-current={item.url === url ? 'page' : undefined}
                  >
                    {item.name}
                  </Link>
                ))}
              </div>
            ))}
          </nav>
        </aside>

        <main className="tile on flex-[999_1_560px]">
          <div className="bar">
            <span className="tt">{title}</span>
          </div>
          <nav aria-label="Breadcrumb" className="crumbs">
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
            className="tile side max-w-[240px] flex-[1_1_200px] max-md:max-w-none"
          >
            <div className="bar">
              <span className="tt">on this page</span>
            </div>
            <nav className="toc scroll py-2 pb-4" aria-label="On this page">
              {headings.map((h) => (
                <a key={h.url} href={h.url} className={`depth-${h.depth}`}>
                  {h.title}
                </a>
              ))}
            </nav>
          </aside>
        )}
      </div>
    </div>
  )
}
