import { useEffect, useId, useRef, type ReactNode } from 'react'
import { focusElement, focusFirst } from '../../lib/focus/engine'
import { useBack } from '../../lib/focus/back'
import { cx } from '../../lib/format'
import { Hint } from './Glyph'

export interface MenuOption {
  key: string
  label: string
  /** One line under the label. */
  hint?: string
  /** Shown as chosen (a mark already set, a filter on). */
  checked?: boolean
  disabled?: boolean
  onSelect: () => void
}

/** A sheet over the page that holds the focus until it closes, then hands
 *  it back to what opened it. Back closes it first. */
export function BigSheet({
  title,
  onClose,
  children,
  className,
  place = 'center',
}: {
  title: string
  onClose: () => void
  children: ReactNode
  className?: string
  place?: 'center' | 'bottom'
}) {
  const titleId = useId()
  const root = useRef<HTMLDivElement>(null)
  const opener = useRef<HTMLElement | null>(null)
  useBack(true, onClose)
  useEffect(() => {
    opener.current = document.activeElement as HTMLElement | null
    if (root.current) focusFirst(root.current)
    return () => {
      const back = opener.current
      if (back?.isConnected) focusElement(back)
    }
  }, [])
  return (
    <div className={cx('fixed inset-x-0 top-(--titlebar-h) bottom-0 z-50 flex justify-center bg-[var(--backdrop)] p-[1.5rem]', place === 'center' ? 'items-center' : 'items-end')}>
      <div
        ref={root}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        data-focus-scope
        className={cx('fade-up flex max-h-full w-full max-w-[34rem] flex-col gap-[1rem] overflow-y-auto rounded-[1.2rem] border-[0.08rem] border-line-strong bg-sheet p-[1.5rem] shadow-2xl', className)}
      >
        <h2 id={titleId} className="m-0 font-display text-[1.5rem] leading-tight font-bold">
          {title}
        </h2>
        {children}
      </div>
    </div>
  )
}

/** A short list of choices in a sheet: a card's menu, Mark as, a rating. */
export function BigMenu({ title, options, onClose }: { title: string; options: MenuOption[]; onClose: () => void }) {
  return (
    <BigSheet title={title} onClose={onClose}>
      <ul className="m-0 flex list-none flex-col gap-[0.5rem] p-0">
        {options.map((o) => (
          <li key={o.key}>
            <button
              type="button"
              disabled={o.disabled}
              aria-pressed={o.checked ?? undefined}
              onClick={() => {
                onClose()
                o.onSelect()
              }}
              className="flex min-h-[3rem] w-full flex-col items-start justify-center rounded-[0.7rem] bg-raised px-[1.1rem] py-[0.5rem] text-left text-[1.1rem] disabled:opacity-50 aria-pressed:font-bold aria-pressed:text-accent"
            >
              <span>
                {o.checked && <span aria-hidden="true">✓ </span>}
                {o.label}
              </span>
              {o.hint && <span className="text-[0.85rem] text-dim">{o.hint}</span>}
            </button>
          </li>
        ))}
      </ul>
      <div className="flex justify-end">
        <Hint action="back">Close</Hint>
      </div>
    </BigSheet>
  )
}
