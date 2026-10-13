import { useState } from 'react'
import type { DiaryHabits, HabitKey, TasteHabit } from '../../types'
import { cx, minutesToText, plural } from '../../lib/format'
import { SentenceChoice } from '../SentenceMenu'
import { Section, TextButton } from '../settings/parts'
import { andList, CHIP, CHIP_OFF, CHIP_ON, HABIT_CHOICES, SESSION_BLANK, SESSION_MINUTES } from './words'

const ROWS: { key: HabitKey; question: string }[] = [
  { key: 'times', question: 'When you usually play' },
  { key: 'who', question: 'Who with' },
  { key: 'where', question: 'Where' },
]

/** Habits and context: personal, only what you say, each answer editable. */
export function HabitsSection({
  habits,
  diary,
  diaryOn,
  onSet,
}: {
  habits: TasteHabit[]
  /** What the play diary shows. */
  diary: DiaryHabits
  diaryOn: boolean
  /** null clears the answer. */
  onSet: (about: HabitKey, value: number | string[] | null, note?: string | null) => void
}) {
  const of = (k: HabitKey) => habits.find((h) => h.about === k)
  const minutes = (k: HabitKey) => {
    const v = of(k)?.value
    return typeof v === 'number' ? String(v) : ''
  }
  const choices = (k: HabitKey) => {
    const v = of(k)?.value
    return Array.isArray(v) ? v : []
  }
  return (
    <Section id="habits" title="Habits and context" desc="Personal. Only what you tell it here, and you can change or clear any of it">
      <div className="mt-4 flex max-w-[860px] flex-col gap-5 text-[15.5px] leading-[1.6]">
        <p className="m-0 text-muted">
          On a weeknight you usually play for{' '}
          <SentenceChoice
            label="How long on a weeknight"
            value={minutes('weeknight')}
            options={SESSION_MINUTES}
            placeholder={SESSION_BLANK}
            onChange={(v) => onSet('weeknight', v ? Number(v) : null)}
          />
          , and on a weekend for{' '}
          <SentenceChoice
            label="How long on a weekend"
            value={minutes('weekend')}
            options={SESSION_MINUTES}
            placeholder={SESSION_BLANK}
            onChange={(v) => onSet('weekend', v ? Number(v) : null)}
          />
          .
        </p>
        {ROWS.map((r) => (
          <ChoiceRow key={`${r.key}:${choices(r.key).join(',')}`} question={r.question} options={HABIT_CHOICES[r.key] ?? []} value={choices(r.key)} onChange={(v) => onSet(r.key, v.length ? v : null)} />
        ))}
        <DiaryHabitsBlock diary={diary} minutes={minutes} choices={choices} onSet={onSet} />
        <p className="m-0 text-[13.5px] text-dim">
          {diaryOn
            ? 'The play diary is on: it notes when you play, hour by hour, and you can turn it off or delete it under Where it comes from. What you say here wins over what it shows.'
            : 'Nothing here is gathered by itself unless you turn on the play diary, under Where it comes from.'}
        </p>
      </div>
    </Section>
  )
}

const TIME_WORD: Record<string, string> = { mornings: 'mornings', afternoons: 'afternoons', evenings: 'evenings', late_nights: 'late at night' }

/** What the play diary shows, each with "Use this" to say it yourself. */
function DiaryHabitsBlock({
  diary,
  minutes,
  choices,
  onSet,
}: {
  diary: DiaryHabits
  minutes: (k: HabitKey) => string
  choices: (k: HabitKey) => string[]
  onSet: (about: HabitKey, value: number | string[] | null) => void
}) {
  const lines: { key: string; text: string; use?: () => void }[] = []
  for (const [k, when] of [
    ['weeknight', 'weeknight'],
    ['weekend', 'weekend'],
  ] as const) {
    const s = diary[k]
    if (!s) continue
    const same = minutes(k) === String(s.minutes)
    lines.push({
      key: k,
      text: `Your ${when} sessions usually run ${minutesToText(s.minutes)} (${plural(s.count, 'session')}).`,
      use: same ? undefined : () => onSet(k, s.minutes),
    })
  }
  if (diary.times.length) {
    const stated = choices('times')
    const same = stated.length === diary.times.length && diary.times.every((t) => stated.includes(t))
    lines.push({
      key: 'times',
      text: `You play mostly ${andList(diary.times.map((t) => TIME_WORD[t] ?? t))}.`,
      use: same ? undefined : () => onSet('times', (HABIT_CHOICES.times ?? []).map((o) => o.value).filter((v) => diary.times.includes(v))),
    })
  }
  const mp = diary.multiplayer
  if (mp) {
    const when = mp.weekend_share >= 0.6 ? ' mostly at the weekend' : mp.weekend_share <= 0.4 ? ' mostly on weeknights' : ''
    const at = mp.time ? `${when ? ',' : ''} mostly ${TIME_WORD[mp.time] ?? mp.time}` : ''
    lines.push({ key: 'mp', text: `Multiplayer games get played${when}${at} (${minutesToText(mp.minutes)} in all).` })
  }
  if (!lines.length) return null
  return (
    <div className="flex flex-col gap-1">
      <span className="text-muted">
        From your play diary, {diary.days} {diary.days === 1 ? 'day' : 'days'} of play
      </span>
      <ul className="m-0 list-none p-0">
        {lines.map((l) => (
          <li key={l.key} className="flex flex-wrap items-center gap-x-4">
            <span>{l.text}</span>
            {l.use && <TextButton onClick={l.use}>Use this</TextButton>}
          </li>
        ))}
      </ul>
      <p className="m-0 text-[13.5px] text-dim">Picks read these where you haven’t said; “Use this” says it for you, so it holds even after the diary is deleted.</p>
    </div>
  )
}

function ChoiceRow({
  question,
  options,
  value,
  onChange,
}: {
  question: string
  options: { value: string; label: string }[]
  value: string[]
  onChange: (v: string[]) => void
}) {
  const [chosen, setChosen] = useState<string[] | null>(null)
  const current = chosen ?? value
  const flip = (v: string) => {
    const next = current.includes(v) ? current.filter((x) => x !== v) : [...current, v]
    setChosen(next)
    onChange(options.map((o) => o.value).filter((o) => next.includes(o)))
  }
  return (
    <div className="flex flex-col gap-2">
      <span className="text-muted">{question}</span>
      <div role="group" aria-label={question} className="flex flex-wrap gap-2">
        {options.map((o) => (
          <button key={o.value} type="button" aria-pressed={current.includes(o.value)} onClick={() => flip(o.value)} className={cx(CHIP, current.includes(o.value) ? CHIP_ON : CHIP_OFF)}>
            {o.label}
          </button>
        ))}
      </div>
    </div>
  )
}
