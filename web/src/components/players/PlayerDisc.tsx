import { cx } from '../../lib/format'
import { initial } from '../../lib/players'
import type { Player } from '../../types'

/** A player's colour with the first letter of their name. */
export function PlayerDisc({ player, size = 24, className }: { player: Pick<Player, 'name' | 'colour'>; size?: number; className?: string }) {
  return (
    <span
      aria-hidden="true"
      className={cx('inline-flex shrink-0 items-center justify-center rounded-full font-semibold text-white', className)}
      style={{ background: player.colour, width: size, height: size, fontSize: Math.round(size * 0.46) }}
    >
      {initial(player.name)}
    </span>
  )
}
