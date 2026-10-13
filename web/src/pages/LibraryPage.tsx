import { useCallback, useMemo, useState } from 'react'
import { keepPreviousData, useInfiniteQuery, useQuery } from '@tanstack/react-query'
import { api } from '../api'
import type { GameView, GamesQuery } from '../types'
import { GameSheet } from '../components/GameSheet'
import { LibrarySearch } from '../components/LibrarySearch'
import { CoverGrid, CoverGridSkeleton } from '../components/library/CoverGrid'
import { LibraryTable } from '../components/library/LibraryTable'
import { LibraryFilters } from '../components/library/LibraryFilters'
import { defaultOrder, type LibrarySort } from '../components/library/filters'
import { filterState, gamesQuery, useLibraryParams } from '../components/library/useLibraryParams'
import { cx } from '../lib/format'
import { useReviewVocabulary } from '../lib/reviewTags'
import { useConfig, useSettings } from '../lib/queries'

const PAGE = 96

function CoversIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true">
      <rect x="3" y="3" width="7" height="9" rx="1.5" />
      <rect x="14" y="3" width="7" height="9" rx="1.5" />
      <rect x="3" y="15" width="7" height="6" rx="1.5" />
      <rect x="14" y="15" width="7" height="6" rx="1.5" />
    </svg>
  )
}

function TableIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
      <path d="M4 6h16M4 12h16M4 18h16" />
    </svg>
  )
}

export function LibraryPage() {
  const { data: config } = useConfig()
  const linuxActive = config?.linux_active === true
  const { p, q, update, setView, setSort, clearFilters } = useLibraryParams(linuxActive)
  const { view, sort, order } = p
  const [detail, setDetail] = useState<GameView | null>(null)

  const onHeaderSort = (s: LibrarySort) => setSort(s, s === sort ? (order === 'asc' ? 'desc' : 'asc') : defaultOrder(s))

  // ---- data ------------------------------------------------------------
  const { data: stats } = useQuery({ queryKey: ['stats'], queryFn: api.stats, staleTime: 60_000 })
  const { data: settings } = useSettings()
  const backlogOn = settings?.verdicts_receive === true
  const { data: vocab } = useReviewVocabulary()

  // React Query keys by value, so a fresh object each render is the same query.
  const baseQuery = gamesQuery(p, q, linuxActive)
  // The API names the store sort "source".
  const apiSort = (sort === 'store' ? 'source' : sort) as GamesQuery['sort']
  // "Show more" fetches the next page and appends it.
  const list = useInfiniteQuery({
    queryKey: ['games', 'library', baseQuery, sort, order],
    queryFn: ({ pageParam }) => api.games({ ...baseQuery, sort: apiSort, order, limit: PAGE, offset: pageParam }),
    initialPageParam: 0,
    getNextPageParam: (last, pages) => {
      const loaded = pages.reduce((n, p) => n + p.items.length, 0)
      return loaded < last.total ? loaded : undefined
    },
    placeholderData: keepPreviousData,
    // Wait for the config so a Linux filter doesn't run once without it.
    enabled: config !== undefined,
  })
  const { isLoading, isFetching, error } = list
  const items = useMemo(() => list.data?.pages.flatMap((p) => p.items) ?? [], [list.data])
  const total = list.data?.pages[0]?.total ?? 0
  const hasMore = list.hasNextPage

  const onUpdated = useCallback((g: GameView) => {
    setDetail((d) => (d && d.id === g.id ? g : d))
  }, [setDetail])

  const { anyFilter } = filterState(p, q, linuxActive)

  return (
    <div className="px-4 pb-16 sm:px-10">
      <section className="flex flex-wrap items-center justify-between gap-x-6 gap-y-4 pt-8">
        {/* The top bar's search box shows from 1024 px; below that it's here. */}
        <div className="basis-full lg:hidden">
          <LibrarySearch place="library" className="flex h-11 w-full sm:max-w-[400px]" />
        </div>
        <LibraryFilters
          p={p}
          q={q}
          stats={stats}
          vocab={vocab}
          linuxActive={linuxActive}
          total={list.data ? total : null}
          update={update}
          setSort={setSort}
          clearFilters={clearFilters}
          backlogOn={backlogOn}
        />

        <div role="group" aria-label="View" data-tour="views" className="seg shrink-0">
          <button
            type="button"
            aria-pressed={view === 'covers'}
            onClick={() => setView('covers')}
            className={cx('seg-item', view === 'covers' && 'seg-item-on')}
          >
            <CoversIcon />
            Covers
          </button>
          <button
            type="button"
            aria-pressed={view === 'table'}
            onClick={() => setView('table')}
            className={cx('seg-item', view === 'table' && 'seg-item-on')}
          >
            <TableIcon />
            Table
          </button>
        </div>
      </section>

      {error && (
        <p className="mt-8 text-bad" role="alert">
          Couldn't load your library: {(error as Error).message}
        </p>
      )}

      <section data-tour="games" className={view === 'covers' ? 'pt-8' : 'pt-5'} aria-label={view === 'covers' ? 'Games' : undefined}>
        {view === 'covers' ? (
          isLoading ? (
            <CoverGridSkeleton />
          ) : (
            <CoverGrid items={items} linuxActive={linuxActive} onOpen={setDetail} dimmed={isFetching} />
          )
        ) : (
          <LibraryTable
            items={items}
            sort={sort}
            order={order}
            onSort={onHeaderSort}
            onOpen={setDetail}
            linuxActive={linuxActive}
            backlogOn={backlogOn}
            loading={isLoading}
            dimmed={isFetching && !isLoading}
          />
        )}

        {!isLoading && !error && items.length === 0 && (
          <p className="py-16 text-center text-muted">
            Nothing here.{' '}
            {(anyFilter || q) && (
              <button type="button" onClick={clearFilters} className="sentence-link">
                Clear the filters
              </button>
            )}
          </p>
        )}

        {hasMore && (
          <div className="mt-10 flex items-center justify-center gap-4">
            <button
              type="button"
              className="btn-outline h-11"
              onClick={() => void list.fetchNextPage()}
              disabled={list.isFetchingNextPage}
            >
              {list.isFetchingNextPage ? 'Loading…' : 'Show more'}
            </button>
            <span className="mono">
              {items.length.toLocaleString()} of {total.toLocaleString()}
            </span>
          </div>
        )}
      </section>

      <GameSheet game={detail} onClose={() => setDetail(null)} onUpdated={onUpdated} />
    </div>
  )
}
