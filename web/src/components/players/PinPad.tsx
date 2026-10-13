import { useEffect, useId, useRef, useState } from 'react'
import { cx } from '../../lib/format'
import { pinOk } from '../../lib/players'

const KEYS = ['1', '2', '3', '4', '5', '6', '7', '8', '9', 'back', '0', 'ok'] as const

/** A PIN: typed, or tapped on the pad (a phone, a controller in big screen).
 *  Four to eight digits; OK sends it. A wrong one clears the field and shows
 *  the server's reason under it. */
export function PinPad({
  label,
  busy = false,
  error,
  submitLabel = 'OK',
  onSubmit,
}: {
  label: string
  busy?: boolean
  /** Why the last one was refused. */
  error?: string | null
  submitLabel?: string
  onSubmit: (pin: string) => void
}) {
  const [pin, setPin] = useState('')
  const field = useRef<HTMLInputElement>(null)
  const errorId = useId()
  useEffect(() => {
    field.current?.focus()
  }, [])
  // Sent, the field empties: a refused PIN is typed again from the start.
  const send = () => {
    if (!pinOk(pin) || busy) return
    onSubmit(pin)
    setPin('')
  }
  const press = (k: (typeof KEYS)[number]) => {
    if (k === 'back') setPin((p) => p.slice(0, -1))
    else if (k === 'ok') send()
    else setPin((p) => (p.length < 8 ? p + k : p))
    field.current?.focus()
  }
  return (
    <form
      className="flex flex-col items-center gap-3"
      onSubmit={(e) => {
        e.preventDefault()
        send()
      }}
    >
      <input
        ref={field}
        type="password"
        inputMode="numeric"
        autoComplete="off"
        aria-label={label}
        aria-invalid={error ? true : undefined}
        aria-describedby={error ? errorId : undefined}
        maxLength={8}
        value={pin}
        disabled={busy}
        onChange={(e) => setPin(e.target.value.replace(/\D/g, '').slice(0, 8))}
        className="field h-12 w-[12rem] text-center font-mono text-[22px] tracking-[0.4em]"
      />
      {error && (
        <p id={errorId} role="alert" className="m-0 max-w-[18rem] text-center text-[13.5px] text-bad">
          {error}
        </p>
      )}
      <div className="grid grid-cols-3 gap-2" data-focus-group>
        {KEYS.map((k) => (
          <button
            key={k}
            type="button"
            disabled={busy || (k === 'ok' && !pinOk(pin))}
            onClick={() => press(k)}
            aria-label={k === 'back' ? 'Delete a digit' : k === 'ok' ? submitLabel : k}
            className={cx(
              'flex h-12 w-16 items-center justify-center rounded-xl border text-[18px] disabled:opacity-40',
              k === 'ok' ? 'border-accent bg-accent font-semibold text-accent-ink text-[15px]' : 'border-line-strong text-ink hover:bg-raised',
            )}
          >
            {k === 'back' ? '⌫' : k === 'ok' ? submitLabel : k}
          </button>
        ))}
      </div>
    </form>
  )
}
