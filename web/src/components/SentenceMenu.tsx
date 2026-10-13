import { cx } from '../lib/format'
import { CheckIcon } from './pick/icons'
import { useMenu } from './useMenu'

export interface MenuOption {
  key: string
  label: string
  checked: boolean
  onSelect: () => void
}

/** An underlined word in a sentence that opens a small list of choices.
 *  `multi` keeps the list open after each choice (checkbox behaviour). */
export function SentenceMenu({
  label,
  title,
  options,
  multi = false,
  align = 'right',
  blank = false,
}: {
  label: string
  /** Accessible name for the control, e.g. "Number of picks". */
  title: string
  options: MenuOption[]
  multi?: boolean
  align?: 'left' | 'right'
  /** Nothing chosen yet: the word is a placeholder, drawn as a blank to fill in. */
  blank?: boolean
}) {
  const { open, close, wrapRef, buttonProps, menuProps } = useMenu<HTMLSpanElement>()
  return (
    <span ref={wrapRef} className="relative inline-block">
      <button {...buttonProps} className={cx('sentence-link -my-3 py-3', blank && 'sentence-blank')} aria-label={`${title}: ${blank ? 'not said' : label}`}>
        {label}
      </button>
      {open && (
        <div
          {...menuProps}
          aria-label={title}
          className={cx(
            'fade-up absolute top-full z-40 mt-3 min-w-44 rounded-[10px] border border-line-strong bg-sheet p-1 shadow-[0_12px_32px_rgb(0_0_0/0.35)]',
            align === 'right' ? 'right-0' : 'left-0',
          )}
        >
          {options.map((o) => (
            <button
              key={o.key}
              type="button"
              role={multi ? 'menuitemcheckbox' : 'menuitemradio'}
              aria-checked={o.checked}
              onClick={() => {
                o.onSelect()
                if (!multi) close()
              }}
              className={cx(
                'flex min-h-10 w-full items-center gap-2.5 rounded-[7px] px-3 text-left text-[14.5px] whitespace-nowrap hover:bg-raised focus-visible:bg-raised',
                o.checked ? 'text-ink' : 'text-muted',
              )}
            >
              <span className={cx('flex w-3.5 justify-center', o.checked ? 'text-accent' : 'text-transparent')}>
                <CheckIcon />
              </span>
              {o.label}
            </button>
          ))}
        </div>
      )}
    </span>
  )
}

/** A SentenceMenu that picks one value: `value` / `options` / `onChange`. */
export function SentenceChoice<T extends string>({
  value,
  options,
  onChange,
  label,
  align = 'left',
  placeholder,
}: {
  value: T
  options: { value: T; label: string }[]
  onChange: (v: T) => void
  /** What the control changes, for screen readers ("Which series"). */
  label: string
  align?: 'left' | 'right'
  /** Shown in the sentence, as a blank to fill in, while the value is ''. */
  placeholder?: string
}) {
  const current = options.find((o) => o.value === value) ?? options[0]
  const blank = placeholder != null && value === ''
  return (
    <SentenceMenu
      title={label}
      label={blank ? placeholder : current.label}
      blank={blank}
      align={align}
      options={options.map((o) => ({ key: o.value, label: o.label, checked: o.value === value, onSelect: () => onChange(o.value) }))}
    />
  )
}
