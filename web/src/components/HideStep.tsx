import { useEffect, useRef, type CSSProperties, type KeyboardEvent } from 'react'
import type { HideScope } from '../types'
import { HIDDEN_ONLY_HERE, HIDE_CHOICES, hiddenText, usePlayers } from '../lib/players'
import { cx } from '../lib/format'
import { useHideGame, type Hideable } from './useHideGame'

/** The Mark as… menu's "Hide this game" step: just from me, from
 *  everyone else or from everyone, and Unhide. With one player, hiding is
 *  only from yourself. Hiding isn't Not for me: it teaches nothing. */
export function HideStep({
  game,
  className,
  style,
  onBack,
  onCancel,
  onHidden,
  onDone,
}: {
  game: Hideable
  className: string
  style: CSSProperties
  onBack: () => void
  onCancel: () => void
  /** After it was hidden (a scope) or shown again (null). */
  onHidden?: (scope: HideScope | null) => void
  /** A choice was made: the menu closes while it's saved. */
  onDone: () => void
}) {
  const { multi } = usePlayers()
  const hide = useHideGame(onHidden)
  const onPick = (scope: HideScope | null) => {
    hide.mutate({ game, scope })
    onDone()
  }
  const ref = useRef<HTMLDivElement>(null)
  const choices = multi ? HIDE_CHOICES : HIDE_CHOICES.filter((c) => c.scope === 'me')
  useEffect(() => {
    ref.current?.querySelector<HTMLElement>('button[data-first]')?.focus()
  }, [])
  const onKeyDown = (e: KeyboardEvent) => {
    if (e.key === 'Escape') {
      e.preventDefault()
      // Don't let Esc also close the sheet underneath.
      e.stopPropagation()
      e.nativeEvent.stopImmediatePropagation()
      onCancel()
    }
  }
  const item = 'flex w-full flex-col items-start rounded-lg px-3 py-2.5 text-left hover:bg-raised focus:bg-raised focus:outline-none disabled:opacity-50 disabled:hover:bg-transparent'
  return (
    <div ref={ref} role="dialog" aria-label="Hide this game" onKeyDown={onKeyDown} className={className} style={style}>
      <div className="px-3 pt-2.5 pb-1">
        <button type="button" onClick={onBack} className="btn-quiet -ml-1 px-1 text-[13px] text-dim hover:text-ink">
          ← Mark as
        </button>
        <p className="m-0 mt-1 text-[14.5px] font-semibold text-ink">Hide this game</p>
        <p className="m-0 text-[13px] leading-snug text-muted">Out of sight, without telling UpNext anything about your taste.</p>
      </div>
      {choices.map((c, i) => {
        const blocked = c.needsCopy && !game.own_copy
        const on = game.hidden_scope === c.scope
        return (
          <button
            key={c.scope}
            type="button"
            aria-pressed={on}
            disabled={blocked}
            data-first={i === 0 ? '' : undefined}
            onClick={() => (on ? onCancel() : onPick(c.scope))}
            className={cx(item, on && 'bg-raised')}
          >
            <span className="text-[14.5px] font-semibold text-ink">{multi ? c.label : 'Hide it'}</span>
            <span className="text-[13px] leading-snug text-muted">
              {blocked ? 'Only for a game in an account of your own; this copy is someone else’s.' : c.hint}
            </span>
          </button>
        )
      })}
      {game.hidden_scope && (
        <button type="button" onClick={() => onPick(null)} className={cx(item, 'mt-1 border-t border-line')}>
          <span className="text-[14px] text-ink">Unhide</span>
          <span className="text-[12.5px] leading-snug text-dim">{hiddenText(game.hidden_scope)} now.</span>
        </button>
      )}
      <p className="m-0 border-t border-line px-3 pt-2 pb-2.5 text-[12.5px] leading-snug text-dim">
        {HIDDEN_ONLY_HERE} Everything you hide is on Lists, under Hidden by me.
      </p>
    </div>
  )
}
