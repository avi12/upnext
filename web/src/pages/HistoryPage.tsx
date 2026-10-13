import { useCallback, useEffect, useState } from 'react'
import { Link, useSearchParams } from 'react-router'
import { useInfiniteQuery, useMutation, useQueryClient, type InfiniteData } from '@tanstack/react-query'
import { api, ApiError } from '../api'
import type { GameView, HistoryOutcome, MissReason, OwnedPick, PickBatch, PickVerdict } from '../types'
import { GameSheet } from '../components/GameSheet'
import { SentenceChoice } from '../components/SentenceMenu'
import { HistoryBatch } from '../components/collections/HistoryBatch'
import { useToast } from '../components/useToast'
import { LAST_BATCH_KEY } from '../components/pick/useGameActions'

/** The History page's list; the Pick lineup lives at ['history', 'last']. */
const HISTORY_LIST_KEY = ['history', 'list'] as const

/** Lineups per page; "Show more" asks for the next. */
export const HISTORY_PAGE = 25

type Period = 'all' | 'week' | 'month' | 'quarter' | 'year'
type Order = 'newest' | 'oldest'
type Outcome = '' | HistoryOutcome

const DAYS: Record<Exclude<Period, 'all'>, number> = { week: 7, month: 30, quarter: 91, year: 365 }

const PERIODS: { value: Period; label: string }[] = [
  { value: 'all', label: 'from any time' },
  { value: 'week', label: 'from the last week' },
  { value: 'month', label: 'from the last month' },
  { value: 'quarter', label: 'from the last three months' },
  { value: 'year', label: 'from the last year' },
]

const ORDERS: { value: Order; label: string }[] = [
  { value: 'newest', label: 'newest first' },
  { value: 'oldest', label: 'oldest first' },
]

const OUTCOMES: { value: Outcome; label: string }[] = [
  { value: '', label: 'whatever came of them' },
  { value: 'played', label: 'that you played' },
  { value: 'saved', label: 'that you saved for later' },
  { value: 'turned_down', label: 'that you turned down' },
]

function oneOf<T extends string>(value: string | null, options: { value: T }[], fallback: T): T {
  return options.find((o) => o.value === value)?.value ?? fallback
}

/** The sentence's choices and the search, kept in the address. */
function useHistoryParams() {
  const [params, setParams] = useSearchParams()
  const period = oneOf(params.get('period'), PERIODS, 'all')
  const order = oneOf(params.get('order'), ORDERS, 'newest')
  const outcome = oneOf(params.get('outcome'), OUTCOMES, '')
  const q = params.get('q') ?? ''
  const set = (key: string, value: string, fallback: string) =>
    setParams(
      (cur) => {
        const next = new URLSearchParams(cur)
        if (value === fallback) next.delete(key)
        else next.set(key, value)
        return next
      },
      { replace: true },
    )
  return {
    period,
    order,
    outcome,
    q,
    filtered: period !== 'all' || outcome !== '' || q.trim() !== '',
    setPeriod: (v: Period) => set('period', v, 'all'),
    setOrder: (v: Order) => set('order', v, 'newest'),
    setOutcome: (v: Outcome) => set('outcome', v, ''),
    setQ: (v: string) => set('q', v, ''),
    clear: () => setParams({}, { replace: true }),
  }
}

/** The search box: the address follows it a moment after typing stops. */
function HistorySearch({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  // What's typed, and the address's value it was typed over: when the
  // address changes from elsewhere ("Show all picks"), the box shows that.
  const [draft, setDraft] = useState({ base: value, text: value })
  const text = draft.base === value ? draft.text : value
  useEffect(() => {
    if (text === value) return
    const t = window.setTimeout(() => onChange(text), 250)
    return () => window.clearTimeout(t)
  }, [text, value, onChange])
  return (
    <input
      type="search"
      value={text}
      onChange={(e) => setDraft({ base: value, text: e.target.value })}
      placeholder="Find a game or a request"
      aria-label="Find a game or a request"
      className="field h-11 w-full max-w-[360px]"
    />
  )
}

/** A page of lineups, with the period's cutoff it was asked for. */
type HistoryPageData = { batches: PickBatch[]; since?: number }

/** Unix seconds a period starts at, or none for any time. */
function cutoff(period: Period): number | undefined {
  return period === 'all' ? undefined : Math.floor(Date.now() / 1000) - DAYS[period] * 86_400
}

export function HistoryPage() {
  const qc = useQueryClient()
  const h = useHistoryParams()
  const { period, order, outcome, q } = h
  const { data, isLoading, error, refetch, isFetching, hasNextPage, fetchNextPage, isFetchingNextPage } = useInfiniteQuery({
    queryKey: [...HISTORY_LIST_KEY, { period, order, outcome, q: q.trim() }],
    // The period's cutoff is worked out for the first page and carried to
    // the next ones, so Show more neither skips nor repeats a lineup that
    // falls across it while the page is open.
    queryFn: async ({ pageParam }): Promise<HistoryPageData> => {
      const since = pageParam.offset === 0 ? cutoff(period) : pageParam.since
      const batches = await api.history(HISTORY_PAGE, { offset: pageParam.offset, order, q, outcome: outcome || undefined, since })
      return { batches, since }
    },
    initialPageParam: { offset: 0 } as { offset: number; since?: number },
    getNextPageParam: (last, pages) =>
      last.batches.length < HISTORY_PAGE ? undefined : { offset: pages.length * HISTORY_PAGE, since: last.since },
  })
  const batches = data?.pages.flatMap((p) => p.batches)
  // Batches the user has toggled away from their default (first open, rest closed).
  const [flipped, setFlipped] = useState<Set<string>>(() => new Set())
  const [sheet, setSheet] = useState<GameView | null>(null)
  const toast = useToast()

  // Rating a pick here puts the answer in place rather than
  // reloading the list, so the page keeps its place and its open lineups.
  const rate = useMutation({
    mutationFn: ({ batchId, pick, verdict, reason }: { batchId: string; pick: OwnedPick; verdict: PickVerdict | null; reason?: MissReason | null }) =>
      api.ratePick(batchId, { title_id: pick.id, verdict, reason }),
    onSuccess: (b, { pick }) => {
      qc.setQueriesData<InfiniteData<HistoryPageData>>({ queryKey: HISTORY_LIST_KEY }, (d) =>
        d && {
          ...d,
          pages: d.pages.map((page) => ({ ...page, batches: page.batches.map((x) => (x.batch_id === b.batch_id ? b : x)) })),
        },
      )
      qc.invalidateQueries({ queryKey: LAST_BATCH_KEY })
      qc.invalidateQueries({ queryKey: ['game', pick.id] })
      qc.invalidateQueries({ queryKey: ['taste-sources'] })
    },
    onError: (e) => toast.error('Couldn’t save that rating', e instanceof ApiError ? e.message : String(e)),
  })

  const onUpdated = useCallback(
    (g: GameView) => {
      setSheet((d) => (d && d.id === g.id ? g : d))
      qc.invalidateQueries({ queryKey: HISTORY_LIST_KEY })
    },
    [qc, setSheet],
  )

  const toggle = (id: string) =>
    setFlipped((cur) => {
      const next = new Set(cur)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })

  const none = batches && batches.length === 0
  const neverPicked = none && !h.filtered

  return (
    <div className="px-4 pb-16 sm:px-10">
      {batches && !neverPicked && (
        <div className="flex flex-col gap-4 pt-8 pb-2 lg:flex-row lg:items-baseline lg:justify-between">
          <p data-tour="history" className="text-[17px] leading-[1.6] text-muted">
            Your picks <SentenceChoice label="When" value={period} options={PERIODS} onChange={h.setPeriod} />,{' '}
            <SentenceChoice label="Order" value={order} options={ORDERS} onChange={h.setOrder} />,{' '}
            <SentenceChoice label="What came of them" value={outcome} options={OUTCOMES} onChange={h.setOutcome} />.
          </p>
          <HistorySearch value={q} onChange={h.setQ} />
        </div>
      )}

      {isLoading && (
        <div aria-busy="true" aria-label="Loading history" className="pt-8">
          {[0, 1, 2].map((i) => (
            <div key={i} className="border-b border-line py-6">
              <div className="shimmer h-6 w-2/3 max-w-[520px] rounded-md" />
              <div className="shimmer mt-3 h-3 w-56 rounded" />
            </div>
          ))}
        </div>
      )}

      {error && !data && (
        <div className="py-10" role="alert">
          <p className="text-[15px] text-ink">Couldn't load your history.</p>
          <p className="mono mt-1">{(error as Error).message}</p>
          <button type="button" className="btn-outline mt-4 h-11" onClick={() => refetch()} disabled={isFetching}>
            {isFetching ? 'Trying again…' : 'Try again'}
          </button>
        </div>
      )}

      {neverPicked && (
        <div data-tour="history" className="py-10">
          <p className="text-[15.5px] text-muted">No picks yet. Ask for something and every round shows up here.</p>
          <Link to="/" className="btn-outline mt-4 h-11">
            Go to Pick
          </Link>
        </div>
      )}

      {none && h.filtered && (
        <div className="py-10">
          <p className="text-[15.5px] text-muted">No picks match that.</p>
          <button type="button" className="btn-outline mt-4 h-11" onClick={h.clear}>
            Show all picks
          </button>
        </div>
      )}

      {batches?.map((b, i) => (
        <HistoryBatch
          key={b.batch_id}
          batch={b}
          open={(i === 0) !== flipped.has(b.batch_id)}
          onToggle={() => toggle(b.batch_id)}
          onOpen={setSheet}
          rating={rate.isPending && rate.variables?.batchId === b.batch_id ? rate.variables.pick.id : null}
          onRate={(pick, verdict, reason) => rate.mutate({ batchId: b.batch_id, pick, verdict, reason })}
        />
      ))}

      {hasNextPage && (
        <div className="pt-8">
          <button type="button" className="btn-outline h-11" onClick={() => fetchNextPage()} disabled={isFetchingNextPage}>
            {isFetchingNextPage ? 'Loading…' : 'Show more'}
          </button>
        </div>
      )}

      <GameSheet game={sheet} onClose={() => setSheet(null)} onUpdated={onUpdated} />
    </div>
  )
}
