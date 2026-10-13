import { useId } from 'react'
import { ChevronDown } from 'lucide-react'
import type { GameView, MissReason, OverrideStatus, OwnedPick, Pick, PickBatch, PickVerdict, WishlistedPick } from '../../types'
import { Cover } from '../Cover'
import { wishCover, wishMeta } from '../pick/text'
import { PlayLink, TrailerLink } from './GameActions'
import { absoluteTime, cx, hoursToText, relativeTime, storeName } from '../../lib/format'
import { yearOf } from '../../lib/gameText'
import { usageText } from '../../lib/jobs'
import { Money } from '../Money'
import { overrideMeta } from '../../lib/statuses'
import { RatePick } from '../pick/RatePick'

/** Rate a pick of this lineup, or take its rating back (null). */
export type RateHandler = (pick: OwnedPick, verdict: PickVerdict | null, reason?: MissReason | null) => void

function pickMeta(g: GameView): string {
  const year = yearOf(g)
  return [
    hoursToText(g.hltb_main) && `${hoursToText(g.hltb_main)} story`,
    g.review_pct != null && `${g.review_pct}% liked it`,
    storeName(g.primary_source),
    year,
  ]
    .filter(Boolean)
    .join(' · ')
}

/** Why a pick was taken off its lineup: "too long", "not tonight". */
function takenOffText(p: Pick): string {
  const f = p.feedback
  switch (f?.action) {
    case 'not_tonight':
      return 'not tonight'
    case 'too':
      return `too ${f.text}`
    case 'same_problem':
      return `too ${f.text} as well`
    case 'not_for_me':
      return 'never again'
    case 'started':
      return `started: ${overrideMeta(f.text as OverrideStatus | null)?.label.toLowerCase() ?? f.text}`
    default:
      return 'taken off'
  }
}

/** A wishlisted game you don't own that stood in the lineup. */
function WishRow({ pick }: { pick: WishlistedPick }) {
  const g = pick.wishlist
  const meta = wishMeta(g)
  return (
    <li className="grid grid-cols-[112px_minmax(0,1fr)] items-start gap-x-5 gap-y-3 border-t border-line-soft py-5 first:border-t-0 sm:grid-cols-[184px_minmax(0,1fr)] sm:gap-x-7">
      <Cover game={wishCover(g)} shape="wide" titleSize="text-[13px]" />
      <div className="flex min-w-0 flex-col gap-1.5">
        <div className="font-display text-[19px] leading-[1.2] font-bold tracking-[-0.01em]">{pick.name || g.name}</div>
        <div className="mono">
          {meta ? `${meta} · ` : ''}not owned, on your {g.on_steam ? 'Steam ' : ''}wishlist
        </div>
        {pick.reason && <p className="mt-1 max-w-[70ch] text-[15px] leading-[1.6] text-ink">{pick.reason}</p>}
      </div>
    </li>
  )
}

/** How the lineup's picks were rated: "2 good picks, 1 not a fit". */
function ratingsText(batch: PickBatch): string | null {
  const all = [...batch.picks, ...(batch.removed ?? [])]
  const good = all.filter((p) => p.rating?.verdict === 'good').length
  const miss = all.filter((p) => p.rating?.verdict === 'not_a_fit').length
  const parts = [good && `${good} ${good === 1 ? 'good pick' : 'good picks'}`, miss && `${miss} not a fit`].filter(Boolean)
  return parts.length ? parts.join(', ') : null
}

function PickRow({
  pick,
  onOpen,
  rating,
  onRate,
}: {
  pick: OwnedPick
  onOpen: (g: GameView) => void
  rating: boolean
  onRate?: RateHandler
}) {
  const g = pick.game
  return (
    <li className="grid grid-cols-[112px_minmax(0,1fr)] items-start gap-x-5 gap-y-3 border-t border-line-soft py-5 first:border-t-0 sm:grid-cols-[184px_minmax(0,1fr)] sm:gap-x-7">
      <button type="button" onClick={() => onOpen(g)} className="block rounded-[10px]" aria-label={`Open ${g.name}`}>
        <Cover game={g} shape="wide" titleSize="text-[13px]" />
      </button>
      <div className="flex min-w-0 flex-col gap-1.5">
        <button
          type="button"
          onClick={() => onOpen(g)}
          className="self-start text-left font-display text-[19px] leading-[1.2] font-bold tracking-[-0.01em] hover:text-accent"
        >
          {pick.name || g.name}
        </button>
        <div className="mono">
          {pickMeta(g)}
          {pick.feedback?.action === 'chosen' && <span className="text-accent"> · you played this one</span>}
        </div>
        {pick.reason && <p className="mt-1 max-w-[70ch] text-[15px] leading-[1.6] text-ink">{pick.reason}</p>}
        <div className="mt-2 flex flex-wrap items-center gap-x-6 text-[14.5px]">
          <PlayLink game={g} />
          <TrailerLink game={g} />
        </div>
        {onRate && (
          <div className="mt-3">
            <RatePick rating={pick.rating} busy={rating} onRate={(verdict, reason) => onRate(pick, verdict, reason)} />
          </div>
        )}
      </div>
    </li>
  )
}

export function HistoryBatch({
  batch,
  open,
  onToggle,
  onOpen,
  rating = null,
  onRate,
}: {
  batch: PickBatch
  open: boolean
  onToggle: () => void
  onOpen: (g: GameView) => void
  /** The title id whose rating is being saved. */
  rating?: number | null
  /** Rate a pick from here; without it the rows only show the lineup. */
  onRate?: RateHandler
}) {
  const panel = useId()
  const n = batch.picks.length
  const rated = ratingsText(batch)
  return (
    <section className="border-b border-line">
      <h2>
        <button
          type="button"
          onClick={onToggle}
          aria-expanded={open}
          aria-controls={panel}
          className="group flex min-h-11 w-full items-start gap-4 py-6 text-left"
        >
          <span className="block min-w-0 flex-1">
            <span
              className={cx(
                // A long request with no spaces wraps anywhere rather than widen the page.
                'block font-display text-[22px] leading-[1.2] font-bold tracking-[-0.015em] [overflow-wrap:anywhere] group-hover:text-accent',
                !batch.prompt && 'text-muted',
              )}
            >
              {batch.prompt || (batch.mode === 'random' ? 'Rolled the dice' : 'No request, just a surprise')}
            </span>
            <span className="mono mt-2 block">
              <span title={absoluteTime(batch.created_at)}>{relativeTime(batch.created_at)}</span> · {batch.mode === 'random' ? 'drawn at random' : batch.ranked_by ? 'picked without AI' : batch.model} · {n}{' '}
              {n === 1 ? 'pick' : 'picks'}
              {batch.routine && ` · for “${batch.routine.name}”`}
              {rated && ` · ${rated}`}
              {batch.usage && ` · ${usageText(batch.usage)}`}
              <Money cost={batch} prefix=" · " />
            </span>
            {!open && n > 0 && (
              <span className="mt-2 block truncate text-[14px] font-normal text-dim">
                {batch.picks.map((p) => p.name || p.game?.name).join(' · ')}
              </span>
            )}
          </span>
          <ChevronDown
            aria-hidden="true"
            className={cx('mt-1.5 h-5 w-5 shrink-0 text-dim transition-transform group-hover:text-ink', open && 'rotate-180')}
          />
        </button>
      </h2>
      {open && (
        <ol id={panel} className="pb-6">
          {batch.picks.map((p) =>
            p.owned === false ? (
              <WishRow key={p.id} pick={p} />
            ) : (
              <PickRow key={p.id} pick={p} onOpen={onOpen} rating={rating === p.id} onRate={onRate} />
            ),
          )}
        </ol>
      )}
      {open && batch.removed?.length > 0 && (
        <p className="m-0 -mt-2 pb-6 text-[14px] text-dim">
          Taken off:{' '}
          {batch.removed.map((p, i) => (
            <span key={p.id}>
              {i > 0 && ' · '}
              {p.owned === false ? (
                <>
                  <span className="text-muted">{p.name}</span> (switched for an owned game)
                </>
              ) : (
                <>
                  <button type="button" onClick={() => onOpen(p.game)} className="text-muted hover:text-ink hover:underline">
                    {p.name || p.game.name}
                  </button>{' '}
                  ({takenOffText(p)})
                </>
              )}
            </span>
          ))}
        </p>
      )}
    </section>
  )
}
