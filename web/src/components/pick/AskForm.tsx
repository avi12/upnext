import { useRef } from 'react'
import { Link } from 'react-router'
import type { Config, Source } from '../../types'
import { SOURCES, cx, storeName } from '../../lib/format'
import { BACKEND_KEY, BACKEND_LABEL } from '../../lib/ai'
import { CodeText } from '../CodeText'
import { SentenceMenu } from '../SentenceMenu'
import { TuneNudge } from '../questions/TuneNudge'
import { CurateIcon, DiceIcon } from './icons'
import { storesLabel } from './text'

/** The longest request, in characters, that the server takes. */
const MAX_PROMPT = 2000

const QUICK = ['A quiet hour', 'A weekend story', 'Something I can actually finish', 'Co-op night', 'New to me', 'A hidden gem', 'Surprise me']

/** The top of the Pick page: what you're in the mood for, Curate, the dice
 *  (one game off your radar, at random), the "Or try" suggestions, how
 *  many picks from which stores, and a note when no AI is set up. */
export function AskForm({
  prompt,
  onDraft,
  onRoll,
  onDice,
  pending,
  dicing = false,
  count,
  setCount,
  sources,
  toggleSource,
  config,
}: {
  prompt: string
  onDraft: (text: string) => void
  onRoll: () => void
  /** Roll the dice: one game off your radar, drawn at random with no AI. */
  onDice: () => void
  /** Curating or rolling the dice. */
  pending: boolean
  /** Rolling the dice, so Curate isn't the one working. */
  dicing?: boolean
  count: number
  setCount: (n: number) => void
  sources: Source[]
  toggleSource: (s: Source) => void
  config: Config | undefined
}) {
  const inputRef = useRef<HTMLInputElement>(null)
  return (
    <section data-tour="ask" className="flex flex-col gap-4 pt-10">
      <form
        className="flex flex-col gap-4"
        onSubmit={(e) => {
          e.preventDefault()
          onRoll()
        }}
      >
        <label htmlFor="ask" className="text-[15px] text-muted">
          What are you in the mood for?
        </label>
        <div className="flex flex-col gap-3 sm:flex-row">
          <input
            id="ask"
            ref={inputRef}
            type="text"
            value={prompt}
            // The server refuses a longer request.
            maxLength={MAX_PROMPT}
            onChange={(e) => onDraft(e.target.value)}
            onKeyDown={(e) => {
              if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
                e.preventDefault()
                onRoll()
              }
            }}
            placeholder="About 2 hours, something atmospheric with a good story…"
            autoComplete="off"
            // Stacked on a phone, flex-1 would size it to its text (25 px), so
            // it only grows beside the buttons.
            className="h-[60px] min-w-0 rounded-xl border border-line-strong bg-raised px-5 sm:flex-1 text-[19px] text-ink outline-none placeholder:text-dim focus:border-accent"
          />
          <div className="flex gap-3">
            <button
              type="submit"
              disabled={pending}
              className="btn-accent h-[60px] flex-1 gap-2.5 rounded-xl px-6 text-[17px] sm:flex-none"
              title="Curate (Ctrl+Enter)"
            >
              <CurateIcon busy={pending && !dicing} />
              {pending && !dicing ? 'Curating…' : 'Curate'}
            </button>
            {/* Beside Curate: chance instead of fit, so it
                ignores the request and is instant. */}
            <button
              type="button"
              onClick={onDice}
              disabled={pending}
              className="btn-outline h-[60px] w-[60px] shrink-0 justify-center rounded-xl px-0"
              aria-label="Roll the dice"
              title="Roll the dice: one game you haven't started or haven't played in a year, at random, with no AI"
            >
              <DiceIcon className={cx('h-6 w-6', dicing && 'dicing')} />
            </button>
          </div>
        </div>
      </form>

      <TuneNudge />

      <div className="flex flex-col gap-3 text-[14.5px] text-dim lg:flex-row lg:items-baseline lg:justify-between">
        <div className="flex flex-wrap items-baseline gap-x-3.5 gap-y-1.5">
          <span>Or try:</span>
          {QUICK.map((q) => (
            <button
              key={q}
              type="button"
              onClick={() => {
                onDraft(q)
                inputRef.current?.focus()
              }}
              className={cx(
                'underline decoration-underline underline-offset-4 hover:text-ink',
                prompt === q ? 'text-ink' : 'text-muted',
              )}
            >
              {q}
            </button>
          ))}
        </div>
        <div className="text-dim">
          <SentenceMenu
            title="Number of picks"
            label={`${count} pick${count === 1 ? '' : 's'}`}
            options={[1, 2, 3, 4, 5].map((n) => ({
              key: String(n),
              label: `${n} pick${n === 1 ? '' : 's'}`,
              checked: n === count,
              onSelect: () => setCount(n),
            }))}
          />
          {' · '}
          <SentenceMenu
            title="Libraries to pick from"
            label={storesLabel(sources)}
            multi
            options={SOURCES.map((s) => ({
              key: s,
              label: storeName(s),
              checked: sources.includes(s),
              onSelect: () => toggleSource(s),
            }))}
          />
        </div>
      </div>

      {config && !config.claude_available && (
        <p className="flex items-baseline gap-2 text-sm text-muted">
          <span className="h-[7px] w-[7px] shrink-0 translate-y-[-1px] rounded-full bg-warn" aria-hidden="true" />
          <span>
            No AI is set up, so picks come from UpNext's own ranking and won't fit as well.{' '}
            {BACKEND_KEY[config.llm_backend] ? (
              <>{BACKEND_LABEL[config.llm_backend]} is chosen, with no key saved yet. </>
            ) : (
              config.llm_error && (
                <>
                  <CodeText text={config.llm_error[0].toUpperCase() + config.llm_error.slice(1).replace(/\.?$/, '.')} />{' '}
                </>
              )
            )}
            <Link to="/settings#picks" className="underline decoration-underline underline-offset-4 hover:text-ink">
              Set one up
            </Link>
          </span>
        </p>
      )}
    </section>
  )
}
