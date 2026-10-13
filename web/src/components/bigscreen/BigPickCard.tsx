import { useState } from 'react'
import { useNavigate } from 'react-router'
import type { FeedbackAction, MissReason, OwnedPick, PickVerdict } from '../../types'
import { api } from '../../api'
import { cx } from '../../lib/format'
import { playUrl } from '../library/sheetText'
import { useGameActions } from '../pick/useGameActions'
import { metaLine } from '../pick/text'
import { Cover } from '../Cover'
import { BigMenu, type MenuOption } from './BigMenu'
import { Hint } from './Glyph'
import { useActions } from './pad'

/** The words "It's too ___" offers on a pad, where typing one is slow. */
const TOO = ['long', 'short', 'slow', 'hard', 'grindy']

/** One pick in big screen's lineup: the art and name open the game page, and
 *  under them Play, Not tonight and the ratings Good pick and Not a fit. X (or
 *  M) opens the rest: Play later, "It's too ___" and Never recommend this. */
export function BigPickCard({
  pick,
  number,
  busy,
  sifting,
  onAct,
  onToo,
  onRate,
  first,
}: {
  pick: OwnedPick
  number: number
  busy: boolean
  sifting: string | null
  onAct: (action: FeedbackAction) => void
  onToo: (word: string) => void
  onRate: (verdict: PickVerdict | null, reason?: MissReason | null) => void
  first?: boolean
}) {
  const g = pick.game
  const navigate = useNavigate()
  const { later } = useGameActions()
  const [menu, setMenu] = useState(false)
  const ref = useActions<HTMLElement>((a) => {
    if (a !== 'menu') return false
    setMenu(true)
    return true
  })
  const url = playUrl(g)
  const saved = g.saved_at != null
  const verdict = pick.rating?.verdict ?? null
  const locked = busy || sifting != null
  const titleId = `big-pick-${g.id}`

  const options: MenuOption[] = [
    { key: 'open', label: 'Open the game page', onSelect: () => navigate(`/game/${g.id}`) },
    { key: 'later', label: saved ? 'Take off Play later' : 'Save for later', onSelect: () => later.mutate({ id: g.id, save: !saved }) },
    ...TOO.map((w) => ({ key: `too-${w}`, label: `It's too ${w}`, hint: 'Swap it for one that isn’t', disabled: locked, onSelect: () => onToo(w) })),
    { key: 'never', label: 'Never recommend this', disabled: locked, onSelect: () => onAct('not_for_me') },
  ]

  return (
    <article ref={ref} aria-labelledby={titleId} aria-busy={sifting != null} className="fade-up flex min-w-0 flex-col gap-[0.75rem]">
      <button
        type="button"
        data-focus-first={first ? '' : undefined}
        onClick={() => navigate(`/game/${g.id}`)}
        className={cx('flex flex-col gap-[0.6rem] rounded-[0.9rem] text-left', sifting != null && 'sifting opacity-70')}
      >
        <Cover game={g} shape="wide" titleSize="text-[1.3rem]" className="w-full rounded-[0.9rem]" />
        <span className="flex items-baseline gap-[0.6rem]">
          <span aria-hidden="true" className="font-display text-[1.6rem] leading-none font-extrabold text-accent">
            {number}
          </span>
          <span id={titleId} className="font-display text-[1.4rem] leading-tight font-bold">
            {g.name}
          </span>
        </span>
      </button>
      <div className="text-[0.85rem] text-dim">{metaLine(g)}</div>
      {sifting != null ? (
        <p role="status" className="m-0 text-[1rem] text-accent">
          Looking for one that isn't too {sifting}…
        </p>
      ) : (
        <p className="m-0 line-clamp-4 text-[1rem] leading-[1.45]">{pick.reason}</p>
      )}
      <div className="flex flex-wrap gap-[0.5rem]">
        {url && (
          <a
            href={url}
            className="big-btn-ink"
            onClick={() => {
              onAct('chosen')
              void api.diaryLaunch(g.id)
            }}
          >
            Play
          </a>
        )}
        <button type="button" className="big-btn-outline" disabled={locked} onClick={() => onAct('not_tonight')}>
          Not tonight
        </button>
      </div>
      <div role="group" aria-label="How well this pick fits your request" className="flex flex-wrap gap-[0.5rem]">
        {(['good', 'not_a_fit'] as const).map((v) => (
          <button
            key={v}
            type="button"
            className="big-chip"
            aria-pressed={verdict === v}
            disabled={busy}
            onClick={() => onRate(verdict === v ? null : v)}
          >
            {v === 'good' ? 'Good pick' : 'Not a fit'}
          </button>
        ))}
      </div>
      <Hint action="menu">More</Hint>
      {menu && <BigMenu title={g.name} options={options} onClose={() => setMenu(false)} />}
    </article>
  )
}
