import { useEffect, useRef } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useNavigate } from 'react-router'
import { api, ApiError } from '../../api'
import type { Job, ScheduleChange, ScheduledResponse, SetupBackgroundAsk, SetupInfo, TaskId } from '../../types'
import { plural, storeName } from '../../lib/format'
import { progressText } from '../../lib/jobs'
import { BACKEND_LABEL } from '../../lib/ai'
import { ScheduleCell } from '../settings/Jobs'
import { ProgressBar } from '../ProgressBar'
import { TasteQuestions } from '../questions/TasteQuestions'
import { useToast } from '../useToast'
import { StepButtons, StepHead, type StepProps } from './parts'
import { SharedDataOption } from './SharedDataOption'
import { VerdictsOption } from './VerdictsOption'
import { keys, useJobs } from '../../lib/queries'

function errText(e: unknown) {
  return e instanceof ApiError ? e.message : String(e)
}

/** The running job, and the newest finished one of `task`. When a job
 *  starts or ends, the wizard reads the library again. */
function useTaskJob(task: TaskId) {
  const qc = useQueryClient()
  const jobs = useJobs({
    refetchInterval: (q) => (q.state.data?.current?.status === 'running' ? 1000 : 5000),
  })
  const current = jobs.data?.current?.status === 'running' ? jobs.data.current : null
  const mine = current?.task === task ? current : null
  const last = jobs.data?.recent.find((j) => j.task === task && j.status !== 'running') ?? null
  const runningId = current?.id ?? null
  const seen = useRef(runningId)
  useEffect(() => {
    if (seen.current !== runningId) {
      void qc.invalidateQueries({ queryKey: ['setup'] })
      void qc.invalidateQueries({ queryKey: ['scheduled'] })
    }
    seen.current = runningId
  }, [runningId, qc])
  return { current, mine, last }
}

/** Ask the wizard's background work for a sync or Fetch details:
 *  it starts once the running job ends, instead of being refused. */
function useBackground() {
  const qc = useQueryClient()
  const toast = useToast()
  return useMutation({
    mutationFn: (ask: SetupBackgroundAsk) => api.setupBackground(ask),
    onSuccess: (bg) => {
      qc.setQueryData<SetupInfo>(['setup'], (old) => old && { ...old, background: bg })
      void qc.invalidateQueries({ queryKey: ['setup'] })
      void qc.invalidateQueries({ queryKey: keys.jobs })
    },
    onError: (e) => toast.error("Couldn't start it", errText(e)),
  })
}

/** What the running job is, for "once the running … ends". */
function jobNoun(job: Job | null) {
  if (!job) return 'job'
  return job.task === 'details' ? 'Fetch details' : job.kind
}

/** 8. The first sync, which started in the background when the person
 *  left the stores step: its progress, then what came in per store.
 *  Next never waits for it. */
export function SyncStep({ setup, next, back, go }: StepProps) {
  const { current, mine, last } = useTaskJob('sync')
  const run = useBackground()
  const bg = setup.background
  const lib = setup.library
  const synced = lib.titles > 0
  const failed = last && !mine && !bg.sync_waiting && (last.status === 'failed' || last.status === 'cancelled') ? last : null
  const stores = Object.entries(lib.by_source)
    .filter(([, n]) => n > 0)
    .sort((a, b) => b[1] - a[1])
  return (
    <section aria-labelledby="step-sync" className="flex flex-col gap-6">
      <StepHead id="step-sync" title="First sync">
        <p className="m-0">
          A sync reads every library you connected. The first one starts when you leave the stores step and runs in the
          background, so you can go on while it does; after that it runs by itself once a day while UpNext runs.
        </p>
      </StepHead>
      <div aria-live="polite" className="flex max-w-[640px] flex-col gap-3">
        {mine ? (
          <div>
            <span className="text-[15px] text-muted">Syncing… {progressText(mine)}</span>
            <ProgressBar job={mine} label="Sync progress" />
            <p className="m-0 mt-2 text-[13.5px] text-dim">It carries on in the background: Next doesn’t wait for it.</p>
          </div>
        ) : bg.sync_waiting ? (
          <p className="m-0 text-[15px] text-muted">A sync starts once the running {jobNoun(current)} ends.</p>
        ) : synced ? (
          <>
            <p className="m-0 text-[17px] text-ink">
              {plural(lib.titles, 'game')}
              {lib.multi_store > 0 && `, ${plural(lib.multi_store, 'game')} on two or more stores`}.
            </p>
            {stores.length > 0 && (
              <ul className="m-0 flex list-none flex-col gap-1 p-0 text-[15px] text-muted">
                {stores.map(([store, n]) => (
                  <li key={store}>
                    {storeName(store)}: {n.toLocaleString()}
                  </li>
                ))}
              </ul>
            )}
          </>
        ) : (
          <p className="m-0 text-[15px] text-muted">No games yet.</p>
        )}
        {failed && (
          <p className="m-0 text-[14px] text-bad" role="alert">
            The last sync {failed.status === 'cancelled' ? 'was stopped' : `failed: ${failed.error ?? 'no reason given'}`}. The Steam
            and stores steps have what each library needs.
          </p>
        )}
        {last?.summary && !mine && <p className="m-0 text-[13.5px] text-dim">Last sync: {last.summary}</p>}
        <SharedDataOption seed={bg.seed} empty={!synced} />
        <VerdictsOption />
        {!mine && !bg.sync_waiting && (
          <div className="flex flex-wrap items-center gap-3">
            <button
              type="button"
              className={synced ? 'btn-outline' : 'btn-accent'}
              disabled={run.isPending}
              onClick={() => run.mutate({ sync: 'again' })}
            >
              {synced ? 'Sync again' : 'Sync now'}
            </button>
            {!synced && (
              <button type="button" className="btn-quiet text-sm underline underline-offset-4" onClick={() => go('steam')}>
                Back to the stores
              </button>
            )}
          </div>
        )}
      </div>
      <StepButtons onNext={next} onBack={back} onSkip={synced || mine || bg.sync_waiting ? undefined : next} />
    </section>
  )
}

/** 9. Fetch details in the background, with an estimate, and the other jobs' schedules. */
export function DetailsStep({ setup, s, next, back }: StepProps) {
  const qc = useQueryClient()
  const toast = useToast()
  const { current, mine } = useTaskJob('details')
  const run = useBackground()
  const waiting = setup.background.details_waiting
  const scheduled = useQuery({ queryKey: ['scheduled'], queryFn: api.scheduled })
  const change = useMutation({
    mutationFn: ({ task, change }: { task: TaskId; change: ScheduleChange }) => api.setSchedule(task, change),
    onSuccess: (data) => qc.setQueryData<ScheduledResponse>(['scheduled'], data),
    onError: (e) => toast.error("Couldn't change that", errText(e)),
  })
  const lib = setup.library
  const minutes = Math.max(1, Math.round(lib.details_minutes))
  const rows = (scheduled.data?.tasks ?? []).filter((r) => !r.uses_ai && (s.wishlist || r.task !== 'wishlist'))
  const labels = Object.fromEntries((scheduled.data?.tasks ?? []).map((r) => [r.task, r.label]))
  return (
    <section aria-labelledby="step-details" className="flex flex-col gap-6">
      <StepHead id="step-details" title="Details">
        <p className="m-0">
          Fetching details reads each game's store page, reviews and tags, which picks are made from.
          It runs in the background, so you can carry on
          {lib.unenriched > 0 ? `: about ${plural(minutes, 'minute')} for ${plural(lib.unenriched, 'game')}` : ''}.
          {s.auto_sync ? ' It starts by itself after each sync.' : ''}
          {setup.environment.mode === 'app' && ' Keep the app open until it’s done: closing it stops the fetch, which carries on the next time you open it.'}
        </p>
      </StepHead>
      <div aria-live="polite" className="max-w-[640px]">
        {mine ? (
          <>
            <span className="text-[15px] text-muted">Fetching details… {progressText(mine)}</span>
            <ProgressBar job={mine} label="Details progress" />
          </>
        ) : waiting ? (
          <p className="m-0 text-[15px] text-muted">Fetching details starts once the running {jobNoun(current)} ends.</p>
        ) : lib.titles === 0 ? (
          <p className="m-0 text-[15px] text-muted">There are no games to fetch details for until a sync brings some.</p>
        ) : lib.unenriched === 0 ? (
          <p className="m-0 text-[15px] text-good">Every game has its details.</p>
        ) : (
          <button type="button" className="btn-accent" disabled={run.isPending} onClick={() => run.mutate({ details: 'after_sync' })}>
            Fetch details
          </button>
        )}
      </div>
      {rows.length > 0 && (
        <div className="flex flex-col gap-2">
          <h2 className="m-0 text-[16px] font-bold">What else runs by itself</h2>
          <p className="m-0 max-w-[720px] text-[14px] text-dim">
            {s.auto_sync ? 'While UpNext runs.' : 'Schedules are off (Settings › Jobs, “Run jobs on their schedules”).'} Change any of
            them here or later in Settings › Jobs.
          </p>
          <ul className="m-0 flex list-none flex-col p-0">
            {rows.map((row) => (
              <li key={row.task} className="flex flex-wrap items-baseline gap-x-3 gap-y-1 border-b border-line py-2.5 text-[15px]">
                <span className="w-[200px] shrink-0 font-semibold">{row.label}</span>
                <span className="min-w-0">
                  <ScheduleCell
                    row={row}
                    rows={scheduled.data?.tasks ?? []}
                    labels={labels}
                    here={!!s.listen.this_computer}
                    onChange={(c) => change.mutate({ task: row.task, change: c })}
                  />
                </span>
              </li>
            ))}
          </ul>
        </div>
      )}
      <StepButtons onNext={next} onBack={back} />
    </section>
  )
}

/** 10. The first-run questions: favourites, why, and quick questions. */
export function TasteStep({ setup, next, back }: StepProps) {
  const navigate = useNavigate()
  if (setup.library.titles === 0) {
    return (
      <section aria-labelledby="step-taste" className="flex flex-col gap-6">
        <StepHead id="step-taste" title="Your taste">
          <p className="m-0">
            A couple of questions about the games you love make the first picks fit you. They need your library, so they wait for a
            sync; "Tune your profile" on your profile page asks them any time.
          </p>
        </StepHead>
        <StepButtons onNext={next} onBack={back} />
      </section>
    )
  }
  return (
    <section aria-labelledby="step-taste" className="flex flex-col gap-6">
      <StepHead id="step-taste" title="Your taste">
        <p className="m-0">
          A couple of minutes, and the first picks fit you better, especially when you ask for “anything”. Every answer saves as you
          give it; skip any of them.
        </p>
      </StepHead>
      <TasteQuestions onFinish={(to) => navigate(to === 'pick' ? '/' : '/profile')} onEnd={next} onBack={back} onSkip={next} />
    </section>
  )
}

/** 11. Done: what's set up, and where to go. */
export function DoneStep({ setup, back }: StepProps) {
  const navigate = useNavigate()
  const qc = useQueryClient()
  // Turning the tour down here ends its offer too; Settings › Accounts keeps it.
  const explore = async () => {
    try {
      qc.setQueryData(keys.setupState, await api.updateSetup({ tour: 'skipped' }))
    } catch {
      // The offer stays; nothing else depends on it.
    }
    navigate('/library')
  }
  const lib = setup.library
  const ai = setup.ai
  const running = lib.job?.status === 'running' ? lib.job : null
  return (
    <section aria-labelledby="step-done" className="flex flex-col gap-6">
      <StepHead id="step-done" title="You're set up">
        <ul className="m-0 flex list-disc flex-col gap-1 pl-5">
          <li>{lib.titles ? `${plural(lib.titles, 'game')} in your library.` : 'No games yet: a sync from Settings › Jobs brings them.'}</li>
          <li>
            {ai.available
              ? `Picks use ${BACKEND_LABEL[ai.backend] ?? ai.backend}.`
              : 'No AI yet, so picks are ranked by UpNext alone. Settings › Picks adds one.'}
          </li>
          {running && <li>{running.task === 'details' ? 'Details are still coming in, in the background.' : 'A job is still running in the background.'}</li>}
        </ul>
        <p className="m-0">
          Everything here is in Settings, and Settings › Setup has <b className="font-semibold text-ink">Restart the setup wizard</b> and
          the tour.
        </p>
      </StepHead>
      <div className="flex flex-wrap items-center gap-3 border-t border-line pt-5">
        <button type="button" className="btn-accent" onClick={() => navigate('/?tour=1')}>
          Show me around (2 minutes)
        </button>
        <button type="button" className="btn-outline" onClick={() => void explore()}>
          I'll explore
        </button>
        <button type="button" className="btn-quiet text-sm underline underline-offset-4" onClick={back}>
          Back
        </button>
      </div>
    </section>
  )
}
