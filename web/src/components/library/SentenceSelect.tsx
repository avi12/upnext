import { cx } from '../../lib/format'

export interface SentenceOption {
  value: string
  /** Text in the open menu (may carry a count). */
  label: string
}

/**
 * An underlined word in a filter sentence that is really a native <select>.
 * The visible word is plain text; a transparent select sits on top of it, so
 * the width follows the word and keyboard/screen-reader use stays native.
 */
export function SentenceSelect({
  display,
  value,
  options,
  onChange,
  label,
}: {
  display: string
  value: string
  options: SentenceOption[]
  onChange: (v: string) => void
  /** Accessible name, e.g. "Status". */
  label: string
}) {
  return (
    <span className="relative inline-block rounded-[3px] outline-offset-4 has-[select:focus-visible]:outline-2 has-[select:focus-visible]:outline-accent">
      <span aria-hidden="true" className="sentence-link whitespace-nowrap">
        {display}
      </span>
      <select
        aria-label={label}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className={cx(
          'absolute -inset-x-1 -top-2 -bottom-2 w-[calc(100%+0.5rem)] cursor-pointer appearance-none opacity-0',
          'text-[15px]',
        )}
      >
        {options.map((o) => (
          <option key={o.value} value={o.value} className="bg-raised text-ink">
            {o.label}
          </option>
        ))}
      </select>
    </span>
  )
}
