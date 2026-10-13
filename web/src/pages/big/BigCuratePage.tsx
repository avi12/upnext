import { useEffect, useRef, useState } from 'react'
import { useLocation } from 'react-router'
import { ApiError } from '../../api'
import { cx, loadJson, saveJson, upperFirst } from '../../lib/format'
import { GROUPS, REQUEST_KEY, requestText, type Request } from '../../components/bigscreen/request'
import { focusElement } from '../../lib/focus/engine'
import { useCurate } from '../../components/pick/useCurate'
import { wishCover } from '../../components/pick/text'
import { Cover } from '../../components/Cover'
import { BigPickCard } from '../../components/bigscreen/BigPickCard'
import { BigTextField } from '../../components/bigscreen/BigKeyboard'
import { Hint } from '../../components/bigscreen/Glyph'
import { useToast } from '../../components/useToast'

/** Big screen's Curate: your routines and the request as chips, with
 *  an optional line of your own words, then the lineup as large cards. A
 *  request made here counts as "on a controller" for picks, unless the
 *  routine chosen says what it's played on. */
export function BigCuratePage() {
  const toast = useToast()
  const location = useLocation()
  const [request, setRequest] = useState<Request>(() => loadJson<Request>(REQUEST_KEY, { chips: {}, words: '' }))
  const [editing, setEditing] = useState(false)
  const [curatedAt, setCuratedAt] = useState(0)
  const lineupRef = useRef<HTMLDivElement>(null)
  const requestRef = useRef<HTMLDivElement>(null)

  const { last, batch, lineup, routines, routine, setRoutineChoice, m, curate: ask } = useCurate({
    input: 'controller',
    onPicked: () => {
      setEditing(false)
      setCuratedAt(Date.now())
    },
    // Without a lineup on screen the error shows in its place instead.
    onFailed: (e, _request, shown) => {
      if (shown) toast.error("Couldn't curate", e instanceof Error ? e.message.replace(/^no candidates: /, '') : String(e))
    },
  })
  const routineList = routines.data?.routines ?? []
  const routineId = routine?.id ?? null

  const save = (r: Request) => {
    setRequest(r)
    saveJson(REQUEST_KEY, r)
  }

  // useCurate asks once at a time: Enter in the request's field is easy to
  // press again while the AI takes its seconds.
  const curate = (withoutAi: boolean) => ask({ prompt: requestText(request), count: 3, exclude: [], why: [], withoutAi, routineId })

  // Y (or /) from anywhere brings the request up with the focus on it,
  // until the next lineup comes.
  const asked = (location.state as { request?: number } | null)?.request ?? 0
  const showRequest = editing || asked > curatedAt || (!batch && !last.isPending) || m.isPending
  useEffect(() => {
    if (!asked) return
    // The first of the request's own chips, past the routines, not Curate:
    // Y is for changing the request, and the routine that fits now is
    // usually chosen already.
    requestAnimationFrame(() => {
      const chip = requestRef.current?.querySelector<HTMLElement>('[data-focus-group]:not([data-focus-group="routine"]) .big-chip')
      if (chip) focusElement(chip)
    })
  }, [asked])

  // A fresh lineup takes the focus, on its first card.
  const batchId = batch?.batch_id
  useEffect(() => {
    if (!batchId || !m.isSuccess) return
    requestAnimationFrame(() => {
      const first = lineupRef.current?.querySelector<HTMLElement>('[data-focus-first]')
      if (first) focusElement(first)
    })
  }, [batchId, m.isSuccess])

  const picks = batch?.picks ?? []
  const acting = lineup.act.isPending || lineup.too.isPending || lineup.swap.isPending
  const err = !m.isPending && m.error instanceof ApiError ? m.error : null

  return (
    <div className="flex flex-col gap-[1.5rem] pt-[1.5rem]">
      <h1 className="sr-only">Curate</h1>
      {showRequest ? (
        <section ref={requestRef} aria-label="Request" className="flex flex-col gap-[1rem]">
          {routineList.length > 0 && (
            <div role="group" aria-label="Routine" data-focus-group="routine" className="flex flex-wrap items-center gap-[0.5rem]">
              <span className="w-[7.5rem] shrink-0 text-[1rem] text-muted">Routine</span>
              {routineList.map((r) => {
                const on = routine?.id === r.id
                return (
                  // Choosing the chosen routine again picks with none.
                  <button key={r.id} type="button" className="big-chip" aria-pressed={on} onClick={() => setRoutineChoice(on ? null : r.id)}>
                    {r.name}
                    {routines.data?.now === r.id && <span className="ml-[0.4rem] opacity-70">· now</span>}
                  </button>
                )
              })}
            </div>
          )}
          {GROUPS.map((g) => (
            <div key={g.name} role="group" aria-label={g.name} data-focus-group={g.name} className="flex flex-wrap items-center gap-[0.5rem]">
              <span className="w-[7.5rem] shrink-0 text-[1rem] text-muted">{g.name}</span>
              {g.chips.map(([label]) => {
                const on = request.chips[g.name] === label
                return (
                  <button
                    key={label}
                    type="button"
                    className="big-chip"
                    aria-pressed={on}
                    onClick={() => {
                      const chips = { ...request.chips }
                      if (on) delete chips[g.name]
                      else chips[g.name] = label
                      save({ ...request, chips })
                    }}
                  >
                    {label}
                  </button>
                )
              })}
            </div>
          ))}
          <div className="flex flex-wrap items-center gap-[0.75rem]">
            <span className="w-[7.5rem] shrink-0 text-[1rem] text-muted">Or say it</span>
            <BigTextField
              id="big-request"
              label="In your own words"
              value={request.words}
              maxLength={300}
              onChange={(words) => save({ ...request, words })}
              onSubmit={() => curate(false)}
              placeholder="In your own words (optional)"
              className="min-w-[16rem] flex-1"
            />
            <button type="button" className="big-btn-accent min-h-[3.25rem] px-[1.75rem] text-[1.2rem]" data-focus-first={batch ? undefined : ''} disabled={m.isPending} onClick={() => curate(false)}>
              {m.isPending ? 'Curating…' : 'Curate'}
            </button>
          </div>
          <p className="m-0 text-[1rem] text-muted" aria-live="polite">
            {requestText(request) ? `Asking for: ${requestText(request)}` : 'Nothing chosen: UpNext will surprise you.'}
          </p>
        </section>
      ) : batch ? (
        <section aria-label="Request" className="flex flex-wrap items-center gap-[1rem]">
          <p className="m-0 min-w-0 flex-1 text-[1.15rem]">
            <span className="text-muted">Asked for: </span>
            {batch.prompt || 'Surprise me'}
          </p>
          <button type="button" className="big-btn-outline" onClick={() => setEditing(true)}>
            Change the request
          </button>
          <button type="button" className="big-btn-outline" disabled={m.isPending} onClick={() => curate(false)}>
            Curate again
          </button>
          <Hint action="request">Change</Hint>
        </section>
      ) : null}

      <section aria-label="Lineup" className="flex flex-col gap-[1rem]">
        {m.isPending ? (
          <p role="status" className="m-0 text-[1.2rem] text-muted">
            Curating a lineup from your library…
          </p>
        ) : err && !batch ? (
          <div className="flex flex-col items-start gap-[0.75rem]" role="alert">
            <p className="m-0 text-[1.1rem]">{upperFirst(err.message.replace(/^no candidates: /, ''))}</p>
            <div className="flex gap-[0.5rem]">
              <button type="button" className="big-btn-outline" onClick={() => curate(false)}>
                Try again
              </button>
              {err.status !== 400 && (
                <button type="button" className="big-btn-outline" onClick={() => curate(true)}>
                  Pick without AI
                </button>
              )}
            </div>
          </div>
        ) : batch && picks.length > 0 ? (
          <div ref={lineupRef} data-focus-group="lineup" className={cx('grid gap-[2rem]', picks.length >= 3 ? 'grid-cols-3' : picks.length === 2 ? 'grid-cols-2' : 'max-w-[36rem] grid-cols-1')}>
            {picks.map((p, i) =>
              p.owned === false ? (
                <article key={p.id} className="flex min-w-0 flex-col gap-[0.75rem]">
                  <Cover game={wishCover(p.wishlist)} shape="wide" titleSize="text-[1.3rem]" className="w-full rounded-[0.9rem]" />
                  <h2 className="m-0 font-display text-[1.4rem] font-bold">{p.wishlist.name}</h2>
                  <p className="m-0 text-[0.9rem] text-dim">On your wishlist; you don't own it yet.</p>
                  <p className="m-0 line-clamp-4 text-[1rem] leading-[1.45]">{p.reason}</p>
                  {p.alternate && (
                    <button type="button" className="big-btn-ink self-start" data-focus-first={i === 0 ? '' : undefined} disabled={acting} onClick={() => lineup.swap.mutate(p.id)}>
                      Switch with {p.alternate.name}
                    </button>
                  )}
                </article>
              ) : (
                <BigPickCard
                  key={`${batch.batch_id}-${p.id}`}
                  pick={p}
                  number={i + 1}
                  first={i === 0}
                  busy={acting}
                  sifting={lineup.too.isPending && lineup.too.variables?.pick.id === p.id ? lineup.too.variables.word : null}
                  onAct={(action) => lineup.act.mutate({ pick: p, action })}
                  onToo={(word) => lineup.too.mutate({ pick: p, word })}
                  onRate={(verdict, reason) => lineup.rate.mutate({ pick: p, verdict, reason })}
                />
              ),
            )}
            {picks.length < batch.shown && (
              <div className="flex flex-col items-start gap-[0.75rem]" role="status">
                <p className="m-0 text-[1.05rem] text-muted">{lineup.more.isPending ? 'Finding another…' : 'The spares ran out.'}</p>
                {!lineup.more.isPending && (
                  <button type="button" className="big-btn-outline" onClick={() => lineup.more.mutate(batch.batch_id)}>
                    Find another
                  </button>
                )}
              </div>
            )}
          </div>
        ) : batch ? (
          <p className="m-0 text-[1.1rem] text-muted">Nothing came back for that. Try other chips.</p>
        ) : last.isPending ? null : (
          <p className="m-0 text-[1.1rem] text-muted">Pick a few chips, or none, and choose Curate: picks come from games you own and haven't finished.</p>
        )}
      </section>
    </div>
  )
}
