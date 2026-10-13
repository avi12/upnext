import { useCallback, useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { Link } from 'react-router'
import { api, ApiError } from '../api'
import type { GameView, HabitKey, JobsResponse, StatementKind, TasteFavourite, TastePattern, TasteSourceId, TasteStatement } from '../types'
import { GameSheet } from '../components/GameSheet'
import { Skeleton } from '../components/Skeleton'
import { useToast } from '../components/useToast'
import { useSettingsSave } from '../components/settings/useSettingsSave'
import { SourcesSection } from '../components/profile/Sources'
import { FavouritesSection } from '../components/profile/Favourites'
import { StatementsSection } from '../components/profile/Statements'
import { PatternsSection } from '../components/profile/Patterns'
import { HabitsSection } from '../components/profile/Habits'
import { RoutinesSection } from '../components/profile/Routines'
import { KINDS } from '../components/profile/words'
import { NOT_PRIVATE, possessive, usePlayers } from '../lib/players'
import { keys, useJobs } from '../lib/queries'

function errText(e: unknown) {
  return e instanceof ApiError ? e.message : String(e)
}

/** The taste profile: everything UpNext knows about you as a player,
 *  where it came from, and a way to correct or delete any of it. */
export function ProfilePage() {
  const qc = useQueryClient()
  const toast = useToast()
  const saveSetting = useSettingsSave()
  const taste = useQuery({ queryKey: ['taste'], queryFn: api.taste })
  const sources = useQuery({ queryKey: ['taste-sources'], queryFn: api.tasteSources })
  // Polled by Activity's job watcher; this only reads it.
  const jobs = useJobs()
  const [sheet, setSheet] = useState<GameView | null>(null)
  const updating = jobs.data?.current?.status === 'running' && jobs.data.current.task === 'taste'

  /** Resolves once the profile has been read again, so an editor closes on the new text. */
  const refresh = useCallback(
    () => Promise.all([qc.invalidateQueries({ queryKey: ['taste'] }), qc.invalidateQueries({ queryKey: ['taste-sources'] })]),
    [qc],
  )

  /** Run a change, then refresh; a failure is a toast. Resolves to whether it worked. */
  const run = async (what: string, f: () => Promise<unknown>, done?: string): Promise<boolean> => {
    try {
      await f()
      if (done) toast.success(done)
      return true
    } catch (e) {
      toast.error(what, errText(e))
      return false
    } finally {
      await refresh()
    }
  }

  const openGame = async (id: number) => {
    try {
      setSheet(await api.game(id))
    } catch (e) {
      toast.error("Couldn't open that game", errText(e))
    }
  }

  const addFavourite = async (g: GameView): Promise<number | null> => {
    try {
      const row = await api.tasteState({ kind: 'favourite', title_id: g.id })
      await refresh()
      return row.id
    } catch (e) {
      toast.error(`Couldn't add ${g.name}`, errText(e))
      return null
    }
  }

  const saveFavourite = (f: TasteFavourite, why: string[], note: string) =>
    run(`Couldn't save why you love ${f.game.name}`, () => api.tasteEdit(f.id, { why, note: note.trim() }))

  const removeFavourite = (f: TasteFavourite) =>
    void run(`Couldn't remove ${f.game.name}`, async () => {
      await api.tasteDelete(f.id)
      toast.success(`${f.game.name} is no longer a favourite`, undefined, {
        label: 'Undo',
        onClick: () =>
          void run(`Couldn't put ${f.game.name} back`, () =>
            api.tasteState({ kind: 'favourite', title_id: f.game.id, why: f.why.map((c) => c.key), note: f.note }),
          ),
      })
    })

  const kindLabel = (k: StatementKind) => KINDS.find((x) => x.value === k)?.label.toLowerCase() ?? k

  const addStatement = (kind: StatementKind, about: string) =>
    run("Couldn't add that", () => api.tasteState({ kind, about }), undefined)

  const moveStatement = (s: TasteStatement, kind: StatementKind) =>
    void run(`Couldn't change ${s.label}`, () => api.tasteEdit(s.id, { kind }), `${s.label} is a ${kindLabel(kind)} now`)

  const removeStatement = (s: TasteStatement) =>
    void run(`Couldn't remove ${s.label}`, async () => {
      await api.tasteDelete(s.id)
      toast.success(`Removed ${s.label}`, undefined, {
        label: 'Undo',
        onClick: () => void run(`Couldn't put ${s.label} back`, () => api.tasteState({ kind: s.kind, about: s.about, note: s.note })),
      })
    })

  const answerPattern = (p: TastePattern, answer: 'right' | 'not_really' | 'show') => {
    if (answer === 'right') {
      const kind = p.sense === 'drift' ? 'dislike' : 'like'
      void run("Couldn't save that", () => api.tasteEdit(p.id, { confirm: true }), `Saved as a ${kind}`)
    } else if (answer === 'not_really') {
      void run("Couldn't hide it", async () => {
        await api.tasteEdit(p.id, { hidden: true })
        toast.success('Hidden, and it won’t come back', undefined, {
          label: 'Undo',
          onClick: () => void run("Couldn't show it again", () => api.tasteEdit(p.id, { hidden: false })),
        })
      })
    } else {
      void run("Couldn't show it again", () => api.tasteEdit(p.id, { hidden: false }))
    }
  }

  const countAgain = async () => {
    try {
      const job = await api.runTask('taste')
      qc.setQueryData<JobsResponse>(keys.jobs, (old) => ({ current: job, recent: old?.recent ?? [] }))
    } catch (e) {
      toast.error("Couldn't count the patterns again", errText(e))
    }
  }

  const setHabit = (about: HabitKey, value: number | string[] | null) => {
    const habit = taste.data?.habits.find((h) => h.about === about)
    if (value == null) {
      if (habit) void run("Couldn't clear that", () => api.tasteDelete(habit.id))
      return
    }
    void run("Couldn't save that", () => api.tasteState({ kind: 'context', about, why: value, note: habit?.note ?? null }))
  }

  const switchDiary = async (on: boolean) => {
    try {
      const r = await api.tasteDiary(on)
      qc.invalidateQueries({ queryKey: keys.settings })
      if (on && r.diary.error) toast.error('The play diary is on, but Steam didn’t answer', r.diary.error)
      else toast.success(on ? 'The play diary is on' : 'The play diary is off', on ? undefined : 'What it holds stays until you delete it.')
    } catch (e) {
      toast.error(on ? "Couldn't turn the play diary on" : "Couldn't turn the play diary off", errText(e))
    } finally {
      await refresh()
    }
  }

  const deleteSource = async (source: TasteSourceId) => {
    const what: Partial<Record<TasteSourceId, [string, string]>> = {
      statuses: ['status', 'statuses'],
      reactions: ['reaction', 'reactions'],
      answers: ['thing you said', 'things you said'],
      diary: ['row', 'rows'],
      wishlist: ['game', 'games'],
    }
    await run(`Couldn't delete it`, async () => {
      const r = await api.tasteDeleteSource(source)
      const [one, many] = what[source] ?? ['row', 'rows']
      toast.success('Deleted', `${r.deleted.toLocaleString()} ${r.deleted === 1 ? one : many}`)
      for (const key of ['games', 'stats', 'game', 'history', 'second-chances', 'wishlist']) qc.invalidateQueries({ queryKey: [key] })
    })
  }

  const d = taste.data
  const { multi, current } = usePlayers()
  return (
    // The Settings width, with sections side by side from 1280 px, so the
    // page isn't one long column on a wide screen.
    <div className="mx-auto flex max-w-[1440px] flex-col gap-12 px-4 pt-8 pb-24 sm:px-10">
      {/* A heading and one line, with its two actions beside them. */}
      <header className="flex flex-wrap items-start justify-between gap-x-10 gap-y-4">
        <div className="flex max-w-[760px] flex-col gap-2 text-[17px] leading-[1.6] text-muted">
          <h1 className="m-0 font-display text-[32px] leading-[1.1] font-[750] tracking-[-0.02em] text-ink">
            {multi && current ? `${possessive(current.name)} profile` : 'Your profile'}
          </h1>
          <p className="m-0">What UpNext knows about you as a player, and where it came from. Change or delete any of it.</p>
          {multi && <p className="m-0 text-[14px] text-dim">{NOT_PRIVATE}</p>}
          <details className="text-[15px]">
            <summary className="cursor-pointer text-ink underline decoration-underline underline-offset-4">How picks use this</summary>
            <p className="m-0 mt-2">
              Hard nos never come up; likes and games like your favourites rank higher; dislikes and the kinds of game you drift away from rank
              a little lower. It’s kept on this computer, and a short summary of it goes to your AI with each pick.
            </p>
          </details>
        </div>
        <div className="flex flex-wrap gap-3">
          <Link to="/profile/questions" className="btn-accent h-11" title="Your favourites from your most played, then a few quick questions, each one skippable">
            Tune your profile
          </Link>
          <a href={api.tasteExportUrl} className="btn-outline h-11" download>
            Export as JSON
          </a>
        </div>
      </header>

      {taste.isError ? (
        <div role="alert">
          <p className="m-0 text-[15px] text-ink">Couldn't load your profile.</p>
          <p className="mono mt-1">{(taste.error as Error).message}</p>
          <button type="button" className="btn-outline mt-4 h-11" onClick={() => taste.refetch()} disabled={taste.isFetching}>
            {taste.isFetching ? 'Trying again…' : 'Try again'}
          </button>
        </div>
      ) : !d ? (
        <div className="space-y-4" aria-busy="true">
          <Skeleton className="h-6 w-56" />
          <Skeleton className="h-10 w-full" />
          <Skeleton className="h-10 w-full" />
          <Skeleton className="h-10 w-2/3" />
        </div>
      ) : (
        <>
          <SourcesSection
            data={sources.data}
            onDelete={deleteSource}
            onSeeAnswers={() => document.getElementById('favourites')?.scrollIntoView({ block: 'start' })}
            onDiary={switchDiary}
          />
          <FavouritesSection
            favourites={d.favourites}
            suggestions={d.suggestions}
            onOpen={setSheet}
            onAdd={addFavourite}
            onSave={saveFavourite}
            onRemove={removeFavourite}
          />
          <StatementsSection statements={d.statements} subjects={d.subjects} onAdd={addStatement} onMove={moveStatement} onRemove={removeStatement} />
          <div className="grid grid-cols-1 gap-12 xl:grid-cols-2 xl:gap-x-14">
            <PatternsSection
              data={d}
              updating={updating}
              onUpdate={() => void countAgain()}
              onAnswer={answerPattern}
              onOpen={(id) => void openGame(id)}
              onRule={(patch) => void saveSetting(patch, 'Counting the patterns again').then((err) => !err && void countAgain())}
            />
            <HabitsSection habits={d.habits} diary={d.diary} diaryOn={sources.data?.diary.on ?? false} onSet={setHabit} />
          </div>
          <RoutinesSection />
        </>
      )}

      <GameSheet game={sheet} onClose={() => setSheet(null)} onUpdated={(g) => setSheet((cur) => (cur && cur.id === g.id ? g : cur))} />
    </div>
  )
}
