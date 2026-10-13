import type { ReactNode } from 'react'
import type { GameView } from '../../types'
import { Cover } from '../Cover'
import { PlayLink, TrailerLink } from './GameActions'
import { hoursToText } from '../../lib/format'
import { savedText } from '../../lib/gameText'

export function LaterRow({
  game,
  onOpen,
  onRemove,
  removing = false,
  meta: metaOverride,
  action,
}: {
  game: GameView
  onOpen: (g: GameView) => void
  /** Play later rows: the Remove button. */
  onRemove?: (g: GameView) => void
  removing?: boolean
  /** Replaces the default "9.1h · Saved Tuesday" line. */
  meta?: string
  /** Replaces the Remove button (e.g. a status menu). */
  action?: ReactNode
}) {
  const hours = hoursToText(game.hltb_main)
  const meta = metaOverride ?? [hours, savedText(game.saved_at)].filter(Boolean).join(' · ')
  return (
    <li className="flex flex-wrap items-center gap-x-6 gap-y-3 border-b border-line py-4">
      <button type="button" onClick={() => onOpen(game)} className="shrink-0 rounded-[8px]" aria-label={`Open ${game.name}`}>
        <Cover game={game} className="w-16 rounded-[8px]" titleSize="text-[10px]" />
      </button>
      <div className="flex min-w-0 flex-1 basis-56 flex-col gap-1.5">
        <button
          type="button"
          onClick={() => onOpen(game)}
          className="self-start text-left font-display text-[19px] leading-[1.2] font-bold tracking-[-0.01em] hover:text-accent"
        >
          {game.name}
        </button>
        <div className="mono">{meta}</div>
      </div>
      <div className="flex items-center gap-x-6 text-[14.5px]">
        <PlayLink game={game} />
        <TrailerLink game={game} />
        {action ??
          (onRemove && (
            <button
              type="button"
              onClick={() => onRemove(game)}
              disabled={removing}
              className="btn-quiet min-h-11"
              aria-label={`Remove ${game.name} from Play later`}
            >
              Remove
            </button>
          ))}
      </div>
    </li>
  )
}
