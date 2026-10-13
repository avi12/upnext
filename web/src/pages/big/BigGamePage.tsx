import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useParams } from 'react-router'
import { api, ApiError } from '../../api'
import { DEFAULT_LINES, OVERRIDES, overrideMeta } from '../../lib/statuses'
import { STAR_WORDS } from '../../lib/ratings'
import { playUrl, stripBBCode } from '../../components/library/sheetText'
import { patchGame, useGameActions } from '../../components/pick/useGameActions'
import { couchFacts } from '../../components/bigscreen/facts'
import { Cover } from '../../components/Cover'
import { BigMenu, type MenuOption } from '../../components/bigscreen/BigMenu'
import { useToast } from '../../components/useToast'

/** Big screen's game page: cover, summary, Vibes, the facts for a couch,
 *  and Play, Play later, the marks and your rating. */
export function BigGamePage() {
  const { id } = useParams()
  const gameId = Number(id)
  const qc = useQueryClient()
  const toast = useToast()
  const game = useQuery({ queryKey: ['game', gameId], queryFn: () => api.game(gameId), enabled: Number.isFinite(gameId) })
  const { later, mark } = useGameActions()
  const [menu, setMenu] = useState<'mark' | 'rate' | null>(null)
  const rate = useMutation({
    mutationFn: (rating: number | null) => api.rateGame(gameId, rating),
    onSuccess: (g) => {
      patchGame(qc, g)
      qc.invalidateQueries({ queryKey: ['games'] })
      qc.invalidateQueries({ queryKey: ['taste-sources'] })
    },
    onError: (e) => toast.error('Couldn’t save your rating', e instanceof ApiError ? e.message : String(e)),
  })

  if (game.isPending) return <p className="pt-[2rem] text-[1.1rem] text-muted">Loading…</p>
  if (!game.data) return <p className="pt-[2rem] text-[1.1rem] text-muted">Couldn't load that game.</p>
  const g = game.data
  const url = playUrl(g)
  const saved = g.saved_at != null
  const current = overrideMeta(g.override)

  const marks: MenuOption[] = [
    ...OVERRIDES.filter((o) => o.kinds.includes(g.kind ?? 'ending')).map((o) => ({
      key: o.value,
      label: o.label,
      hint: o.hint(DEFAULT_LINES),
      checked: g.override === o.value,
      onSelect: () => mark.mutate({ id: g.id, status: g.override === o.value ? null : o.value }),
    })),
    ...(g.override ? [{ key: 'clear', label: 'Clear the mark', onSelect: () => mark.mutate({ id: g.id, status: null }) }] : []),
  ]
  const ratings: MenuOption[] = [
    ...[5, 4, 3, 2, 1].map((n) => ({
      key: String(n),
      label: `${'★'.repeat(n)} ${STAR_WORDS[n - 1]}`,
      checked: g.my_rating === n,
      onSelect: () => rate.mutate(n),
    })),
    ...(g.my_rating ? [{ key: 'clear', label: 'Take my rating back', onSelect: () => rate.mutate(null) }] : []),
  ]
  const summary = g.short_description ? stripBBCode(g.short_description) : null

  return (
    <article aria-labelledby="big-game-title" className="grid grid-cols-[minmax(0,16rem)_minmax(0,1fr)] gap-[2rem] pt-[2rem]">
      <Cover game={g} shape="portrait" titleSize="text-[1.4rem]" className="w-full rounded-[1rem]" />
      <div className="flex min-w-0 flex-col gap-[1rem]">
        <h1 id="big-game-title" className="m-0 font-display text-[2.2rem] leading-tight font-bold tracking-[-0.015em]">
          {g.name}
        </h1>
        <p className="m-0 text-[1rem] text-muted">{couchFacts(g).join(' · ')}</p>
        <div className="flex flex-wrap gap-[0.6rem]">
          {url && (
            <a href={url} className="big-btn-ink" data-focus-first="" onClick={() => void api.diaryLaunch(g.id)}>
              Play
            </a>
          )}
          <button type="button" className="big-btn-outline" aria-pressed={saved} disabled={later.isPending} onClick={() => later.mutate({ id: g.id, save: !saved })}>
            {saved ? 'Saved for later' : 'Save for later'}
          </button>
          <button type="button" className="big-btn-outline" onClick={() => setMenu('mark')}>
            {current ? `Marked: ${current.label}` : 'Mark as…'}
          </button>
          <button type="button" className="big-btn-outline" onClick={() => setMenu('rate')}>
            {g.my_rating ? `Your rating: ${'★'.repeat(g.my_rating)}` : 'Rate it'}
          </button>
        </div>
        {summary && <p className="m-0 max-w-[48rem] text-[1.1rem] leading-[1.5]">{summary}</p>}
        {g.vibes && (
          <p className="m-0 max-w-[48rem] text-[1.05rem] leading-[1.5] text-muted">
            <span className="font-semibold text-ink">Vibes: </span>
            {g.vibes}
          </p>
        )}
        {g.digest && (
          <p className="m-0 max-w-[48rem] text-[1.05rem] leading-[1.5] text-muted">
            <span className="font-semibold text-ink">People say: </span>
            {g.digest}
          </p>
        )}
      </div>
      {menu === 'mark' && <BigMenu title={`Mark ${g.name}`} options={marks} onClose={() => setMenu(null)} />}
      {menu === 'rate' && <BigMenu title={`Rate ${g.name}`} options={ratings} onClose={() => setMenu(null)} />}
    </article>
  )
}
