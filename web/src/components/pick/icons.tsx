import { cx } from '../../lib/format'

/** Small inline icons from the design boards. All decorative (aria-hidden). */

/** Three cards in rank order: picks are chosen, not rolled. While
 *  `busy` they trade heights, as if being sorted into a lineup; the
 *  reduced-motion rule in index.css keeps them still. */
export function CurateIcon({ className = 'h-5 w-5', busy = false }: { className?: string; busy?: boolean }) {
  const card = (x: number, scale: number) => (
    <rect
      x={x}
      y="4"
      width="4.5"
      height="16"
      rx="1.2"
      fill="currentColor"
      style={{ transformBox: 'fill-box', transformOrigin: 'bottom', transform: `scaleY(${scale})` }}
    />
  )
  return (
    <svg viewBox="0 0 24 24" className={cx(className, busy && 'curating')} aria-hidden="true">
      {card(3.5, 0.5)}
      {card(9.75, 0.75)}
      {card(16, 1)}
    </svg>
  )
}

/** The dice: Roll the dice beside Curate, and Another on its card. */
export function DiceIcon({ className = 'h-5 w-5' }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" className={className} fill="none" stroke="currentColor" strokeWidth="2" strokeLinejoin="round" aria-hidden="true">
      <rect x="3.5" y="3.5" width="17" height="17" rx="4" />
      <circle cx="8.5" cy="8.5" r="1.3" fill="currentColor" />
      <circle cx="15.5" cy="15.5" r="1.3" fill="currentColor" />
      <circle cx="12" cy="12" r="1.3" fill="currentColor" />
    </svg>
  )
}

export function PlayIcon({ className = 'h-3 w-3' }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" className={className} aria-hidden="true">
      <path d="M7 4.5c0-1.1 1.2-1.8 2.2-1.2l10.3 6.7c.9.6.9 1.9 0 2.5L9.2 19.2c-1 .6-2.2-.1-2.2-1.2z" fill="currentColor" />
    </svg>
  )
}

export function TrailerIcon({ className = 'h-[15px] w-[15px]' }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" className={className} fill="none" stroke="currentColor" strokeWidth="2" strokeLinejoin="round" aria-hidden="true">
      <rect x="2.5" y="5" width="19" height="14" rx="4" />
      <path d="M10 9.5v5l4.5-2.5z" fill="currentColor" />
    </svg>
  )
}

export function BookmarkIcon({ className = 'h-3.5 w-3.5', filled = false }: { className?: string; filled?: boolean }) {
  return (
    <svg viewBox="0 0 24 24" className={className} fill={filled ? 'currentColor' : 'none'} stroke="currentColor" strokeWidth="2" strokeLinejoin="round" aria-hidden="true">
      <path d="M6 3.5h12v17l-6-4-6 4z" />
    </svg>
  )
}

export function CheckIcon({ className = 'h-3.5 w-3.5' }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" className={className} fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M5 12.5l4.5 4.5L19 7.5" />
    </svg>
  )
}
