import { useState } from 'react'
import type { GameView } from '../types'
import { cx, toneVar } from '../lib/format'

/** Game art with a rounded frame. `portrait` (2:3) uses the square/box art,
 *  `wide` (460:215) the store header. Without art, or when an image fails,
 *  shows a theme-toned tile with the title set in the display face. */
export function Cover({
  game,
  shape = 'portrait',
  className,
  titleSize = 'text-[15px]',
}: {
  game: Pick<GameView, 'id' | 'name' | 'appid' | 'art_square' | 'header_image'> & Partial<Pick<GameView, 'portrait_url'>>
  shape?: 'portrait' | 'wide'
  className?: string
  titleSize?: string
}) {
  const src =
    shape === 'portrait'
      ? (game.art_square ??
        game.portrait_url ??
        (game.appid ? `https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/${game.appid}/library_600x900.jpg` : null))
      : game.header_image || null
  const [failed, setFailed] = useState(false)
  const showArt = !!src && !failed
  return (
    <div
      className={cx(
        'relative overflow-hidden rounded-[10px]',
        shape === 'portrait' ? 'aspect-[2/3]' : 'aspect-[460/215]',
        className,
      )}
      style={{ background: toneVar(game.id) }}
    >
      {showArt ? (
        <img src={src} alt="" loading="lazy" onError={() => setFailed(true)} className="h-full w-full object-cover" />
      ) : (
        <div
          className={cx('absolute inset-0 flex items-end p-3 font-display font-bold leading-[1.1]', titleSize)}
          style={{ color: 'var(--cover-ink)' }}
        >
          {game.name}
        </div>
      )}
    </div>
  )
}
