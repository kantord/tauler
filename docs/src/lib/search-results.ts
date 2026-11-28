import type { SearchResultItem } from 'fumadocs-core/search/client'

type Hit = Extract<SearchResultItem, { hastContent: unknown }>
type Node = Hit['hastContent'] | Hit['hastContent']['children'][number]

/** A run of text, and whether it matched the query. */
export type Segment = { text: string; match: boolean }

/** One row of the search dialog: a page or a section, where it is, and one
 * line of its text. */
export type Result = Hit & {
  /** Set on a page's own row: the label over its group of results. */
  group?: string
  title: Segment[]
  path: string
  snippet?: Segment[]
}

function textOf(node: Node): string {
  if (node.type === 'text') return node.value
  if (!('children' in node)) return ''
  return node.children
    .map((child, i) =>
      // Table cells carry no space of their own.
      i > 0 && child.type === 'element' && /^t[dh]$/.test(child.tagName)
        ? ` ${textOf(child)}`
        : textOf(child),
    )
    .join('')
}

/** `text` split where any word of `query` occurs, ignoring case. */
function segments(text: string, query: string): Segment[] {
  const words = query
    .split(/\s+/)
    .filter(Boolean)
    .map((w) => w.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'))
  if (words.length === 0) return [{ text, match: false }]
  // A capturing split puts the matches at the odd positions.
  return text
    .split(new RegExp(`(${words.join('|')})`, 'i' + 'g'))
    .map((part, i) => ({ text: part, match: i % 2 === 1 }))
    .filter((s) => s.text)
}

// How much text a snippet keeps before its first match.
const LEAD_IN = 40

/** A block of text from just before its first match, so the clamped
 * snippet shows it. */
function snippet(text: string, query: string): Segment[] {
  const all = segments(text, query)
  const first = all.findIndex((s) => s.match)
  if (first < 1) return all
  const before = all
    .slice(0, first)
    .map((s) => s.text)
    .join('')
  if (before.length <= LEAD_IN) return all
  const cut = `…${before.slice(-LEAD_IN).replace(/^\S*\s/, '')}`
  return [{ text: cut, match: false }, ...all.slice(first)]
}

/**
 * Fumadocs lists every matching page, heading and block of text on its own
 * row, with the block rendered in full. The design has one row per page or
 * section: its title, its path and the first matching line.
 */
export function summarise(items: SearchResultItem[], query: string): Result[] {
  const results: Result[] = []
  let page = { title: '', crumbs: [] as string[] }
  for (const hit of items.flatMap((i) => (i.type === 'table' ? i.rows : [i]))) {
    const text = textOf(hit.hastContent)
    const last = results.at(-1)
    if (hit.type === 'page') {
      // The root of the tree is the site itself, which every page is under.
      const crumbs = (hit.breadcrumbs ?? [])
        .map(String)
        .filter((c) => c !== 'tauler')
      page = { title: text, crumbs }
      results.push({
        ...hit,
        group: page.title,
        title: segments(text, query),
        path: crumbs.join(' / '),
      })
    } else if (hit.type === 'heading') {
      results.push({
        ...hit,
        title: segments(text, query),
        path: [...page.crumbs, page.title].join(' / '),
      })
    } else if (last && !last.snippet) {
      last.snippet = snippet(text, query)
    }
  }
  return results
}
