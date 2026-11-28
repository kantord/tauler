import Link from 'next/link'
import type * as PageTree from 'fumadocs-core/page-tree'

export type NavGroup = {
  label?: string
  /** Links that never show as current (the drawer's site links). */
  plain?: boolean
  items: { url: string; name: string }[]
}

/** The sidebar tree as the design groups it: separators become labels. */
export function groupsOf(tree: PageTree.Root): NavGroup[] {
  const groups: NavGroup[] = [{ items: [] }]
  for (const node of tree.children) {
    if (node.type === 'separator')
      groups.push({ label: String(node.name ?? ''), items: [] })
    else if (node.type === 'page')
      groups.at(-1)!.items.push({ url: node.url, name: String(node.name) })
  }
  return groups.filter((g) => g.items.length > 0)
}

const bare = (url: string) => url.replace(/\/$/, '')

/** The docs contents: group labels and page links. Shared by the docked
 * sidebar tile and the phone drawer. A title too long for the row ends in an
 * ellipsis. */
export function DocsNav({
  groups,
  url,
  onNavigate,
}: {
  groups: NavGroup[]
  url: string
  onNavigate?: () => void
}) {
  return (
    <nav aria-label="Docs" className="scroll px-2 pb-4">
      {groups.map((g, i) => (
        <div key={g.label ?? i}>
          {g.label && <div className="grp">{g.label}</div>}
          {g.items.map((item) => (
            <Link
              key={item.url}
              href={item.url}
              className="nav"
              title={item.name}
              aria-current={
                !g.plain && bare(item.url) === bare(url) ? 'page' : undefined
              }
              onClick={onNavigate}
            >
              <span className="truncate">{item.name}</span>
            </Link>
          ))}
        </div>
      ))}
    </nav>
  )
}
