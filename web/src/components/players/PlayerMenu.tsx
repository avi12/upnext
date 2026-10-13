import { Lock } from 'lucide-react'
import { useNavigate } from 'react-router'
import { cx } from '../../lib/format'
import { askWhoIsPlaying, possessive, useChoosePlayer, usePlayers } from '../../lib/players'
import type { Player } from '../../types'
import { useMenu } from '../useMenu'
import { PlayerDisc } from './PlayerDisc'

/** The top bar's player, with two or more players: their colour
 *  and name, opening a small menu with their profile, the other players and
 *  Settings › Players. A device bound to one player can't switch. */
export function PlayerMenu({ className }: { className?: string }) {
  const navigate = useNavigate()
  const { current, others, bound } = usePlayers()
  const { open, close, wrapRef, buttonProps, menuProps } = useMenu<HTMLDivElement>()
  const choose = useChoosePlayer()
  if (!current) return null
  const go = (to: string) => {
    close()
    navigate(to)
  }
  const switchTo = (p: Player) => {
    close()
    if (p.has_pin) askWhoIsPlaying(p.id)
    else choose.mutate({ id: p.id })
  }
  const item = 'flex w-full items-center gap-2.5 rounded-lg px-3 py-2.5 text-left text-[14.5px] text-ink hover:bg-raised focus:bg-raised focus:outline-none'
  return (
    <div ref={wrapRef} className={cx('relative', className)}>
      <button
        {...buttonProps}
        aria-label={`Playing as ${current.name}`}
        className="flex h-11 shrink-0 items-center gap-2 rounded-[10px] px-3 text-[14.5px] text-muted hover:text-ink"
      >
        <PlayerDisc player={current} />
        <span className="hidden max-w-[140px] truncate 2xl:inline">{current.name}</span>
      </button>
      {open && (
        <div
          {...menuProps}
          aria-label="Players"
          className="fade-up absolute top-full right-0 z-50 mt-2 w-[min(260px,calc(100vw-32px))] rounded-xl border border-line-strong bg-sheet p-1.5 shadow-2xl"
        >
          <button type="button" role="menuitem" className={item} onClick={() => go('/profile')}>
            <PlayerDisc player={current} size={22} />
            <span className="min-w-0 truncate">{possessive(current.name)} profile</span>
          </button>
          {!bound && others.length > 0 && (
            <div className="mt-1 border-t border-line pt-1">
              {others.map((p) => (
                <button key={p.id} type="button" role="menuitem" className={item} onClick={() => switchTo(p)}>
                  <PlayerDisc player={p} size={22} />
                  <span className="min-w-0 flex-1 truncate">Play as {p.name}</span>
                  {p.has_pin && <Lock aria-label="Has a PIN" className="h-3.5 w-3.5 shrink-0 text-dim" />}
                </button>
              ))}
            </div>
          )}
          <div className="mt-1 border-t border-line pt-1">
            <button type="button" role="menuitem" className={cx(item, 'text-muted')} onClick={() => go('/settings#players')}>
              Players
            </button>
          </div>
          {bound && <p className="m-0 px-3 py-2 text-[12.5px] text-dim">This device is {possessive(current.name)}, so it can’t switch.</p>}
        </div>
      )}
    </div>
  )
}

/** Settings' top on a phone, where the top bar has no room for the player:
 *  who's playing, and a way to switch. */
export function PlayingAs() {
  const { current, multi, bound } = usePlayers()
  if (!multi || !current) return null
  return (
    <div className="flex items-center gap-3 border-b border-line pb-3 xl:hidden">
      <PlayerDisc player={current} size={28} />
      <span className="min-w-0 flex-1 truncate text-[15px]">
        Playing as <b className="font-semibold">{current.name}</b>
      </span>
      {!bound && (
        <button type="button" className="btn-outline" onClick={() => askWhoIsPlaying()}>
          Switch
        </button>
      )}
    </div>
  )
}
