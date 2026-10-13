import { Children, useId, useRef, useState, type InputHTMLAttributes, type KeyboardEvent, type ReactNode, type Ref } from 'react'
import { cx } from '../../lib/format'

/** A settings section: heading with a one-line description beside it. */
export function Section({ id, title, desc, children }: { id: string; title: string; desc?: ReactNode; children: ReactNode }) {
  return (
    <section id={id} aria-labelledby={`h-${id}`} className="scroll-mt-24">
      <div className="flex flex-wrap items-baseline gap-x-4 gap-y-1 border-b border-line pb-3">
        <h2 id={`h-${id}`} className="m-0 font-display text-[21px] font-[750] tracking-[-0.02em]">
          {title}
        </h2>
        {desc && <p className="m-0 text-sm text-dim">{desc}</p>}
      </div>
      {children}
    </section>
  )
}

/** Two columns of rows from 1024px up, one below. */
export function RowGrid({ children }: { children: ReactNode }) {
  return <div className="grid grid-cols-1 lg:grid-cols-2 lg:gap-x-14">{children}</div>
}

/** One tight setting: bold name and a short muted line on the left, controls on the right. */
export function Row({
  name,
  tag,
  desc,
  right,
  children,
  className,
  wrap = false,
}: {
  name: ReactNode
  tag?: string
  desc?: ReactNode
  right?: ReactNode
  /** Expanded content under the row (Change, options, ...). */
  children?: ReactNode
  className?: string
  /** The short line wraps instead of being cut off. */
  wrap?: boolean
}) {
  return (
    <div className={cx('min-w-0 border-b border-line py-3.5', className)}>
      {/* The name and its short line never give way to the right side: the
          left side asks for the name's width, and 10rem at least, since the
          short line adds no width of its own; when the right side doesn't
          fit beside that, it takes a line of its own, and a name wider than
          a phone wraps. A long status ran over the name before (Windows
          retest). */}
      <div className="flex min-h-11 flex-wrap items-center gap-x-4 gap-y-1">
        <div className="min-w-[min(100%,10rem)] flex-[1_1_auto]">
          <div className="flex items-baseline gap-2.5">
            <span className="text-[15.5px] font-bold">{name}</span>
            {tag && <span className="text-[12.5px] text-dim">{tag}</span>}
          </div>
          {desc && <div className={cx('mt-0.5 w-0 min-w-full text-[13.5px] text-dim', !wrap && 'truncate')}>{desc}</div>}
        </div>
        {right && <div className="ml-auto flex min-w-0 max-w-full items-center gap-4">{right}</div>}
      </div>
      {/* Counted, so children that are all closed (false) add no space. */}
      {Children.toArray(children).length > 0 && <div className="pt-3">{children}</div>}
    </div>
  )
}

/** Status as a dot and a word. Dim statuses skip the dot. */
export function Status({ tone, children, title }: { tone: 'good' | 'warn' | 'bad' | 'dim'; children: ReactNode; title?: string }) {
  const color = { good: 'text-good', warn: 'text-warn', bad: 'text-bad', dim: 'text-dim' }[tone]
  return (
    <span className={cx('inline-flex min-w-0 items-center gap-2 text-sm whitespace-nowrap', color)} title={title}>
      {tone !== 'dim' && <span aria-hidden className="h-[7px] w-[7px] shrink-0 rounded-full bg-current" />}
      {children}
    </span>
  )
}

/** A text-style action (Change, Replace, Log). 44px tall for easy clicking. */
export function TextButton({
  children,
  className,
  ...rest
}: { children: ReactNode } & React.ButtonHTMLAttributes<HTMLButtonElement>) {
  return (
    <button
      type="button"
      className={cx(
        'inline-flex h-11 shrink-0 items-center px-1 text-sm whitespace-nowrap text-muted underline decoration-underline underline-offset-4 hover:text-ink hover:decoration-accent disabled:pointer-events-none disabled:opacity-50',
        className,
      )}
      {...rest}
    >
      {children}
    </button>
  )
}

export function Segmented<T extends string>({
  label,
  options,
  value,
  onChange,
  disabled,
  className,
}: {
  label: string
  options: { value: T; label: ReactNode }[]
  value: T
  onChange: (v: T) => void
  disabled?: boolean
  className?: string
}) {
  const refs = useRef<(HTMLButtonElement | null)[]>([])
  const onKey = (e: KeyboardEvent<HTMLButtonElement>, i: number) => {
    const delta = e.key === 'ArrowRight' || e.key === 'ArrowDown' ? 1 : e.key === 'ArrowLeft' || e.key === 'ArrowUp' ? -1 : 0
    if (!delta) return
    e.preventDefault()
    const next = (i + delta + options.length) % options.length
    refs.current[next]?.focus()
    onChange(options[next].value)
  }
  return (
    <div role="radiogroup" aria-label={label} className={cx('seg shrink-0', className)}>
      {options.map((o, i) => {
        const on = o.value === value
        return (
          <button
            key={o.value}
            ref={(el) => {
              refs.current[i] = el
            }}
            type="button"
            role="radio"
            aria-checked={on}
            tabIndex={on ? 0 : -1}
            disabled={disabled}
            onClick={() => !on && onChange(o.value)}
            onKeyDown={(e) => onKey(e, i)}
            className={cx('seg-item', on && 'seg-item-on')}
          >
            {o.label}
          </button>
        )
      })}
    </div>
  )
}

export function Checkbox({
  checked,
  onChange,
  disabled,
  children,
  note,
}: {
  checked: boolean
  onChange: (v: boolean) => void
  disabled?: boolean
  children: ReactNode
  note?: ReactNode
}) {
  return (
    <label className={cx('flex min-h-9 cursor-pointer items-center gap-2.5 text-sm', disabled && 'cursor-default text-dim')}>
      <input
        type="checkbox"
        className="h-[17px] w-[17px] shrink-0 accent-accent"
        checked={checked}
        disabled={disabled}
        onChange={(e) => onChange(e.target.checked)}
      />
      <span>
        {children}
        {note && <span className="ml-1.5 text-[12.5px] text-dim">{note}</span>}
      </span>
    </label>
  )
}

type FieldProps = Omit<InputHTMLAttributes<HTMLInputElement>, 'value' | 'onChange' | 'defaultValue' | 'type'> & {
  /** Current saved value ('' for write-only secrets). */
  value: string
  /** Save; resolve to an error message or null. */
  onCommit: (v: string) => Promise<string | null>
  /** Write-only: never shows the saved value; an empty field means "leave it". */
  secret?: boolean
  label?: ReactNode
  /** Classes for the outer wrapper (label + input + error). */
  wrapClassName?: string
  after?: ReactNode
  ref?: Ref<HTMLInputElement>
}

/** A text field that saves on Enter or blur. Escape puts the saved value back.
 *  A rejected value stays in the field with the server's message under it. */
export function InlineField({ value, onCommit, secret, label, className, wrapClassName, after, ...rest }: FieldProps) {
  const [draft, setDraft] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const busy = useRef(false)
  const errId = useId()

  const commit = async () => {
    if (draft === null || busy.current) return
    const v = draft.trim()
    if (secret ? v === '' : v === value.trim()) {
      setDraft(null)
      setError(null)
      return
    }
    busy.current = true
    const err = await onCommit(v)
    busy.current = false
    setError(err)
    if (!err) setDraft(null)
  }

  return (
    <div className={cx('flex min-w-0 flex-col gap-1', wrapClassName)}>
      <label className="flex min-w-0 items-center gap-2 text-[13.5px] text-dim">
        {label}
        <input
          {...rest}
          type={secret ? 'password' : 'text'}
          autoComplete="off"
          spellCheck={false}
          value={draft ?? value}
          aria-invalid={error ? true : undefined}
          aria-describedby={error ? errId : undefined}
          onChange={(e) => {
            setDraft(e.target.value)
          }}
          onBlur={() => void commit()}
          onKeyDown={(e) => {
            if (e.key === 'Enter') {
              e.preventDefault()
              void commit()
            } else if (e.key === 'Escape') {
              setDraft(null)
              setError(null)
            }
          }}
          className={cx('field min-w-0 text-sm', error && 'border-bad', className)}
        />
        {after}
      </label>
      {error && (
        <p id={errId} role="alert" className="m-0 text-[12.5px] text-bad">
          {error}
        </p>
      )}
    </div>
  )
}

/** An external link in help text. */
export function Ext({ href, children }: { href: string; children: ReactNode }) {
  return (
    <a href={href} target="_blank" rel="noreferrer" className="text-muted underline decoration-underline underline-offset-2 hover:text-ink">
      {children}
    </a>
  )
}
