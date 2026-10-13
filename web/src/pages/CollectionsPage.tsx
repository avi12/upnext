import { useCallback, useMemo, useState } from 'react'
import { Link, useSearchParams } from 'react-router'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { api } from '../api'
import type { GameView } from '../types'
import { GameSheet } from '../components/GameSheet'
import { SentenceChoice } from '../components/SentenceMenu'
import { SeriesRow } from '../components/collections/SeriesRow'
import {
  ORDER_LABEL,
  SHOW_LABEL,
  buildSeries,
  matchesShow,
  type SeriesOrder,
  type SeriesShow,
} from '../components/collections/series'

const SHOW_OPTIONS = (Object.keys(SHOW_LABEL) as SeriesShow[]).map((value) => ({ value, label: SHOW_LABEL[value] }))
const ORDER_OPTIONS = (Object.keys(ORDER_LABEL) as SeriesOrder[]).map((value) => ({ value, label: ORDER_LABEL[value] }))

function isShow(v: string | null): v is SeriesShow {
  return v != null && v in SHOW_LABEL
}
function isOrder(v: string | null): v is SeriesOrder {
  return v != null && v in ORDER_LABEL
}

function LoadingRows() {
  return (
    <div aria-busy="true" aria-label="Loading series">
      {[0, 1, 2].map((i) => (
        <div key={i} className="grid grid-cols-1 gap-6 border-b border-line py-8 lg:grid-cols-[280px_minmax(0,1fr)] lg:gap-x-12">
          <div className="flex flex-col gap-3">
            <div className="shimmer h-8 w-48 rounded-md" />
            <div className="shimmer h-3 w-40 rounded" />
            <div className="shimmer mt-2 h-16 w-full rounded-md" />
          </div>
          <div className="flex gap-[18px] overflow-hidden">
            {[0, 1, 2, 3].map((j) => (
              <div key={j} className="shimmer aspect-[2/3] w-[132px] shrink-0 rounded-[10px]" />
            ))}
          </div>
        </div>
      ))}
    </div>
  )
}

export function CollectionsPage() {
  const qc = useQueryClient()
  const [params, setParams] = useSearchParams()
  const showParam = params.get('show')
  const orderParam = params.get('order')
  const show: SeriesShow = isShow(showParam) ? showParam : 'started'
  const order: SeriesOrder = isOrder(orderParam) ? orderParam : 'play'
  const [sheet, setSheet] = useState<GameView | null>(null)

  const { data, isLoading, error, refetch, isFetching } = useQuery({
    queryKey: ['collections'],
    queryFn: api.collections,
    staleTime: 60_000,
  })

  const all = useMemo(() => (data ?? []).map((c) => buildSeries(c, order)), [data, order])
  const shown = useMemo(() => all.filter((s) => matchesShow(s, show)), [all, show])

  const setParam = (key: string, value: string, fallback: string) => {
    const next = new URLSearchParams(params)
    if (value === fallback) next.delete(key)
    else next.set(key, value)
    setParams(next, { replace: true })
  }

  const onUpdated = useCallback(
    (g: GameView) => {
      setSheet((cur) => (cur && cur.id === g.id ? g : cur))
      qc.invalidateQueries({ queryKey: ['collections'] })
    },
    [qc],
  )

  const total = data?.length ?? 0
  const countText =
    shown.length === total ? `${total} series with 2+ games` : `${shown.length} of ${total} series with 2+ games`

  return (
    <div className="px-4 pb-16 sm:px-10">
      <section data-tour="collections" className="pt-8 pb-2">
        <p className="text-[17px] leading-[1.6] text-muted">
          <SentenceChoice label="Which series" value={show} options={SHOW_OPTIONS} onChange={(v) => setParam('show', v, 'started')} />
          , in{' '}
          <SentenceChoice label="Order" value={order} options={ORDER_OPTIONS} onChange={(v) => setParam('order', v, 'play')} />
          {data && total > 0 && <span className="mono ml-2.5 inline-block text-[11.5px]">{countText}</span>}
        </p>
      </section>

      {isLoading && <LoadingRows />}

      {error && !data && (
        <div className="border-b border-line py-10" role="alert">
          <p className="text-[15px] text-ink">Couldn't load your series.</p>
          <p className="mono mt-1">{(error as Error).message}</p>
          <button type="button" className="btn-outline mt-4 h-11" onClick={() => refetch()} disabled={isFetching}>
            {isFetching ? 'Trying again…' : 'Try again'}
          </button>
        </div>
      )}

      {data && total === 0 && (
        <div className="max-w-[640px] py-10">
          <p className="font-display text-[22px] font-bold">No series yet</p>
          <p className="mt-2 text-[15.5px] leading-[1.6] text-muted">
            Series come from enrichment. Turn on series (and series guides, if you want tips on where to start) and run it,
            and any series you own two or more games from shows up here.
          </p>
          <Link to="/settings#data" className="btn-outline mt-5 h-11">
            Go to enrichment
          </Link>
        </div>
      )}

      {data && total > 0 && shown.length === 0 && (
        <div className="py-10">
          <p className="text-[15.5px] text-muted">
            {show === 'finished'
              ? "You haven't played every main game of any series yet."
              : show === 'started'
                ? "You haven't started any of these series yet."
                : "You've started every series you own."}
          </p>
          <button type="button" className="btn-quiet mt-3 min-h-11 text-[15px] underline underline-offset-4" onClick={() => setParam('show', 'all', 'started')}>
            Show all series
          </button>
        </div>
      )}

      {shown.map((s) => (
        <SeriesRow key={s.c.key} s={s} onOpen={setSheet} />
      ))}

      <GameSheet game={sheet} onClose={() => setSheet(null)} onUpdated={onUpdated} />
    </div>
  )
}
