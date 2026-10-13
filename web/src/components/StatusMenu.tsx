import { useEffect, useRef, useState, type CSSProperties, type KeyboardEvent } from 'react'
import { useQuery } from '@tanstack/react-query'
import { api } from '../api'
import type { GameView, HideScope, OverrideStatus, StopReason } from '../types'
import { DEFAULT_LINES, OVERRIDES, overrideMeta, sinceText } from '../lib/statuses'
import { cx } from '../lib/format'
import { useMenu } from './useMenu'
import { HideStep } from './HideStep'
import type { Hideable } from './useHideGame'
import { hiddenText } from '../lib/players'
import { useSettings } from '../lib/queries'

/** The panel's width on a wide screen (its class says the same). */
const PANEL_WIDTH = 320

/** Why a game was put down, as sent with Shelved, Abandoned or Burnt out. */
export interface Reasons {
  codes: string[]
  note?: string
}

/** "Mark as…" for a game: a clear button that opens the statuses for its
 *  kind (Beat it, Shelved, Abandoned, Not for me; or for an endless game
 *  Still playing, Burnt out, Not for me). Shelved, Abandoned and Burnt out
 *  then ask why it was put down, in a step you can skip. When a status is
 *  set it shows the status and when, with Change / Clear. */
export function StatusMenu({
  game,
  onChange,
  busy = false,
  size = 'md',
  onEndless,
  hide,
  onHidden,
}: {
  game: Pick<GameView, 'override' | 'override_at' | 'kind'>
  onChange: (status: OverrideStatus | null, reasons?: Reasons) => void
  busy?: boolean
  size?: 'sm' | 'md'
  /** "Can be played endlessly", for a game UpNext takes to have an ending. */
  onEndless?: () => void
  /** "Hide this game" at the end of the menu, for this game. */
  hide?: Hideable
  /** After it was hidden (a scope) or shown again (null). */
  onHidden?: (scope: HideScope | null) => void
}) {
  const { open, setOpen, close, wrapRef, buttonProps, menuProps } = useMenu<HTMLDivElement>()
  // Where the panel starts, from the button's left edge: under the button,
  // moved left as far as it takes to stay on screen (the sheet puts the
  // button near its right edge, and a phone is narrower than the panel).
  const [offset, setOffset] = useState(0)
  // The status picked while its "why" step is showing.
  const [asking, setAsking] = useState<OverrideStatus | null>(null)
  // The "Hide this game" step is showing.
  const [hiding, setHiding] = useState(false)
  const current = overrideMeta(game.override)
  const settings = useSettings({ enabled: open })
  const lines = settings.data ?? DEFAULT_LINES
  const items = OVERRIDES.filter((o) => o.kinds.includes(game.kind ?? 'ending'))

  const toggle = () => {
    const r = wrapRef.current?.getBoundingClientRect()
    if (r) {
      // Without the scrollbar, unlike innerWidth.
      const room = document.documentElement.clientWidth
      const width = Math.min(PANEL_WIDTH, room - 32)
      setOffset(Math.max(16, Math.min(r.left, room - 16 - width)) - r.left)
    }
    setAsking(null)
    setHiding(false)
    setOpen((o) => !o)
  }

  const pick = (v: OverrideStatus | null, reasons?: Reasons) => {
    setAsking(null)
    close()
    onChange(v, reasons)
  }

  // One button either way, "Mark as…" or "Change": two would swap when a
  // status is picked, and the focus the menu gives back would go with the
  // one that left, to the page's body.
  const trigger = (
    <span className={current ? cx('inline-flex flex-wrap items-center gap-x-3 gap-y-1', size === 'sm' ? 'text-[13.5px]' : 'text-[14.5px]') : 'contents'}>
      {current && (
        <span className="inline-flex items-center gap-2 font-semibold text-ink">
          <span aria-hidden="true" className={cx('h-[7px] w-[7px] rounded-full', current.dot)} />
          {current.label}
          {sinceText(game.override_at) && <span className="font-normal text-dim">{sinceText(game.override_at)}</span>}
        </span>
      )}
      <button
        {...buttonProps}
        onClick={toggle}
        disabled={busy}
        className={
          current ? 'btn-quiet underline decoration-underline underline-offset-4' : cx('btn-outline', size === 'sm' ? 'h-9 px-3 text-[13.5px]' : 'h-10')
        }
      >
        {current ? (
          'Change'
        ) : (
          <>
            Mark as…
            <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" aria-hidden="true">
              <path d="M6 9l6 6 6-6" />
            </svg>
          </>
        )}
      </button>
    </span>
  )

  const panel = 'fade-up absolute top-full z-50 mt-2 w-[min(320px,calc(100vw-32px))] rounded-xl border border-line-strong bg-sheet p-1.5 shadow-2xl'
  const place = { left: offset }

  return (
    <div ref={wrapRef} className="relative inline-flex">
      {trigger}
      {open && asking && (
        <WhyStep
          status={asking}
          className={panel}
          style={place}
          onBack={() => setAsking(null)}
          onCancel={() => {
            setAsking(null)
            close()
          }}
          onDone={(reasons) => pick(asking, reasons)}
        />
      )}
      {open && hiding && hide && (
        <HideStep
          game={hide}
          className={panel}
          style={place}
          onBack={() => setHiding(false)}
          onCancel={() => {
            setHiding(false)
            close()
          }}
          onHidden={onHidden}
          onDone={() => {
            setHiding(false)
            close()
          }}
        />
      )}
      {open && !asking && !hiding && (
        <div {...menuProps} aria-label="Mark this game as" className={panel} style={place}>
          {items.map((o) => (
            <button
              key={o.value}
              type="button"
              role="menuitemradio"
              aria-checked={game.override === o.value}
              onClick={() => (o.asksWhy ? setAsking(o.value) : pick(o.value))}
              className={cx(
                'flex w-full items-start gap-3 rounded-lg px-3 py-2.5 text-left hover:bg-raised focus:bg-raised focus:outline-none',
                game.override === o.value && 'bg-raised',
              )}
            >
              <span aria-hidden="true" className={cx('mt-[7px] h-[7px] w-[7px] shrink-0 rounded-full', o.dot)} />
              <span className="min-w-0">
                <span className="block text-[14.5px] font-semibold text-ink">{o.label}</span>
                <span className="block text-[13px] leading-snug text-muted">{o.hint(lines)}</span>
              </span>
            </button>
          ))}
          {onEndless && game.kind !== 'endless' && (
            <button
              type="button"
              role="menuitem"
              onClick={() => {
                close()
                onEndless()
              }}
              className="mt-1 flex w-full flex-col items-start rounded-lg border-t border-line px-3 py-2.5 text-left hover:bg-raised focus:bg-raised focus:outline-none"
            >
              <span className="text-[14px] text-ink">Can be played endlessly</span>
              <span className="text-[12.5px] leading-snug text-dim">No ending to reach, like a sandbox or an online game. Picks treat it as one.</span>
            </button>
          )}
          {current && (
            <button
              type="button"
              role="menuitem"
              onClick={() => pick(null)}
              className="mt-1 flex w-full flex-col items-start rounded-lg border-t border-line px-3 py-2.5 text-left hover:bg-raised focus:bg-raised focus:outline-none"
            >
              <span className="text-[14px] text-muted">Clear status</span>
              <span className="text-[12.5px] leading-snug text-dim">UpNext works it out from your playtime again.</span>
            </button>
          )}
          {hide && (
            <button
              type="button"
              role="menuitem"
              onClick={() => setHiding(true)}
              className="mt-1 flex w-full flex-col items-start rounded-lg border-t border-line px-3 py-2.5 text-left hover:bg-raised focus:bg-raised focus:outline-none"
            >
              <span className="text-[14px] text-ink">{hide.hidden_scope ? hiddenText(hide.hidden_scope) : 'Hide this game…'}</span>
              <span className="text-[12.5px] leading-snug text-dim">
                {hide.hidden_scope ? 'Change who it’s hidden from, or unhide it.' : 'Out of sight; it says nothing about your taste.'}
              </span>
            </button>
          )}
        </div>
      )}
    </div>
  )
}

/** The optional step after Shelved, Abandoned or Burnt out: why it was put
 *  down, any mix of life and the game, and a line of your own. */
export function WhyStep({
  status,
  className,
  style,
  onBack,
  onCancel,
  onDone,
}: {
  status: OverrideStatus
  className: string
  style: CSSProperties
  onBack: () => void
  onCancel: () => void
  onDone: (reasons?: Reasons) => void
}) {
  const reasons = useQuery({ queryKey: ['stop-reasons'], queryFn: api.stopReasons, staleTime: Infinity })
  const [codes, setCodes] = useState<string[]>([])
  const [note, setNote] = useState('')
  const ref = useRef<HTMLDivElement>(null)
  const label = overrideMeta(status)?.label ?? status

  useEffect(() => {
    ref.current?.querySelector<HTMLElement>('[data-first]')?.focus()
  }, [reasons.data])

  const flip = (code: string) => setCodes((c) => (c.includes(code) ? c.filter((x) => x !== code) : [...c, code]))
  const chosen = codes.length > 0 || note.trim() !== ''
  const save = () => chosen && onDone({ codes, note: note.trim() || undefined })

  const onKeyDown = (e: KeyboardEvent) => {
    if (e.key === 'Escape') {
      e.preventDefault()
      // Don't let Esc also close the sheet underneath.
      e.stopPropagation()
      e.nativeEvent.stopImmediatePropagation()
      onCancel()
    }
  }

  const group = (g: StopReason['group'], title: string, list: StopReason[]) => (
    <fieldset className="m-0 border-0 p-0">
      <legend className="mb-1.5 p-0 font-mono text-[10.5px] tracking-[0.08em] text-dim uppercase">{title}</legend>
      <div className="flex flex-wrap gap-1.5">
        {list
          .filter((r) => r.group === g)
          .map((r, i) => (
            <button
              key={r.code}
              type="button"
              aria-pressed={codes.includes(r.code)}
              data-first={g === 'life' && i === 0 ? '' : undefined}
              onClick={() => flip(r.code)}
              className={cx(
                'min-h-8 rounded-full border px-3 text-[13px] leading-tight',
                codes.includes(r.code) ? 'border-accent bg-accent text-accent-ink' : 'border-line-strong text-ink hover:bg-raised',
              )}
            >
              {r.label}
            </button>
          ))}
      </div>
    </fieldset>
  )

  return (
    <div ref={ref} role="dialog" aria-label={`Why you ${status === 'burnt_out' ? 'burnt out' : 'put it down'}`} onKeyDown={onKeyDown} className={className} style={style}>
      <div className="flex flex-col gap-3.5 px-3 py-2.5">
        <div>
          <button type="button" onClick={onBack} className="btn-quiet -ml-1 px-1 text-[13px] text-dim hover:text-ink">
            ← {label}
          </button>
          <p className="m-0 mt-1 text-[14.5px] font-semibold text-ink">Why did you stop?</p>
          <p className="m-0 text-[13px] leading-snug text-muted">Pick any that fit, or skip. UpNext uses them when it offers the game again.</p>
        </div>
        {reasons.data && group('life', 'Life', reasons.data)}
        {reasons.data && group('game', 'The game', reasons.data)}
        {reasons.isError && <p className="m-0 text-[13px] text-dim">Couldn't load the reasons; a line of your own still works.</p>}
        <input
          type="text"
          value={note}
          maxLength={500}
          onChange={(e) => setNote(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && save()}
          placeholder="In your own words (optional)"
          aria-label="Why you stopped, in your own words"
          data-first={reasons.data ? undefined : ''}
          className="h-10 w-full rounded-lg border border-line-strong bg-ground px-3 text-[14px] text-ink placeholder:text-dim"
        />
        <div className="flex items-center justify-end gap-2">
          <button type="button" onClick={() => onDone()} className="btn-quiet h-10 px-3 text-[14px]">
            Skip
          </button>
          <button type="button" onClick={save} disabled={!chosen} className="btn-accent h-10 rounded-lg px-4 text-[14px] disabled:opacity-50">
            Save
          </button>
        </div>
      </div>
    </div>
  )
}
