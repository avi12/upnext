import { useMemo, useState } from 'react'
import type { StatementKind, TasteStatement, TasteSubject } from '../../types'
import { cx } from '../../lib/format'
import { SentenceChoice } from '../SentenceMenu'
import { Section, Segmented, TextButton } from '../settings/parts'
import { games, KINDS } from './words'

/** Likes, dislikes and hard nos: each can be moved to another kind or removed, and new ones added. */
export function StatementsSection({
  statements,
  subjects,
  onAdd,
  onMove,
  onRemove,
}: {
  statements: TasteStatement[]
  subjects: TasteSubject[]
  onAdd: (kind: StatementKind, about: string) => Promise<boolean>
  onMove: (s: TasteStatement, kind: StatementKind) => void
  onRemove: (s: TasteStatement) => void
}) {
  return (
    <Section id="statements" title="Likes, dislikes and hard nos" desc="A hard no is never picked; a dislike only counts against a game">
      <div className="mt-4 grid gap-x-10 gap-y-6 md:grid-cols-3">
        {KINDS.map((k) => {
          const list = statements.filter((s) => s.kind === k.value)
          return (
            <section key={k.value} aria-label={k.plural} className="flex flex-col gap-1">
              <h3 className="m-0 font-mono text-[10.5px] font-normal tracking-[0.08em] text-dim uppercase">{k.plural}</h3>
              {list.length === 0 && <p className="m-0 text-[14.5px] text-dim">{k.empty}</p>}
              <ul className="m-0 list-none p-0">
                {list.map((s) => (
                  <li key={s.id} className="flex flex-wrap items-baseline gap-x-2 border-b border-line py-2">
                    <span className="text-[15px] text-ink">{s.label}</span>
                    <span className="text-[13px] text-dim">{s.matches == null ? 'your words' : games(s.matches)}</span>
                    <span className="flex-1" />
                    <span className="text-[13px] text-muted">
                      <SentenceChoice
                        label={`Change ${s.label}`}
                        value={s.kind}
                        align="right"
                        options={KINDS.map((x) => ({ value: x.value, label: x.label.toLowerCase() }))}
                        onChange={(v) => onMove(s, v)}
                      />
                    </span>
                    <TextButton className="h-8 text-[13px]" onClick={() => onRemove(s)} aria-label={`Remove ${s.label}`}>
                      Remove
                    </TextButton>
                  </li>
                ))}
              </ul>
            </section>
          )
        })}
      </div>
      <AddStatement subjects={subjects} taken={new Set(statements.map((s) => s.about))} onAdd={onAdd} />
    </Section>
  )
}

/** A box that finds a genre, mood or other thing UpNext knows, or takes your own words. */
function AddStatement({
  subjects,
  taken,
  onAdd,
}: {
  subjects: TasteSubject[]
  taken: Set<string>
  onAdd: (kind: StatementKind, about: string) => Promise<boolean>
}) {
  const [text, setText] = useState('')
  const [picked, setPicked] = useState<TasteSubject | null>(null)
  const [kind, setKind] = useState<StatementKind>('like')
  const [busy, setBusy] = useState(false)
  const term = text.trim().toLowerCase()
  const matches = useMemo(() => {
    if (!term || picked) return []
    const starts = (s: TasteSubject) => s.label.toLowerCase().startsWith(term)
    return subjects
      .filter((s) => !taken.has(s.about) && (s.label.toLowerCase().includes(term) || s.group.toLowerCase().startsWith(term)))
      .sort((a, b) => Number(starts(b)) - Number(starts(a)) || b.count - a.count)
      .slice(0, 8)
  }, [term, picked, subjects, taken])

  const add = async () => {
    const about = picked ? picked.about : term ? `text:${text.trim()}` : null
    if (!about || busy) return
    setBusy(true)
    const ok = await onAdd(kind, about)
    setBusy(false)
    if (ok) {
      setText('')
      setPicked(null)
    }
  }

  return (
    <div className="mt-6 flex max-w-[760px] flex-col gap-2">
      <div className="flex flex-wrap items-center gap-3">
        <input
          type="text"
          value={text}
          onChange={(e) => {
            setText(e.target.value)
            setPicked(null)
          }}
          onKeyDown={(e) => e.key === 'Enter' && void add()}
          placeholder="Horror, short sessions, cozy, games about trains…"
          aria-label="Something you like or avoid"
          className="field min-w-0 flex-1 basis-64 text-sm"
        />
        <Segmented label="As a" options={KINDS.map((k) => ({ value: k.value, label: k.label }))} value={kind} onChange={setKind} />
        <button type="button" className="btn-outline h-10" disabled={busy || !term} onClick={() => void add()}>
          Add
        </button>
      </div>
      {matches.length > 0 && (
        <ul aria-label="Matches" className="m-0 list-none p-0">
          {matches.map((s) => (
            <li key={s.about}>
              <button
                type="button"
                onClick={() => {
                  setPicked(s)
                  setText(s.label)
                }}
                className="flex w-full items-baseline gap-3 rounded-md px-1 py-1.5 text-left hover:bg-raised"
              >
                <span className="text-[14.5px] text-ink">{s.label}</span>
                <span className="text-[12.5px] text-dim">{s.group}</span>
                <span className="flex-1" />
                <span className="mono">{games(s.count)}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
      <p className={cx('m-0 text-[13px] text-dim', !term && 'invisible')}>
        {picked
          ? `${picked.label}: ${games(picked.count)} in your library.`
          : matches.length
            ? 'Choose one of these, or Add keeps your own words, which only the AI reads.'
            : 'Not on the list: kept in your own words, which only the AI reads.'}
      </p>
    </div>
  )
}
