'use client'
import {
  SearchDialog,
  SearchDialogClose,
  SearchDialogContent,
  SearchDialogHeader,
  SearchDialogIcon,
  SearchDialogInput,
  SearchDialogList,
  SearchDialogListItem,
  SearchDialogOverlay,
  type SharedProps,
} from 'fumadocs-ui/components/dialog/search'
import { useStaticSearch } from 'fumadocs-core/search/client'
import { X } from 'lucide-react'
import { useMemo } from 'react'
import { Icon } from '@/components/ui/icon'
import { summarise, type Result, type Segment } from '@/lib/search-results'

// Static search: the index is prerendered at /api/search and filtered in the
// browser, which is all a GitHub Pages host can serve. The classes give
// Fumadocs' dialog the site's shape: a heavy-framed panel over a flat scrim,
// 12px from the edges on a phone and 600px wide, 80px down, from 768px.
export default function DefaultSearchDialog(props: SharedProps) {
  const search = useStaticSearch()
  const results = useMemo(
    () => search.data && summarise(search.data.items, search.data.query),
    [search.data],
  )
  return (
    <SearchDialog {...search} {...props}>
      <SearchDialogOverlay className="bg-ink/45 backdrop-blur-none" />
      <SearchDialogContent className="top-3 w-[calc(100%-24px)] max-w-none border-rule bg-card md:top-20 md:w-[600px]">
        <SearchDialogHeader className="gap-3 py-2 pr-2 pl-4">
          <SearchDialogIcon className="size-4" />
          <SearchDialogInput className="min-h-9 text-base" />
          <SearchDialogClose className="size-9 border-input p-0 text-foreground">
            <Icon icon={X} />
          </SearchDialogClose>
        </SearchDialogHeader>
        <SearchDialogList
          items={results}
          Item={({ item, onClick }) => (
            <ResultRow result={item as Result} onClick={onClick} />
          )}
        />
      </SearchDialogContent>
    </SearchDialog>
  )
}

function ResultRow({
  result,
  onClick,
}: {
  result: Result
  onClick: () => void
}) {
  return (
    <>
      {result.group && (
        <div
          role="presentation"
          className="px-3 pt-4 pb-1 text-label text-ink-muted"
        >
          {result.group}
        </div>
      )}
      <SearchDialogListItem
        item={result}
        onClick={onClick}
        className="border-l border-transparent px-3 aria-selected:border-orange aria-selected:bg-transparent"
      >
        <div className="text-base font-bold">
          <Marked segments={result.title} />
        </div>
        {result.path && (
          <div className="mt-0.5 text-label text-ink-muted">{result.path}</div>
        )}
        {result.snippet && (
          <div className="mt-1 line-clamp-2 text-sm text-ink-muted">
            <Marked segments={result.snippet} />
          </div>
        )}
      </SearchDialogListItem>
    </>
  )
}

function Marked({ segments }: { segments: Segment[] }) {
  return segments.map((s, i) =>
    s.match ? (
      <mark key={i} className="bg-transparent font-bold text-orange-ink">
        {s.text}
      </mark>
    ) : (
      s.text
    ),
  )
}
