import { useState } from 'react'
import { keepPreviousData, useQuery } from '@tanstack/react-query'
import { api } from '../../api'
import type { GamesQuery } from '../../types'
import { useDebounced } from '../../lib/useDebounced'
import { BigCoverGrid } from '../../components/bigscreen/BigCoverGrid'
import { BigTextField } from '../../components/bigscreen/BigKeyboard'
import { BigLoadError } from '../../components/bigscreen/BigLoadError'

/** The Library's few filters for a couch, each a query of the games API. */
const FILTERS: { key: string; label: string; query: GamesQuery }[] = [
  { key: 'all', label: 'Everything', query: { sort: 'last_played' } },
  { key: 'installed', label: 'Installed', query: { installed: true, sort: 'last_played' } },
  { key: 'new', label: 'Not started', query: { status: ['untouched'], sort: 'review' } },
  { key: 'short', label: 'Short first', query: { sort: 'hltb', order: 'asc' } },
  { key: 'controller', label: 'Controller', query: { controller: true, sort: 'last_played' } },
]

const PAGE = 60

/** Big screen's Library: the cover grid, a few filters and a search, with
 *  more covers on request. The table stays in the normal layout. */
export function BigLibraryPage() {
  const [filter, setFilter] = useState('all')
  const [q, setQ] = useState('')
  const [limit, setLimit] = useState(PAGE)
  const search = useDebounced(q.trim(), 250)
  const query = { ...FILTERS.find((f) => f.key === filter)!.query, q: search || undefined, limit }
  const games = useQuery({ queryKey: ['games', 'big', query], queryFn: () => api.games(query), placeholderData: keepPreviousData })
  const items = games.data?.items ?? []
  const total = games.data?.total ?? 0

  return (
    <div className="flex flex-col gap-[1.25rem] pt-[1.5rem]">
      <h1 className="sr-only">Library</h1>
      <div className="flex flex-wrap items-center gap-[0.5rem]">
        <div role="group" aria-label="Show" data-focus-group="filters" className="flex flex-wrap gap-[0.5rem]">
          {FILTERS.map((f) => (
            <button
              key={f.key}
              type="button"
              className="big-chip"
              aria-pressed={filter === f.key}
              data-focus-first={f.key === 'all' ? '' : undefined}
              onClick={() => {
                setFilter(f.key)
                setLimit(PAGE)
              }}
            >
              {f.label}
            </button>
          ))}
        </div>
        <BigTextField
          id="big-library-search"
          type="search"
          label="Search your library"
          value={q}
          onChange={(v) => {
            setQ(v)
            setLimit(PAGE)
          }}
          placeholder="Search"
          className="ml-auto w-[16rem]"
        />
      </div>
      {games.isError ? (
        <BigLoadError what="your games" error={games.error} retrying={games.isFetching} onRetry={() => void games.refetch()} />
      ) : (
        <>
          <p className="m-0 text-[0.95rem] text-muted" aria-live="polite">
            {games.isPending ? 'Loading…' : `${total.toLocaleString()} ${total === 1 ? 'game' : 'games'}`}
          </p>
          <BigCoverGrid
            games={items}
            label="Games"
            after={
              items.length < total ? (
                <button type="button" className="big-btn-outline" onClick={() => setLimit((l) => l + PAGE)}>
                  Show more
                </button>
              ) : null
            }
          />
        </>
      )}
    </div>
  )
}
