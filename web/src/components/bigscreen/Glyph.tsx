import { GLYPHS, KEYS, type PadAction } from '../../lib/gamepad/pad'
import { cx } from '../../lib/format'
import { usePadStyle } from './pad'

/** The button (or, with no pad used yet, the key) that does `action`, as a
 *  small key cap: "A", "✕", "LB", "Esc". */
export function Glyph({ action, className }: { action: PadAction; className?: string }) {
  const style = usePadStyle()
  const label = style ? GLYPHS[style][action] : KEYS[action]
  return (
    <kbd
      className={cx(
        'inline-flex min-w-[1.6em] items-center justify-center rounded-[0.4rem] border-[0.08rem] border-key bg-raised-2 px-[0.35em] font-sans text-[0.85rem] leading-[1.5] font-bold text-ink',
        className,
      )}
    >
      {label}
    </kbd>
  )
}

/** A glyph and what it does, for the hints along the bottom. */
export function Hint({ action, children }: { action: PadAction; children: string }) {
  return (
    <span className="inline-flex items-center gap-[0.4rem] text-[0.9rem] text-muted">
      <Glyph action={action} />
      {children}
    </span>
  )
}
