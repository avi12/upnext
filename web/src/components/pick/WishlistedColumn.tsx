import type { CSSProperties } from 'react'
import type { WishlistedPick } from '../../types'
import { cx } from '../../lib/format'
import { wishCover, wishMeta } from './text'
import { Cover } from '../Cover'

/** A wishlisted game you don't own in the lineup: marked Not
 *  owned, with the one thing to do about it, "Switch with owned game",
 *  which puts the owned game the pick chose with it in its place. */
export function WishlistedColumn({
  pick,
  number,
  busy,
  switching,
  onSwitch,
  className,
  style,
}: {
  pick: WishlistedPick
  number: number
  /** An action on the lineup is in flight. */
  busy: boolean
  /** This card's switch is in flight. */
  switching: boolean
  onSwitch: () => void
  className?: string
  style?: CSSProperties
}) {
  const g = pick.wishlist
  const titleId = `pick-wish-${g.id}-title`
  const meta = wishMeta(g)
  return (
    <article aria-labelledby={titleId} className={cx('fade-up flex min-w-0 flex-col gap-3', className)} style={style}>
      <div className="flex items-center justify-between">
        <div className="font-display text-[44px] leading-[0.9] font-extrabold text-accent" aria-hidden="true">
          {number}
        </div>
        <span className="flex min-h-11 items-center">
          <span className="rounded-full border border-line-strong px-3 py-1 text-[13px] font-semibold text-muted">Not owned</span>
        </span>
      </div>

      <Cover game={wishCover(g)} shape="wide" titleSize="text-[18px]" className={cx('w-4/5', switching && 'sifting opacity-70')} />

      <h2 id={titleId} className="mt-1.5 font-display text-[25px] leading-[1.15] font-bold tracking-[-0.015em]">
        {g.name}
      </h2>

      <div className="flex flex-col gap-1">
        {meta && <div className="font-mono text-[11px] leading-[1.6] text-dim">{meta}</div>}
        <div className="text-[13.5px] text-muted">{g.on_steam ? 'On your Steam wishlist' : 'On your wishlist'}</div>
      </div>

      <p className="mt-0.5 text-base leading-[1.6]">{pick.reason}</p>
      {g.short_description && <p className="text-[14.5px] leading-[1.6] text-muted">{g.short_description}</p>}

      {pick.alternate && (
        <div className="mt-1.5 flex flex-col items-start gap-2">
          <button type="button" className="btn-ink" disabled={busy} onClick={onSwitch}>
            {switching ? 'Switching…' : 'Switch with owned game'}
          </button>
          <p className="m-0 text-[13.5px] text-dim">You'd get {pick.alternate.name}, from your library.</p>
        </div>
      )}
    </article>
  )
}
