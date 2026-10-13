import type { GameView } from '../../types'
import { PlayMark } from '../Shell'
import { cx } from '../../lib/format'
import { playUrl } from '../library/sheetText'
import { api } from '../../api'

/** Launch link for the game's preferred copy. */
export function PlayLink({ game, className }: { game: GameView; className?: string }) {
  const url = playUrl(game)
  if (!url) return null
  return (
    <a href={url} onClick={() => void api.diaryLaunch(game.id)} className={cx('btn-ink text-ground!', className)} aria-label={`Play ${game.name}`}>
      <PlayMark className="h-3 w-3" fill="currentColor" />
      Play
    </a>
  )
}

export function TrailerLink({ game, className }: { game: GameView; className?: string }) {
  if (!game.trailer_url) return null
  return (
    <a
      href={game.trailer_url}
      target="_blank"
      rel="noreferrer"
      className={cx('inline-flex min-h-11 items-center gap-[7px] font-semibold text-ink! hover:text-accent!', className)}
      aria-label={`Trailer for ${game.name} (opens YouTube)`}
    >
      <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinejoin="round" aria-hidden="true">
        <rect x="2.5" y="5" width="19" height="14" rx="4" />
        <path d="M10 9.5v5l4.5-2.5z" fill="currentColor" />
      </svg>
      Trailer
    </a>
  )
}
