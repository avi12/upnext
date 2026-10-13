import { useEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from 'react'
import type { PadAction } from '../../lib/gamepad/pad'
import { lastInput } from '../../lib/gamepad/useGamepad'
import { cx } from '../../lib/format'
import { BigSheet } from './BigMenu'
import { Hint } from './Glyph'
import { useActions } from './pad'

const ROWS = ['1234567890', 'qwertyuiop', "asdfghjkl'", 'zxcvbnm,.-']

/** Big screen's own keyboard, for typing with a pad: letters, digits, space,
 *  delete and Done. X deletes and Y types a space from anywhere on it, and
 *  Back or Menu is Done. A touch or a real keyboard uses the system's own
 *  keyboard instead (see BigTextField). */
export function BigKeyboard({
  label,
  value,
  maxLength,
  onChange,
  onDone,
}: {
  label: string
  value: string
  maxLength?: number
  onChange: (v: string) => void
  onDone: () => void
}) {
  const [shift, setShift] = useState(false)
  const type = (s: string) => {
    if (maxLength == null || value.length + s.length <= maxLength) onChange(value + s)
    if (shift) setShift(false)
  }
  const del = () => onChange(value.slice(0, -1))
  const ref = useActions<HTMLDivElement>((a: PadAction) => {
    if (a === 'menu') del()
    else if (a === 'request') type(' ')
    else if (a === 'settings') onDone()
    else return false
    return true
  })
  const key = 'flex min-h-[3rem] min-w-[3rem] flex-1 items-center justify-center rounded-[0.6rem] bg-raised text-[1.25rem] font-semibold'

  return (
    <BigSheet title={label} onClose={onDone} place="bottom" className="max-w-[52rem]">
      <div ref={ref} className="flex flex-col gap-[0.75rem]">
        <output
          aria-live="polite"
          className="block min-h-[3rem] rounded-[0.7rem] border-[0.1rem] border-line-strong bg-raised px-[1rem] py-[0.6rem] text-[1.25rem] break-words"
        >
          {value || <span className="text-dim">Nothing typed yet</span>}
          <span aria-hidden="true" className="ml-[0.1rem] inline-block h-[1.2em] w-[0.1rem] translate-y-[0.2em] animate-pulse bg-accent" />
        </output>
        <div data-focus-group="keys" className="flex flex-col gap-[0.4rem]">
          {ROWS.map((row, r) => (
            <div key={row} className="flex gap-[0.4rem]">
              {row.split('').map((c, i) => {
                const ch = shift ? c.toUpperCase() : c
                return (
                  <button key={c} type="button" className={key} data-focus-first={r === 1 && i === 0 ? '' : undefined} onClick={() => type(ch)}>
                    {ch}
                  </button>
                )
              })}
            </div>
          ))}
          <div className="flex gap-[0.4rem]">
            <button type="button" className={cx(key, 'flex-[1.5]')} aria-pressed={shift} onClick={() => setShift((s) => !s)}>
              Shift
            </button>
            <button type="button" className={cx(key, 'flex-[4]')} onClick={() => type(' ')}>
              Space
            </button>
            <button type="button" className={cx(key, 'flex-[1.5]')} onClick={del}>
              Delete
            </button>
            <button type="button" className={cx(key, 'flex-[1.5] bg-accent text-accent-ink')} onClick={onDone}>
              Done
            </button>
          </div>
        </div>
        <div className="flex flex-wrap justify-end gap-x-[1.25rem] gap-y-[0.5rem]">
          <Hint action="menu">Delete</Hint>
          <Hint action="request">Space</Hint>
          <Hint action="back">Done</Hint>
        </div>
      </div>
    </BigSheet>
  )
}

/** A text field in big screen. Chosen with a pad (A), it opens BigKeyboard;
 *  touched or typed into, it is a plain field with the system's keyboard.
 *  Enter calls `onSubmit`; Done on the big keyboard only closes it, so the
 *  person sees what they typed before acting on it. */
export function BigTextField({
  id,
  label,
  value,
  onChange,
  onSubmit,
  placeholder,
  maxLength,
  className,
  type = 'text',
}: {
  id: string
  label: string
  value: string
  onChange: (v: string) => void
  onSubmit?: () => void
  placeholder?: string
  maxLength?: number
  className?: string
  type?: 'text' | 'search'
}) {
  const [typing, setTyping] = useState(false)
  const input = useRef<HTMLInputElement>(null)
  // A pad's A on the field arrives as a click (BigShell clicks the focused
  // element); a mouse or a finger clicks it too, but then lastInput() says so.
  useEffect(() => {
    const el = input.current
    if (!el) return
    const onClick = () => {
      if (lastInput() === 'gamepad') setTyping(true)
    }
    el.addEventListener('click', onClick)
    return () => el.removeEventListener('click', onClick)
  }, [])
  const onKeyDown = (e: ReactKeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter' && onSubmit) {
      e.preventDefault()
      onSubmit()
    }
  }
  return (
    <>
      <input
        ref={input}
        id={id}
        type={type}
        value={value}
        maxLength={maxLength}
        placeholder={placeholder}
        autoComplete="off"
        enterKeyHint="go"
        aria-label={label}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={onKeyDown}
        className={cx(
          'min-h-[3rem] min-w-0 rounded-[0.7rem] border-[0.1rem] border-line-strong bg-raised px-[1rem] text-[1.1rem] text-ink outline-none placeholder:text-dim',
          className,
        )}
      />
      {typing && (
        <BigKeyboard
          label={label}
          value={value}
          maxLength={maxLength}
          onChange={onChange}
          onDone={() => setTyping(false)}
        />
      )}
    </>
  )
}
