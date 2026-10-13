import { useState, type ReactNode } from 'react'
import { Link } from 'react-router'
import type { GameView, SecondChanceAnswer, SecondChanceEntry, SecondChanceVerdict, StopReason } from '../../types'
import { Cover } from '../Cover'
import { PlayLink, TrailerLink } from './GameActions'
import { cx, hoursToText, minutesToText, plural } from '../../lib/format'
import { dayText } from '../../lib/statuses'

const VERDICT: Record<SecondChanceVerdict, { label: string; tone: string }> = {
  better: { label: 'Reviews say it’s better now', tone: 'text-good' },
  partly: { label: 'Partly better, reviews say', tone: 'text-warn' },
  same: { label: 'Still the same, reviews say', tone: 'text-bad' },
  unclear: { label: 'The reviews don’t settle it', tone: 'text-dim' },
}

const ANSWERS: { value: SecondChanceAnswer; label: string; className: string }[] = [
  { value: 'willing', label: 'Willing to try again', className: 'btn-accent' },
  { value: 'still_no', label: 'Still no', className: 'btn-outline' },
  { value: 'not_for_me', label: 'Not for me', className: 'btn-outline' },
]

function year(unix: number | null): number | null {
  return unix ? new Date(unix * 1000).getFullYear() : null
}

/** One abandoned game on the Second Chance page: Then (why you stopped)
 *  beside Now (what reviews written since say about it, by the AI), a reply
 *  that goes with your reasons, and the three answers. */
export function SecondChanceRow({
  entry,
  reasons,
  ai,
  busy,
  onOpen,
  onAnswer,
  onReasons,
}: {
  entry: SecondChanceEntry
  /** GET /api/stop-reasons, for their labels and the chips. */
  reasons: StopReason[] | undefined
  ai: { available: boolean; reason: string | null }
  busy: boolean
  onOpen: (g: GameView) => void
  onAnswer: (g: GameView, answer: SecondChanceAnswer) => void
  onReasons: (g: GameView, codes: string[], note: string | null) => Promise<boolean>
}) {
  const g = entry.game
  const why = g.stop_reasons
  const note = g.second_chance
  const [editing, setEditing] = useState(false)
  const [codes, setCodes] = useState<string[]>(why?.codes ?? [])
  const [reply, setReply] = useState('')
  const label = (code: string) => reasons?.find((r) => r.code === code)?.label ?? code
  const played = g.playtime_known && g.playtime_minutes > 0 ? minutesToText(g.playtime_minutes) : null
  const story = hoursToText(g.hltb_main)
  const stopped = entry.stopped_at ? dayText(entry.stopped_at) : null
  const meta = [played && story ? `${played} of a ${story} story` : played, stopped && `stopped ${stopped}`].filter(Boolean).join(' · ')
  const changed = editing && (codes.length !== (why?.codes.length ?? 0) || codes.some((c) => !why?.codes.includes(c)))
  const canSave = reply.trim() !== '' || changed

  const save = async () => {
    if (!canSave) return
    const ok = await onReasons(g, codes, reply.trim() || why?.note || null)
    if (ok) {
      setReply('')
      setEditing(false)
    }
  }

  const flip = (code: string) => setCodes((c) => (c.includes(code) ? c.filter((x) => x !== code) : [...c, code]))

  let now: ReactNode
  if (note?.now) {
    const v = note.verdict ? VERDICT[note.verdict] : null
    now = (
      <>
        {v && <p className={cx('m-0 text-[13px] font-semibold', note.outdated ? 'text-dim' : v.tone)}>{v.label}</p>}
        <p className={cx('m-0 text-[15px] leading-[1.55]', note.outdated ? 'text-muted' : 'text-ink')}>{note.now}</p>
        <p className="m-0 mt-1 text-[12.5px] text-dim">
          {note.outdated
            ? 'Written before your reasons or the reviews last changed; the next check writes it again.'
            : `From ${entry.reviews_since} ${entry.reviews_since === 1 ? 'review' : 'reviews'} written since you stopped${note.written_at ? ` · checked ${dayText(note.written_at)}` : ''}`}
        </p>
      </>
    )
  } else if (entry.reviews_since === 0) {
    now = (
      <p className="m-0 text-[14.5px] text-muted">
        No player reviews since you stopped{year(entry.stopped_at) ? ` in ${year(entry.stopped_at)}` : ''}, so there’s nothing to check against.
      </p>
    )
  } else if (!ai.available) {
    now = (
      <p className="m-0 text-[14.5px] text-muted">
        Now needs an AI to read the {plural(entry.reviews_since, 'review')} written since you stopped.{' '}
        <Link to="/settings#picks" className="text-ink underline decoration-underline underline-offset-4">
          Set one up
        </Link>
      </p>
    )
  } else {
    now = (
      <p className="m-0 text-[14.5px] text-muted">
        Not checked yet: {entry.reviews_since} {entry.reviews_since === 1 ? 'review' : 'reviews'} written since you stopped wait for the next
        check.
      </p>
    )
  }

  const chip = (r: StopReason) => (
    <button
      key={r.code}
      type="button"
      aria-pressed={codes.includes(r.code)}
      onClick={() => flip(r.code)}
      className={cx(
        'min-h-8 rounded-full border px-3 text-[13px] leading-tight',
        codes.includes(r.code) ? 'border-accent bg-accent text-accent-ink' : 'border-line-strong text-ink hover:bg-raised',
      )}
    >
      {r.label}
    </button>
  )

  return (
    <li className="border-b border-line py-6">
      <div className="flex flex-wrap items-center gap-x-6 gap-y-3">
        <button type="button" onClick={() => onOpen(g)} className="shrink-0 rounded-[8px]" aria-label={`Open ${g.name}`}>
          <Cover game={g} className="w-16 rounded-[8px]" titleSize="text-[10px]" />
        </button>
        <div className="flex min-w-0 flex-1 basis-56 flex-col gap-1.5">
          <button
            type="button"
            onClick={() => onOpen(g)}
            className="self-start text-left font-display text-[19px] leading-[1.2] font-bold tracking-[-0.01em] hover:text-accent"
          >
            {g.name}
          </button>
          {meta && <div className="mono">{meta}</div>}
        </div>
        <div className="flex items-center gap-x-6 text-[14.5px]">
          <PlayLink game={g} />
          <TrailerLink game={g} />
        </div>
      </div>

      <div className="mt-4 grid gap-x-10 gap-y-5 md:grid-cols-2">
        <section aria-label={`Then: why you stopped ${g.name}`} className="flex flex-col gap-1.5">
          <h3 className="m-0 font-mono text-[10.5px] font-normal tracking-[0.08em] text-dim uppercase">Then</h3>
          {why ? (
            <>
              {why.codes.length > 0 && <p className="m-0 text-[15px] leading-[1.55] text-ink">{why.codes.map(label).join(' · ')}</p>}
              {why.note && <p className="m-0 text-[15px] leading-[1.55] text-ink">“{why.note}”</p>}
            </>
          ) : (
            <p className="m-0 text-[15px] leading-[1.55] text-muted">You didn’t say why.</p>
          )}
          {!editing && (
            <button
              type="button"
              onClick={() => {
                setCodes(why?.codes ?? [])
                setEditing(true)
              }}
              className="btn-quiet self-start text-[13.5px] text-dim underline decoration-underline underline-offset-4 hover:text-ink"
            >
              {why?.codes.length ? 'Change the reasons' : 'Pick reasons'}
            </button>
          )}
          {editing && reasons && (
            <div className="mt-1 flex flex-col gap-2.5">
              {(['life', 'game'] as const).map((grp) => (
                <fieldset key={grp} className="m-0 border-0 p-0">
                  <legend className="mb-1.5 p-0 font-mono text-[10.5px] tracking-[0.08em] text-dim uppercase">
                    {grp === 'life' ? 'Life' : 'The game'}
                  </legend>
                  <div className="flex flex-wrap gap-1.5">{reasons.filter((r) => r.group === grp).map(chip)}</div>
                </fieldset>
              ))}
            </div>
          )}
        </section>
        <section aria-label={`Now: what reviews say about ${g.name} since`} className="flex flex-col gap-1.5">
          <h3 className="m-0 font-mono text-[10.5px] font-normal tracking-[0.08em] text-dim uppercase">Now</h3>
          {now}
        </section>
      </div>

      <div className="mt-4 flex flex-wrap items-center gap-2">
        <input
          type="text"
          value={reply}
          maxLength={500}
          onChange={(e) => setReply(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && void save()}
          placeholder={why?.note ? 'Your reply (replaces your words above)' : 'Your reply: what stopped you, in your own words'}
          aria-label={`Your reply about ${g.name}`}
          className="h-10 min-w-0 flex-1 basis-64 rounded-lg border border-line-strong bg-ground px-3 text-[14px] text-ink placeholder:text-dim"
        />
        <button type="button" onClick={() => void save()} disabled={!canSave || busy} className="btn-outline h-10 disabled:opacity-50">
          Save
        </button>
        {editing && (
          <button
            type="button"
            onClick={() => {
              setEditing(false)
              setCodes(why?.codes ?? [])
            }}
            className="btn-quiet h-10 px-2 text-[14px]"
          >
            Cancel
          </button>
        )}
      </div>

      <div className="mt-4 flex flex-wrap items-center gap-2" role="group" aria-label={`Your answer for ${g.name}`}>
        {ANSWERS.map((a) => (
          <button
            key={a.value}
            type="button"
            disabled={busy}
            // The answered row leaves the list and the next one moves up under
            // the pointer, so the second click of a double-click is ignored
            // rather than answering that next game.
            onClick={(e) => e.detail <= 1 && onAnswer(g, a.value)}
            className={cx(a.className, 'h-10')}
          >
            {a.label}
          </button>
        ))}
      </div>
    </li>
  )
}
