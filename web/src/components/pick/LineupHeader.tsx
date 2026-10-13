import type { PickBatch, PickMode } from '../../types'
import { lineupTitle } from '../../lib/format'
import { routineSummary } from '../../lib/routines'
import { SentenceMenu } from '../SentenceMenu'

/** Above a lineup picked in a discovery mode, what it was picked from. */
const MODE_NOTE: Record<PickMode, string> = {
  new_to_you: "New to you: easy, short games in a genre or with a mechanic you haven't played, each a way into a different one.",
  gems: 'Hidden gems: unplayed, highly rated games of a session or two, the most unusual first.',
  random: "One game off your radar, drawn at random with no AI: one you haven't started, or haven't played in over a year.",
}

/** Quick reasons for turning a lineup down; they steer the next lineup away. */
const NOT_THESE = [
  'Too long',
  'Too short',
  'Not in the mood for these genres',
  'Too intense',
  'Too slow',
  'Want something newer',
  'Want something with a story',
]

/** Over the lineup: its title, Curate again, "Not these, because…" (with a
 *  reason of your own), Clear, what it was picked from, and the reasons the
 *  next rolls steer away from. A game drawn with the dice has only
 *  its title, Clear and what it was drawn from: its card has the rest. */
export function LineupHeader({
  batch,
  avoid,
  onForget,
  ownReason,
  setOwnReason,
  reroll,
  clear,
}: {
  batch: PickBatch
  avoid: string[]
  onForget: () => void
  /** The "Something else…" reason being typed, or null when that form is closed. */
  ownReason: string | null
  setOwnReason: (r: string | null) => void
  reroll: (reason?: string) => void
  clear: () => void
}) {
  const dice = batch.mode === 'random'
  return (
    <>
      <div className="flex flex-wrap items-baseline justify-between gap-x-6 gap-y-2 border-b border-line pb-3.5">
        <div className="flex flex-wrap items-baseline gap-x-3.5 gap-y-1">
          <h2 className="font-display text-xl font-bold">{dice ? 'Rolled the dice' : lineupTitle()}</h2>
          {!dice && <span className="text-sm text-dim">Can't decide? Tick two to compare.</span>}
        </div>
        <div className="flex gap-6 text-[14.5px]">
          {!dice && (
            <>
              <button
                type="button"
                onClick={() => reroll()}
                className="-my-3 py-3 text-muted underline decoration-underline underline-offset-4 hover:text-ink"
                title="Curate again without these games (R)"
              >
                Curate again
              </button>
              <span className="text-muted">
                <SentenceMenu
                  title="Curate again, and say why not these"
                  label="Not these, because…"
                  options={[
                    ...NOT_THESE.map((r) => ({ key: r, label: r, checked: avoid.includes(r), onSelect: () => reroll(r) })),
                    { key: 'own', label: 'Something else…', checked: false, onSelect: () => setOwnReason('') },
                  ]}
                />
              </span>
            </>
          )}
          <button
            type="button"
            onClick={clear}
            className="-my-3 py-3 text-muted underline decoration-underline underline-offset-4 hover:text-ink"
          >
            Clear
          </button>
        </div>
      </div>

      {ownReason != null && (
        <form
          className="flex flex-wrap items-center gap-3 border-b border-line py-3.5"
          onSubmit={(e) => {
            e.preventDefault()
            if (ownReason.trim()) reroll(ownReason)
          }}
        >
          <label htmlFor="not-these" className="text-[14.5px] text-muted">
            Not these, because
          </label>
          <input
            id="not-these"
            autoFocus
            value={ownReason}
            maxLength={200}
            onChange={(e) => setOwnReason(e.target.value)}
            onKeyDown={(e) => e.key === 'Escape' && setOwnReason(null)}
            placeholder="I just finished a long RPG"
            className="field h-10 min-w-0 flex-1"
          />
          <button type="submit" className="btn-outline h-10" disabled={!ownReason.trim()}>
            Curate again
          </button>
          <button type="button" className="btn-quiet h-10" onClick={() => setOwnReason(null)}>
            Cancel
          </button>
        </form>
      )}
      {batch.routine && (
        <p className="m-0 pt-3 text-[13.5px] text-dim">
          For your routine “{batch.routine.name}”{routineSummary(batch.routine, '').replace(/^(.)/, ': $1')}.
        </p>
      )}
      {batch.mode && (dice || !batch.ranked_by) && <p className="m-0 pt-3 text-[13.5px] text-dim">{MODE_NOTE[batch.mode]}</p>}
      {/* Chance never meant to fit the request, so no warning that it won't. */}
      {batch.ranked_by && !dice && <RankedBy by={batch.ranked_by} />}
      {avoid.length > 0 && (
        <p className="m-0 pt-3 text-[13.5px] text-dim">
          Steering away from: {avoid.map((a) => a.toLowerCase()).join(' · ')}.{' '}
          <button type="button" className="underline decoration-underline underline-offset-4 hover:text-ink" onClick={onForget}>
            Forget these
          </button>
        </p>
      )}
    </>
  )
}

/** Above a lineup picked without AI: that it won't fit as well, and what UpNext ranked it by this time. */
function RankedBy({ by }: { by: string[] }) {
  return (
    <div role="note" className="mt-4 rounded-xl border border-line px-4 py-3.5 text-[13.5px] leading-[1.55] sm:px-5">
      <p className="m-0 flex items-baseline gap-2 text-ink">
        <span className="h-[7px] w-[7px] shrink-0 translate-y-[-1px] rounded-full bg-warn" aria-hidden="true" />
        <span>Picked without AI, so these won't fit what you asked for as well as the AI's picks would.</span>
      </p>
      <p className="m-0 mt-2 text-dim">Ranked by:</p>
      <ul className="m-0 mt-1 list-disc pl-5 text-muted marker:text-faint md:columns-2 md:gap-10">
        {by.map((b) => (
          <li key={b} className="break-inside-avoid">
            {b}
          </li>
        ))}
      </ul>
    </div>
  )
}
