import { useEffect, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { Link } from 'react-router'
import { ApiError } from '../api'
import type { GameView, OwnedPick } from '../types'
import { GameSheet } from '../components/GameSheet'
import { useToast } from '../components/useToast'
import { SOURCES, cx, loadJson, saveJson, upperFirst } from '../lib/format'
import { shortcutBlocked } from '../lib/keys'
import { PlayLaterShelf } from '../components/pick/PlayLaterShelf'
import { EmptySlot, LineupColumn } from '../components/pick/LineupColumn'
import { RollingState } from '../components/pick/RollingState'
import { patchGame } from '../components/pick/useGameActions'
import { useCurate } from '../components/pick/useCurate'
import { WishlistedColumn } from '../components/pick/WishlistedColumn'
import { SuggestionCard } from '../components/pick/SuggestionCard'
import { AskForm } from '../components/pick/AskForm'
import { usePickPrefs } from '../components/pick/usePickPrefs'
import { RollError } from '../components/pick/RollError'
import { LineupHeader } from '../components/pick/LineupHeader'
import { LineupMeta } from '../components/pick/LineupMeta'
import { RoutineChips } from '../components/pick/RoutineChips'
import { useConfig, useSettings } from '../lib/queries'

const CLEARED_KEY = 'upnext.pick.cleared'

/** Column classes for pick `i` of `n`: three per row, thin lines between. */
function columnClass(i: number, n: number): string {
  const cols = Math.min(n, 3)
  const col = i % cols
  const row = Math.floor(i / cols)
  return cx(
    'pt-7 pb-8',
    i > 0 && 'border-t border-line',
    i > 0 && row === 0 && 'md:border-t-0',
    'md:px-7',
    col > 0 && 'md:border-l md:border-l-line',
  )
}

export function PickPage() {
  const qc = useQueryClient()
  const toast = useToast()

  const [draft, setDraft] = useState<string | null>(null)
  const { count, setCount, sources, toggleSource } = usePickPrefs()
  const [clearedId, setClearedId] = useState<string | null>(() => loadJson<string | null>(CLEARED_KEY, null))
  const [selected, setSelected] = useState<number[]>([])
  const [detail, setDetail] = useState<GameView | null>(null)

  const { data: config } = useConfig()

  const pickSources = sources.length === SOURCES.length ? undefined : sources
  // The lineup is the newest pick batch, so it survives a trip to Versus or a reload.
  const { last, batch, lineup, routine, routineChoice, setRoutineChoice, m, curate } = useCurate({
    hide: clearedId,
    sources: pickSources,
    onPicked: () => {
      setSelected([])
      setClearedId(null)
      saveJson(CLEARED_KEY, null)
    },
    onFailed: (e, roll, shown) => {
      // A lineup on screen stays; without one, the error also takes the
      // lineup's place below. Nothing to pick from isn't the AI failing.
      const nothingToPick = e instanceof ApiError && e.status === 400
      if (shown || !nothingToPick) {
        toast.error(
          roll.random ? "Couldn't roll the dice" : shown ? "Couldn't curate again" : "Couldn't curate",
          // The server's "no candidates: " is for API callers, not people.
          upperFirst((e instanceof Error ? e.message : String(e)).replace(/^no candidates: /, '')),
          nothingToPick || roll.withoutAi || roll.random
            ? undefined
            : { label: 'Pick without AI', onClick: () => m.mutate({ ...roll, withoutAi: true }) },
        )
      }
    },
  })
  const prompt = draft ?? batch?.prompt ?? ''

  // Reasons given with "Not these…" or "It's too ___" since the last fresh request; each Curate again adds to them.
  const [avoid, setAvoid] = useState<string[]>([])
  const [ownReason, setOwnReason] = useState<string | null>(null)
  const steer = (reason: string) => setAvoid((a) => [...a.filter((x) => x !== reason), reason].slice(-5))

  const routineId = routine?.id ?? null

  const run = (exclude: number[] = [], why: string[] = [], asked = prompt) => {
    if (m.isPending) return
    const p = asked.trim()
    setAvoid(why)
    curate({ prompt: p.toLowerCase() === 'surprise me' ? '' : p, count, exclude, why, withoutAi: false, routineId })
  }
  // Roll the dice: one game off your radar, by chance. The request
  // and the reasons given don't steer it, so neither is sent.
  const dice = (exclude: number[] = []) => {
    if (m.isPending) return
    setAvoid([])
    // Chance doesn't read a routine either, and draws one game whatever the count says.
    curate({ prompt: '', count: 1, exclude, why: [], withoutAi: false, routineId: null, random: true })
  }
  // Curate around the game the dice drew: games like it, from the AI as usual.
  const curateAround = () => {
    const g = batch?.picks.find((p) => p.slot != null && p.game)
    if (!g?.game) return
    const p = `Games like ${g.game.name}`
    setDraft(p)
    run([g.id], [], p)
  }
  const shownIds = () => (batch ? [...batch.picks, ...(batch.removed ?? [])].map((p) => p.id) : [])
  const reroll = (reason?: string) => {
    // A drawn game's Another (and R) rolls again, without the games drawn so far.
    const r = reason?.trim()
    if (batch?.mode === 'random') dice(shownIds())
    else run(shownIds(), r ? [...avoid.filter((a) => a !== r), r].slice(-5) : avoid)
    setOwnReason(null)
  }
  const clear = () => {
    if (!batch) return
    setClearedId(batch.batch_id)
    saveJson(CLEARED_KEY, batch.batch_id)
    setSelected([])
  }

  // R rolls again while a lineup is showing.
  useEffect(() => {
    if (!batch || m.isPending || detail) return
    const onKey = (e: KeyboardEvent) => {
      if ((e.key === 'r' || e.key === 'R') && !shortcutBlocked(e)) {
        e.preventDefault()
        reroll()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  })

  const onUpdated = (g: GameView) => {
    patchGame(qc, g)
    setDetail((d) => (d && d.id === g.id ? g : d))
  }

  const toggleCompare = (id: number, on: boolean) =>
    setSelected((cur) => (on ? [...cur.filter((x) => x !== id), id].slice(-2) : cur.filter((x) => x !== id)))

  const picks = batch?.picks ?? []
  // The lineup's slots in order; an empty one waits for new spares.
  const slots = batch ? Array.from({ length: Math.max(batch.shown ?? 0, picks.length) }, (_, i) => picks.find((p, j) => (p.slot ?? j) === i) ?? null) : []
  const compared = selected.map((id) => picks.find((p) => p.id === id)).filter((p): p is OwnedPick => p != null && p.owned !== false)
  const acting = lineup.act.isPending || lineup.too.isPending || lineup.swap.isPending
  // The dice are instant: the lineup on screen stays until the draw is back.
  const dicing = m.isPending && m.variables?.random === true
  const curating = m.isPending && !dicing
  // Turning the wishlist off hides a lineup's suggestion too.
  const wishOn = useSettings().data?.wishlist !== false
  const err: { status: number; message: string } | null =
    !m.isPending && m.error ? (m.error instanceof ApiError ? m.error : { status: 0, message: String(m.error.message ?? m.error) }) : null
  // For the tour, which shows an example lineup when there's none yet.
  const lineupState = curating
    ? 'rolling'
    : err && !batch
      ? 'error'
      : batch && slots.length > 0
        ? 'filled'
        : last.isPending
          ? 'loading'
          : 'empty'

  return (
    <div className="px-4 pb-16 sm:px-10">
      <AskForm
        prompt={prompt}
        onDraft={setDraft}
        onRoll={() => run()}
        onDice={() => dice()}
        pending={m.isPending}
        dicing={dicing}
        count={count}
        setCount={setCount}
        sources={sources}
        toggleSource={toggleSource}
        config={config}
      />

      <RoutineChips choice={routineChoice} onChoose={setRoutineChoice} />

      <PlayLaterShelf onOpen={setDetail} />

      <section aria-label="Lineup" data-tour="lineup" data-state={lineupState} className="flex flex-col pt-12">
        {curating ? (
          // When it was asked, as the mutation noted it.
          <RollingState startedAt={m.submittedAt} count={count} />
        ) : err && !batch ? (
          <RollError
            err={err}
            dice={m.variables?.random === true}
            onRetry={() => (m.variables?.random ? dice() : run())}
            onWithoutAi={m.variables && !m.variables.withoutAi && !m.variables.random ? () => m.mutate({ ...m.variables!, withoutAi: true }) : null}
          />
        ) : batch && slots.length > 0 ? (
          <>
            <LineupHeader
              batch={batch}
              avoid={avoid}
              onForget={() => setAvoid([])}
              ownReason={ownReason}
              setOwnReason={setOwnReason}
              reroll={reroll}
              clear={clear}
            />

            <div
              data-tour="picks"
              className={cx(
                'grid grid-cols-1 md:-mx-7',
                slots.length === 2 && 'md:grid-cols-2',
                slots.length >= 3 && 'md:grid-cols-3',
                slots.length === 1 && 'md:max-w-[calc(42rem+3.5rem)]',
              )}
            >
              {slots.map((p, i) =>
                p?.owned === false ? (
                  <WishlistedColumn
                    key={`${batch.batch_id}-${p.id}`}
                    pick={p}
                    number={i + 1}
                    busy={acting}
                    switching={lineup.swap.isPending && lineup.swap.variables === p.id}
                    onSwitch={() => lineup.swap.mutate(p.id)}
                    className={columnClass(i, slots.length)}
                    style={{ animationDelay: `${i * 70}ms` }}
                  />
                ) : p ? (
                  <LineupColumn
                    key={`${batch.batch_id}-${p.id}`}
                    pick={p}
                    number={i + 1}
                    compared={selected.includes(p.id)}
                    onCompare={(on) => toggleCompare(p.id, on)}
                    onOpen={setDetail}
                    showLinux={config?.linux_active === true}
                    busy={acting}
                    sifting={lineup.too.isPending && lineup.too.variables?.pick.id === p.id ? lineup.too.variables.word : null}
                    onAct={(action, text, reasons) => {
                      if (action !== 'chosen') setSelected((cur) => cur.filter((x) => x !== p.id))
                      lineup.act.mutate({ pick: p, action, text, reasons })
                    }}
                    onToo={(word) => {
                      setSelected((cur) => cur.filter((x) => x !== p.id))
                      steer(`Too ${word.replace(/^too\s+/i, '')}`)
                      lineup.too.mutate({ pick: p, word })
                    }}
                    rating={lineup.rate.isPending && lineup.rate.variables?.pick.id === p.id}
                    onRate={(verdict, reason) => lineup.rate.mutate({ pick: p, verdict, reason })}
                    dice={batch.mode === 'random' ? { onAnother: () => reroll(), onCurateAround: curateAround } : undefined}
                    className={columnClass(i, slots.length)}
                    style={{ animationDelay: `${i * 70}ms` }}
                  />
                ) : (
                  <EmptySlot
                    key={`${batch.batch_id}-empty-${i}`}
                    finding={lineup.more.isPending}
                    failed={lineup.more.isError}
                    onFind={() => lineup.more.mutate(batch.batch_id)}
                    className={columnClass(i, slots.length)}
                  />
                ),
              )}
            </div>

            {batch.suggestion && wishOn && <SuggestionCard key={batch.batch_id} batchId={batch.batch_id} suggestion={batch.suggestion} />}

            {compared.length === 2 && (
              <div className="sticky bottom-4 z-20 mt-2 flex flex-col gap-3 rounded-xl bg-raised px-5 py-4 shadow-[0_8px_28px_rgb(0_0_0/0.28)] sm:flex-row sm:items-center sm:justify-between">
                <div className="min-w-0 text-[15.5px]">
                  <span className="font-bold">{compared[0].game.name}</span> <span className="text-dim">vs</span>{' '}
                  <span className="font-bold">{compared[1].game.name}</span>
                </div>
                <Link
                  to={`/versus?${new URLSearchParams({ a: String(compared[0].id), b: String(compared[1].id), prompt: batch.prompt })}`}
                  className="btn-accent shrink-0 px-5 text-base"
                >
                  Compare side by side →
                </Link>
              </div>
            )}

            <LineupMeta batch={batch} />
          </>
        ) : batch ? (
          <p className="border-t border-line pt-7 text-[15px] text-muted">Nothing came back for that. Try asking for something a little different.</p>
        ) : last.isLoading ? null : last.isError ? (
          <div className="flex flex-col items-start gap-3 border-t border-line pt-7" role="alert">
            <p className="m-0 text-[15px] text-ink">Couldn't load your last lineup. Is UpNext still running?</p>
            <button type="button" className="btn-outline" onClick={() => last.refetch()} disabled={last.isFetching}>
              {last.isFetching ? 'Trying again…' : 'Try again'}
            </button>
          </div>
        ) : (
          <p className="border-t border-line pt-7 text-[15px] text-muted">
            You haven’t curated yet. Say what you’re up for and hit Curate: picks come from games you own and haven’t finished, and the ones you play now and then.
          </p>
        )}
      </section>

      <GameSheet game={detail} onClose={() => setDetail(null)} onUpdated={onUpdated} />
    </div>
  )
}
