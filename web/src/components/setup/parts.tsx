import { useRef, useState, type ReactNode } from 'react'
import { cx } from '../../lib/format'
import type { SetupInfo, Settings } from '../../types'
import type { StepId } from '../../lib/setup'
import type { SaveFn } from '../settings/useSettingsSave'
import { TextButton } from '../settings/parts'

/** What every step gets. */
export interface StepProps {
  setup: SetupInfo
  s: Settings
  save: SaveFn
  go: (step: StepId) => void
  next: () => void
  back: () => void
}

/** A step's heading and why it matters. */
export function StepHead({ id, title, children }: { id: string; title: string; children?: ReactNode }) {
  return (
    <div className="flex flex-col gap-2">
      <h1
        id={id}
        tabIndex={-1}
        // Focused by script when the step changes; a ring around a heading helps nobody.
        style={{ outline: 'none' }}
        className="m-0 font-display text-[28px] leading-tight font-bold tracking-[-0.02em]"
      >
        {title}
      </h1>
      {children && <div className="flex max-w-[720px] flex-col gap-2 text-[15.5px] leading-[1.6] text-muted">{children}</div>}
    </div>
  )
}

/** Next, Back and Skip at the foot of a step. */
export function StepButtons({
  onNext,
  onBack,
  onSkip,
  next = 'Next',
  disabled,
}: {
  onNext: () => void
  onBack?: () => void
  onSkip?: () => void
  next?: string
  disabled?: boolean
}) {
  return (
    <div className="flex flex-wrap items-center gap-3 border-t border-line pt-5">
      <button type="button" className="btn-accent" onClick={onNext} disabled={disabled}>
        {next}
      </button>
      {onBack && <TextButton onClick={onBack}>Back</TextButton>}
      {onSkip && (
        <TextButton onClick={onSkip} className="ml-auto">
          Skip this step
        </TextButton>
      )}
    </div>
  )
}

/** A short line that something was found, or wasn't. */
export function Found({ ok, children }: { ok: boolean | null; children: ReactNode }) {
  const tone = ok === null ? 'text-dim' : ok ? 'text-good' : 'text-warn'
  return (
    <li className="flex items-baseline gap-2.5 text-[15px]">
      <span aria-hidden className={`relative top-[-1px] h-[7px] w-[7px] shrink-0 rounded-full bg-current ${tone}`} />
      <span className="min-w-0 text-ink">{children}</span>
    </li>
  )
}

/** Lines to copy, as they'd be typed, with a Copy button. Without a
 *  clipboard (an http:// address that isn't localhost) Copy selects them. */
export function CopyLines({ label, text }: { label: string; text: string }) {
  const block = useRef<HTMLPreElement>(null)
  // What was copied last: a change to the lines takes "Copied" away.
  const [copiedText, setCopiedText] = useState<string | null>(null)
  const copied = copiedText === text
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text)
      setCopiedText(text)
    } catch {
      const range = document.createRange()
      if (!block.current) return
      range.selectNodeContents(block.current)
      const selection = window.getSelection()
      selection?.removeAllRanges()
      selection?.addRange(range)
    }
  }
  return (
    <div className="flex min-w-0 flex-col gap-1.5">
      <div className="flex items-center gap-3">
        <span className="text-[13.5px] text-dim">{label}</span>
        <button type="button" className={cx('btn-outline ml-auto', copied && 'text-good')} onClick={() => void copy()} aria-label={`Copy: ${label}`}>
          {copied ? 'Copied' : 'Copy'}
        </button>
      </div>
      <pre
        ref={block}
        className="m-0 overflow-x-auto rounded-sm border border-line bg-raised p-3 font-mono text-[12.5px] leading-relaxed whitespace-pre text-ink"
      >
        {text}
      </pre>
    </div>
  )
}
