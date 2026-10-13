import { useState } from 'react'
import { useMutation, useQuery } from '@tanstack/react-query'
import { useNavigate, useSearchParams } from 'react-router'
import { api, ApiError } from '../../api'
import type { GameView } from '../../types'
import { PLAY_LATER_KEY, useLastBatch } from '../../components/pick/useGameActions'
import { playUrl } from '../../components/library/sheetText'
import { metaLine } from '../../components/pick/text'
import { Cover } from '../../components/Cover'

/** Big screen's Help me decide: choose two games from the lineup and Play
 *  later, and the AI says which suits the moment better. */
export function BigDecidePage() {
  const navigate = useNavigate()
  const [params] = useSearchParams()
  const last = useLastBatch()
  const later = useQuery({ queryKey: PLAY_LATER_KEY, queryFn: api.playLater })
  const fromAddress = [params.get('a'), params.get('b')].map(Number).filter((n) => n > 0)
  const [chosen, setChosen] = useState<number[]>(fromAddress)

  const seen = new Set<number>()
  const games: GameView[] = []
  for (const g of [...(last.data?.picks ?? []).flatMap((p) => (p.owned === false ? [] : [p.game])), ...(later.data ?? [])]) {
    if (!seen.has(g.id)) {
      seen.add(g.id)
      games.push(g)
    }
  }

  const decide = useMutation({ mutationFn: () => api.decide([chosen[0], chosen[1]], last.data?.prompt || undefined) })
  const winner = decide.data ? games.find((g) => g.id === decide.data.pick_id) : null
  const toggle = (id: number) => {
    decide.reset()
    setChosen((c) => (c.includes(id) ? c.filter((x) => x !== id) : [...c, id].slice(-2)))
  }

  return (
    <div className="flex flex-col gap-[1.25rem] pt-[1.5rem]">
      <h1 className="m-0 font-display text-[1.6rem] font-bold">Help me decide</h1>
      <p className="m-0 text-[1.05rem] text-muted">Choose two games from your lineup and Play later, and the AI says which fits better right now.</p>
      {winner && decide.data && (
        <section aria-label="The pick" role="status" className="flex flex-wrap items-start gap-[1.5rem] rounded-[1rem] bg-raised p-[1.25rem]">
          <Cover game={winner} shape="wide" titleSize="text-[1.2rem]" className="w-[18rem] rounded-[0.8rem]" />
          <div className="flex min-w-[16rem] flex-1 flex-col gap-[0.6rem]">
            <h2 className="m-0 font-display text-[1.5rem] font-bold">Go with {winner.name}</h2>
            <p className="m-0 text-[1.05rem] leading-[1.5]">{decide.data.reason}</p>
            <div className="flex flex-wrap gap-[0.5rem]">
              {playUrl(winner) && (
                <a href={playUrl(winner)!} className="big-btn-ink" data-focus-first="" onClick={() => void api.diaryLaunch(winner.id)}>
                  Play
                </a>
              )}
              <button type="button" className="big-btn-outline" onClick={() => navigate(`/game/${winner.id}`)}>
                Open the game page
              </button>
            </div>
          </div>
        </section>
      )}
      <div className="flex flex-wrap items-center gap-[0.75rem]">
        <button type="button" className="big-btn-accent" disabled={chosen.length !== 2 || decide.isPending} onClick={() => decide.mutate()}>
          {decide.isPending ? 'Deciding…' : 'Help me decide'}
        </button>
        <span className="text-[1rem] text-muted" aria-live="polite">
          {chosen.length === 2 ? 'Two chosen.' : chosen.length === 1 ? 'Choose one more.' : 'Choose two.'}
        </span>
        {decide.error && (
          <span role="alert" className="text-[1rem] text-bad">
            {decide.error instanceof ApiError ? decide.error.message : String(decide.error)}
          </span>
        )}
      </div>
      {games.length ? (
        <ul aria-label="Games to choose from" data-focus-group="decide" className="m-0 grid list-none grid-cols-[repeat(auto-fill,minmax(15rem,1fr))] gap-[1.25rem] p-0">
          {games.map((g) => (
            <li key={g.id}>
              <button type="button" aria-pressed={chosen.includes(g.id)} onClick={() => toggle(g.id)} className="flex w-full flex-col gap-[0.4rem] rounded-[0.8rem] p-[0.4rem] text-left aria-pressed:bg-accent aria-pressed:text-accent-ink">
                <Cover game={g} shape="wide" titleSize="text-[1.1rem]" className="w-full rounded-[0.6rem]" />
                <span className="text-[1.05rem] font-semibold">{g.name}</span>
                <span className="text-[0.85rem] opacity-80">{metaLine(g)}</span>
              </button>
            </li>
          ))}
        </ul>
      ) : (
        <p className="m-0 text-[1.05rem] text-muted">{last.isPending || later.isPending ? 'Loading…' : 'Curate a lineup or save games for later first.'}</p>
      )}
    </div>
  )
}
