import { memo } from 'react'
import type { GameView } from '../../types'
import { Cover } from '../Cover'
import { cx, hoursToText, linuxClass, linuxLabel } from '../../lib/format'
import { statusShort } from './status'
import { RatingMark } from './RatingMark'

const GRID = 'grid grid-cols-[repeat(auto-fill,minmax(136px,1fr))] gap-x-6 gap-y-8'

/** One cover in the grid. Memoised: opening a game re-renders the page, not every tile. */
const CoverTile = memo(function CoverTile({ g, linuxActive, onOpen }: { g: GameView; linuxActive: boolean; onOpen: (g: GameView) => void }) {
  const status = statusShort(g)
  // The story's length, labelled so it can't be read as time played.
  const hltb = hoursToText(g.hltb_main)
  const meta = [hltb && `${hltb} story`, g.review_pct != null ? `${g.review_pct}%` : null].filter(Boolean).join(' · ')
  const linux = linuxActive ? linuxLabel(g.linux) : null
  return (
    <li className="min-w-0">
      <button
        type="button"
        onClick={() => onOpen(g)}
        className="group flex w-full flex-col gap-[9px] rounded-[10px] text-left"
      >
        <Cover game={g} className="w-full transition-transform duration-200 group-hover:-translate-y-0.5" />
        <span className="flex min-w-0 flex-col gap-[3px]">
          <span className="truncate text-sm font-semibold leading-[1.3] text-ink group-hover:underline group-hover:decoration-underline group-hover:underline-offset-4">
            {g.name}
          </span>
          {/* The status on a line of its own, so the story length and the
              score after it aren't cut off on a narrow cover. */}
          <span className="mono truncate text-[10.5px] text-muted">
            {status}
            {g.my_rating != null && (
              <>
                {' · '}
                <RatingMark stars={g.my_rating} />
              </>
            )}
          </span>
          <span className="mono flex min-w-0 items-center gap-1.5 text-[10.5px]">
            {meta && <span className="truncate">{meta}</span>}
            {linux && (
              <span className={cx('shrink-0', linuxClass(g.linux))} title={`Linux: ${linux}`}>
                <span aria-hidden="true">●</span>
                <span className="sr-only">Linux: {linux}</span>
              </span>
            )}
          </span>
        </span>
      </button>
    </li>
  )
})

/** What the coloured dot on a cover means, above the grid while Linux
 *  compatibility is on: a green dot otherwise reads as "synced". */
function LinuxLegend() {
  const dot = (tone: string, text: string) => (
    <span className="inline-flex items-center gap-1.5">
      <span aria-hidden="true" className={tone}>
        ●
      </span>
      {text}
    </span>
  )
  return (
    <p className="mono m-0 mb-5 flex flex-wrap items-center gap-x-4 gap-y-1 text-[11px] text-dim">
      <span>The dot is the Linux rating, from ProtonDB:</span>
      {dot('text-good', 'runs well')}
      {dot('text-warn', 'has issues')}
      {dot('text-bad', 'broken')}
    </p>
  )
}

/** Covers view: portrait art, title, the status ("Not started") and a mono line ("8.6h story · 99%"). */
export function CoverGrid({
  items,
  linuxActive,
  onOpen,
  dimmed,
}: {
  items: GameView[]
  linuxActive: boolean
  onOpen: (g: GameView) => void
  dimmed?: boolean
}) {
  return (
    <>
      {linuxActive && <LinuxLegend />}
      <ul className={cx(GRID, 'transition-opacity', dimmed && 'opacity-70')}>
        {items.map((g) => (
          <CoverTile key={g.id} g={g} linuxActive={linuxActive} onOpen={onOpen} />
        ))}
      </ul>
    </>
  )
}

export function CoverGridSkeleton({ count = 16 }: { count?: number }) {
  return (
    <div className={GRID} aria-hidden="true">
      {Array.from({ length: count }).map((_, i) => (
        <div key={i} className="flex flex-col gap-[9px]">
          <div className="shimmer aspect-[2/3] rounded-[10px]" />
          <div className="shimmer h-3.5 w-3/4 rounded" />
          <div className="shimmer h-2.5 w-1/2 rounded" />
        </div>
      ))}
    </div>
  )
}
