import { useState, type ReactNode } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { api, ApiError } from '../../api'
import type { GameView, HabitKey, StatementKind, TasteFavourite, TasteSubject } from '../../types'
import { cx, minutesToText } from '../../lib/format'
import { Cover } from '../Cover'
import { SentenceChoice } from '../SentenceMenu'
import { Skeleton } from '../Skeleton'
import { TextButton } from '../settings/parts'
import { useToast } from '../useToast'
import { FindGame } from '../profile/Favourites'
import { andList, CHIP, CHIP_OFF, CHIP_ON, DIARY_SENTENCE, HABIT_CHOICES, SESSION_BLANK, SESSION_MINUTES } from '../profile/words'
import { keys } from '../../lib/queries'

type Step = 'favourites' | 'why' | 'quick' | 'done'

function errText(e: unknown) {
  return e instanceof ApiError ? e.message : String(e)
}

/** The first-run questions: favourites from your most played, why
 *  you love them, then quick questions, every one skippable. Each answer is
 *  saved as it's given, as the same statements the profile page writes, so
 *  leaving halfway keeps what was said. "Tune your profile" shows it on a
 *  page of its own, where `onFinish` is where it goes at the end; the setup
 *  wizard shows it as a step and passes `onEnd`, which takes the
 *  last question's Next instead of the summary. */
export function TasteQuestions({
  onFinish,
  onEnd,
  onBack,
  onSkip,
}: {
  onFinish: (to: 'pick' | 'profile') => void
  onEnd?: () => void
  /** Back from the first page: the wizard's step before. */
  onBack?: () => void
  /** "Skip this step", on the pages before the last, whose Next already ends the step. */
  onSkip?: () => void
}) {
  const qc = useQueryClient()
  const toast = useToast()
  const taste = useQuery({ queryKey: ['taste'], queryFn: api.taste })
  const questions = useQuery({ queryKey: ['taste-questions'], queryFn: api.tasteQuestions })
  const [step, setStep] = useState<Step>('favourites')
  const [busy, setBusy] = useState<Set<string>>(new Set())

  const refresh = () =>
    Promise.all(['taste', 'taste-questions', 'taste-sources'].map((k) => qc.invalidateQueries({ queryKey: [k] })))

  /** Run a save under `key` (so its button waits), then read the profile again. */
  const save = async (key: string, what: string, f: () => Promise<unknown>) => {
    if (busy.has(key)) return
    setBusy((b) => new Set(b).add(key))
    try {
      await f()
    } catch (e) {
      toast.error(what, errText(e))
    } finally {
      await refresh()
      setBusy((b) => {
        const n = new Set(b)
        n.delete(key)
        return n
      })
    }
  }

  const t = taste.data
  const q = questions.data
  if (taste.isError || questions.isError) {
    return (
      <div role="alert">
        <p className="m-0 text-[15px] text-ink">Couldn't load the questions.</p>
        <p className="mono mt-1">{((taste.error ?? questions.error) as Error).message}</p>
        <button type="button" className="btn-outline mt-4 h-11" onClick={() => void refresh()}>
          Try again
        </button>
      </div>
    )
  }
  if (!t || !q) {
    return (
      <div className="space-y-4" aria-busy="true">
        <Skeleton className="h-8 w-72" />
        <Skeleton className="h-40 w-full" />
      </div>
    )
  }

  const favouriteOf = (id: number) => t.favourites.find((f) => f.game.id === id)
  const toggleFavourite = (g: GameView) => {
    const f = favouriteOf(g.id)
    void save(`fav:${g.id}`, f ? `Couldn't take ${g.name} off` : `Couldn't add ${g.name}`, () =>
      f ? api.tasteDelete(f.id) : api.tasteState({ kind: 'favourite', title_id: g.id }),
    )
  }
  const statementOf = (about: string) => t.statements.find((s) => s.about === about)
  const toggleStatement = (kind: StatementKind, s: TasteSubject) => {
    const cur = statementOf(s.about)
    void save(`st:${s.about}`, `Couldn't save ${s.label}`, () =>
      cur && cur.kind === kind ? api.tasteDelete(cur.id) : api.tasteState({ kind, about: s.about }),
    )
  }
  const habit = (k: HabitKey) => t.habits.find((h) => h.about === k)
  const setHabit = (k: HabitKey, value: number | string[] | null) => {
    const h = habit(k)
    void save(`habit:${k}`, "Couldn't save that", () =>
      value == null ? (h ? api.tasteDelete(h.id) : Promise.resolve()) : api.tasteState({ kind: 'context', about: k, why: value, note: h?.note ?? null }),
    )
  }
  const setDiary = (on: boolean) =>
    void save('diary', on ? "Couldn't turn the play diary on" : "Couldn't turn it off", async () => {
      const r = await api.tasteDiary(on)
      qc.invalidateQueries({ queryKey: keys.settings })
      if (on && r.diary.error) toast.error('The play diary is on, but Steam didn’t answer', r.diary.error)
    })

  const steps: Step[] = ['favourites', 'why', 'quick']
  const at = steps.indexOf(step)
  const go = (s: Step) => {
    setStep(s)
    window.scrollTo({ top: 0 })
  }

  return (
    <div className="flex max-w-[960px] flex-col gap-8">
      {step !== 'done' && (
        <p className="m-0 text-sm text-dim" aria-live="polite">
          {at + 1} of {steps.length}
        </p>
      )}
      {step === 'favourites' && (
        <FavouritesStep
          mostPlayed={q.most_played.map((m) => m.game)}
          fewPlayed={q.few_played}
          favourites={t.favourites}
          busy={busy}
          onToggle={toggleFavourite}
          onNext={() => go(t.favourites.length ? 'why' : 'quick')}
          onBack={onBack}
          onSkip={onSkip}
        />
      )}
      {step === 'why' && (
        <WhyStep
          favourites={t.favourites}
          busy={busy}
          onSave={(f, edit) =>
            void save(`${edit.note == null ? 'why' : 'note'}:${f.id}`, `Couldn't save why you love ${f.game.name}`, () =>
              api.tasteEdit(f.id, edit),
            )
          }
          onBack={() => go('favourites')}
          onNext={() => go('quick')}
          onSkip={onSkip}
        />
      )}
      {step === 'quick' && (
        <section aria-labelledby="q-quick" className="flex flex-col gap-8">
          <Heading id="q-quick" title="A few quick questions" desc="Answer any, skip the rest. You can change every answer on your profile." />
          <ChipQuestion
            title="Genres or moods you love"
            desc="Picks rank games like these higher."
            subjects={q.loves}
            chosen={(s) => statementOf(s.about)?.kind === 'like'}
            busy={busy}
            onToggle={(s) => toggleStatement('like', s)}
          />
          <ChipQuestion
            title="Hard nos"
            desc="Never picked, whatever you ask for. The count is the games in your library each one leaves out."
            subjects={q.hard_nos}
            chosen={(s) => statementOf(s.about)?.kind === 'hard_no'}
            busy={busy}
            withCount
            onToggle={(s) => toggleStatement('hard_no', s)}
          />
          <div className="flex flex-col gap-2">
            <h3 className="m-0 text-[16px] font-bold">How long you usually play</h3>
            <p className="m-0 text-[14.5px] text-muted">Picks use this when you don’t say how long.</p>
            <p className="m-0 text-[15.5px] leading-[1.6] text-muted">
              On a weeknight, for{' '}
              <SentenceChoice
                label="How long on a weeknight"
                value={typeof habit('weeknight')?.value === 'number' ? String(habit('weeknight')?.value) : ''}
                options={SESSION_MINUTES}
                placeholder={SESSION_BLANK}
                onChange={(v) => setHabit('weeknight', v ? Number(v) : null)}
              />
              , and on a weekend for{' '}
              <SentenceChoice
                label="How long on a weekend"
                value={typeof habit('weekend')?.value === 'number' ? String(habit('weekend')?.value) : ''}
                options={SESSION_MINUTES}
                placeholder={SESSION_BLANK}
                onChange={(v) => setHabit('weekend', v ? Number(v) : null)}
              />
              .
            </p>
          </div>
          <div className="flex flex-col gap-3">
            <div className="flex flex-col gap-1">
              <h3 className="m-0 flex flex-wrap items-baseline gap-x-2.5 gap-y-1 text-[16px] font-bold">
                When, where and who with <PersonalPill />
              </h3>
              <p className="m-0 text-[14.5px] text-muted">
                Only what you choose here. Picks use it to fit the time you have, and it goes to your AI with each pick.
              </p>
            </div>
            {HABIT_ROWS.map(([k, question]) => {
              const options = HABIT_CHOICES[k] ?? []
              const cur = Array.isArray(habit(k)?.value) ? (habit(k)?.value as string[]) : []
              return (
                <div key={k} className="flex flex-col gap-1.5">
                  <span className="text-[14.5px] text-muted">{question}</span>
                  <div role="group" aria-label={question} className="flex flex-wrap gap-2">
                    {options.map((o) => {
                      const on = cur.includes(o.value)
                      const next = on ? cur.filter((x) => x !== o.value) : [...cur, o.value]
                      return (
                        <button
                          key={o.value}
                          type="button"
                          aria-pressed={on}
                          disabled={busy.has(`habit:${k}`)}
                          onClick={() => setHabit(k, next.length ? options.map((c) => c.value).filter((v) => next.includes(v)) : null)}
                          className={cx(CHIP, on ? CHIP_ON : CHIP_OFF)}
                        >
                          {o.label}
                        </button>
                      )
                    })}
                  </div>
                </div>
              )
            })}
          </div>
          <div className="flex flex-col gap-2">
            <h3 className="m-0 flex flex-wrap items-baseline gap-x-2.5 gap-y-1 text-[16px] font-bold">
              The play diary <PersonalPill />
            </h3>
            <p className="m-0 max-w-[760px] text-[15px] leading-[1.6] text-muted">
              Off unless you turn it on. {DIARY_SENTENCE} The diary itself stays on this computer; the habits it finds go to your AI
              with your picks. You can turn it off or delete it any time on your profile.
              {!q.diary.steam && ' It needs your Steam Web API key and Steam ID (Settings › Accounts) to read anything.'}
            </p>
            <div role="group" aria-label="Keep a play diary" className="flex flex-wrap gap-2">
              {[
                [true, 'Keep a diary'],
                [false, 'No thanks'],
              ].map(([on, label]) => (
                <button
                  key={String(on)}
                  type="button"
                  aria-pressed={q.diary.on === on}
                  disabled={busy.has('diary')}
                  onClick={() => q.diary.on !== on && setDiary(on as boolean)}
                  className={cx(CHIP, q.diary.on === on ? CHIP_ON : CHIP_OFF)}
                >
                  {label as string}
                </button>
              ))}
            </div>
          </div>
          <Buttons
            onBack={() => go(t.favourites.length ? 'why' : 'favourites')}
            onNext={() => (onEnd ? onEnd() : go('done'))}
            next={onEnd ? 'Next' : 'Done'}
          />
        </section>
      )}
      {step === 'done' && (
        <section aria-labelledby="q-done" className="flex flex-col gap-5">
          <Heading id="q-done" title="That’s your profile" desc={doneText(t.favourites.length, t.statements, t.habits.length, q.diary.on)} />
          <div className="flex flex-wrap gap-3">
            <button type="button" className="btn-accent h-11" onClick={() => onFinish('pick')}>
              Pick something to play
            </button>
            <button type="button" className="btn-outline h-11" onClick={() => onFinish('profile')}>
              See your profile
            </button>
          </div>
        </section>
      )}
    </div>
  )
}

/** The habits the quick questions ask, as the profile's Habits section does. */
const HABIT_ROWS: [HabitKey, string][] = [
  ['times', 'When you usually play'],
  ['where', 'Where you play'],
  ['who', 'Who you play with'],
]

/** Marks the questions about your life rather than your games. */
function PersonalPill() {
  return (
    <span
      className="rounded-full border border-line-strong px-2 py-px text-[12px] font-normal whitespace-nowrap text-dim"
      title="Optional, and about you rather than your games. Change or clear it any time on your profile."
    >
      Optional · personal
    </span>
  )
}

function doneText(favourites: number, statements: { kind: StatementKind }[], habits: number, diary: boolean): string {
  const n = (k: StatementKind) => statements.filter((s) => s.kind === k).length
  const parts = [
    favourites ? `${favourites} ${favourites === 1 ? 'favourite' : 'favourites'}` : '',
    n('like') ? `${n('like')} ${n('like') === 1 ? 'like' : 'likes'}` : '',
    n('dislike') ? `${n('dislike')} ${n('dislike') === 1 ? 'dislike' : 'dislikes'}` : '',
    n('hard_no') ? `${n('hard_no')} ${n('hard_no') === 1 ? 'hard no' : 'hard nos'}` : '',
    habits ? `${habits} ${habits === 1 ? 'habit' : 'habits'}` : '',
  ].filter(Boolean)
  const said = parts.length ? `It holds ${andList(parts)}, and picks read it from now on.` : 'You skipped everything, which is fine: picks lean on how you play.'
  return `${said} The play diary is ${diary ? 'on' : 'off'}.`
}

function Heading({ id, title, desc }: { id: string; title: string; desc?: ReactNode }) {
  return (
    <div className="flex flex-col gap-1.5">
      <h2 id={id} className="m-0 font-display text-[26px] font-[750] tracking-[-0.02em]">
        {title}
      </h2>
      {desc && <p className="m-0 max-w-[760px] text-[16px] leading-[1.6] text-muted">{desc}</p>}
    </div>
  )
}

function Buttons({ onBack, onNext, onSkip, next = 'Next' }: { onBack?: () => void; onNext: () => void; onSkip?: () => void; next?: string }) {
  return (
    <div className="flex flex-wrap items-center gap-3 border-t border-line pt-5">
      <button type="button" className="btn-accent h-11" onClick={onNext}>
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

function FavouritesStep({
  mostPlayed,
  fewPlayed,
  favourites,
  busy,
  onToggle,
  onNext,
  onBack,
  onSkip,
}: {
  mostPlayed: GameView[]
  fewPlayed: boolean
  favourites: TasteFavourite[]
  busy: Set<string>
  onToggle: (g: GameView) => void
  onNext: () => void
  onBack?: () => void
  onSkip?: () => void
}) {
  const [searching, setSearching] = useState(fewPlayed)
  const chosen = new Set(favourites.map((f) => f.game.id))
  const others = favourites.filter((f) => !mostPlayed.some((g) => g.id === f.game.id))
  return (
    <section aria-labelledby="q-fav" className="flex flex-col gap-6">
      <Heading
        id="q-fav"
        title={fewPlayed ? 'Which games do you love?' : 'Which of these do you love?'}
        desc={
          fewPlayed
            ? 'Your stores report little playtime (Epic and Amazon games through Heroic report none), so search your library for games you love. Choose as many as you like; a vague request like “anything” leans on them most.'
            : 'Your most-played games, recent play weighted up. Choose your favourites, as many as you like; a vague request like “anything” leans on them most.'
        }
      />
      {mostPlayed.length > 0 && (
        <ul aria-label="Your most played" className="m-0 grid list-none grid-cols-3 gap-4 p-0 sm:grid-cols-4 md:grid-cols-6">
          {mostPlayed.map((g) => {
            const on = chosen.has(g.id)
            return (
              <li key={g.id}>
                <button
                  type="button"
                  aria-pressed={on}
                  disabled={busy.has(`fav:${g.id}`)}
                  onClick={() => onToggle(g)}
                  className="group flex w-full flex-col gap-1.5 text-left"
                  aria-label={`${g.name}: ${on ? 'a favourite' : 'make it a favourite'}`}
                >
                  <span className={cx('relative block rounded-[10px] p-[3px] transition', on ? 'bg-accent' : 'bg-transparent')}>
                    <Cover game={g} className={cx('w-full rounded-[8px] transition', !on && 'group-hover:opacity-80')} titleSize="text-[11px]" />
                    {on && (
                      <span aria-hidden className="absolute top-2 right-2 grid h-7 w-7 place-items-center rounded-full bg-accent text-accent-ink">
                        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round">
                          <path d="M5 12.5l4.5 4.5L19 7.5" />
                        </svg>
                      </span>
                    )}
                  </span>
                  <span className={cx('truncate text-[13px]', on ? 'font-semibold text-ink' : 'text-ink')}>{g.name}</span>
                  <span className="mono -mt-1">{minutesToText(g.playtime_minutes)}</span>
                </button>
              </li>
            )
          })}
        </ul>
      )}
      {others.length > 0 && (
        <div className="flex flex-wrap items-center gap-2">
          <span className="text-sm text-dim">Also chosen:</span>
          {others.map((f) => (
            <button
              key={f.id}
              type="button"
              disabled={busy.has(`fav:${f.game.id}`)}
              onClick={() => onToggle(f.game)}
              className={cx(CHIP, CHIP_ON)}
              aria-label={`${f.game.name}: take it off your favourites`}
            >
              {f.game.name} ×
            </button>
          ))}
        </div>
      )}
      {searching ? (
        <FindGame exclude={chosen} onPick={onToggle} onClose={() => setSearching(false)} />
      ) : (
        <TextButton className="self-start" onClick={() => setSearching(true)}>
          Find another game
        </TextButton>
      )}
      {/* With "Skip this step" beside it, Next stays Next: two skips would read as one. */}
      <Buttons onNext={onNext} onBack={onBack} onSkip={onSkip} next={favourites.length || onSkip ? 'Next' : 'Skip this'} />
    </section>
  )
}

function WhyStep({
  favourites,
  busy,
  onSave,
  onBack,
  onNext,
  onSkip,
}: {
  favourites: TasteFavourite[]
  busy: Set<string>
  onSave: (f: TasteFavourite, edit: { why?: string[]; note?: string }) => void
  onBack: () => void
  onNext: () => void
  onSkip?: () => void
}) {
  return (
    <section aria-labelledby="q-why" className="flex flex-col gap-6">
      <Heading
        id="q-why"
        title="What do you love about them?"
        desc="Choose what fits, from what players praise, how reviews describe each game and its tags, or say it in your own words. Skip any you like."
      />
      <ul className="m-0 flex list-none flex-col p-0">
        {favourites.map((f) => (
          <WhyRow key={f.id} fav={f} busy={busy.has(`why:${f.id}`)} onSave={(edit) => onSave(f, edit)} />
        ))}
      </ul>
      <Buttons onBack={onBack} onNext={onNext} onSkip={onSkip} />
    </section>
  )
}

/** One favourite's reasons as chips, saved as they're chosen, and a line of your own, saved when you leave it. */
function WhyRow({ fav, busy, onSave }: { fav: TasteFavourite; busy: boolean; onSave: (edit: { why?: string[]; note?: string }) => void }) {
  const [note, setNote] = useState(fav.note ?? '')
  const chosen = fav.why.map((c) => c.key)
  const all = [...fav.why, ...fav.choices.filter((c) => !chosen.includes(c.key))]
  const flip = (key: string) => onSave({ why: chosen.includes(key) ? chosen.filter((k) => k !== key) : [...chosen, key] })
  return (
    <li className="flex flex-wrap items-start gap-x-5 gap-y-3 border-b border-line py-5">
      <Cover game={fav.game} className="w-16 shrink-0 rounded-[8px]" titleSize="text-[9px]" />
      <div className="flex min-w-0 flex-1 basis-64 flex-col gap-3">
        <p className="m-0 font-display text-[18px] leading-[1.2] font-bold tracking-[-0.01em]">{fav.game.name}</p>
        {all.length > 0 ? (
          <div role="group" aria-label={`What you love about ${fav.game.name}`} className="flex flex-wrap gap-2">
            {all.map((c) => (
              <button
                key={c.key}
                type="button"
                aria-pressed={chosen.includes(c.key)}
                disabled={busy}
                onClick={() => flip(c.key)}
                className={cx(CHIP, chosen.includes(c.key) ? CHIP_ON : CHIP_OFF)}
              >
                {c.label}
              </button>
            ))}
          </div>
        ) : (
          <p className="m-0 text-sm text-dim">UpNext has no tags or review summary for this game yet, so say it in your own words.</p>
        )}
        <label className="flex max-w-[620px] flex-col gap-1 text-[13.5px] text-dim">
          In your own words
          <input
            type="text"
            value={note}
            maxLength={300}
            onChange={(e) => setNote(e.target.value)}
            onBlur={() => note.trim() !== (fav.note ?? '') && onSave({ note: note.trim() })}
            onKeyDown={(e) => e.key === 'Enter' && (e.target as HTMLInputElement).blur()}
            placeholder="The last hour, the soundtrack, playing it with my brother…"
            className="field text-sm"
          />
        </label>
      </div>
    </li>
  )
}

function ChipQuestion({
  title,
  desc,
  subjects,
  chosen,
  busy,
  withCount,
  onToggle,
}: {
  title: string
  desc: string
  subjects: TasteSubject[]
  chosen: (s: TasteSubject) => boolean
  busy: Set<string>
  withCount?: boolean
  onToggle: (s: TasteSubject) => void
}) {
  if (!subjects.length) return null
  return (
    <div className="flex flex-col gap-2">
      <h3 className="m-0 text-[16px] font-bold">{title}</h3>
      <p className="m-0 text-[14.5px] text-muted">{desc}</p>
      <div role="group" aria-label={title} className="flex flex-wrap gap-2">
        {subjects.map((s) => {
          const on = chosen(s)
          return (
            <button
              key={s.about}
              type="button"
              aria-pressed={on}
              disabled={busy.has(`st:${s.about}`)}
              onClick={() => onToggle(s)}
              className={cx(CHIP, on ? CHIP_ON : CHIP_OFF)}
            >
              {s.label}
              {withCount && <span className={cx('ml-1.5 tabular-nums', on ? 'opacity-80' : 'text-dim')}>{s.count}</span>}
            </button>
          )
        })}
      </div>
    </div>
  )
}
