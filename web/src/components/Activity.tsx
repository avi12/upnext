import { useCallback, useEffect, useId, useMemo, useRef, useState, type ReactNode } from 'react'
import { Link } from 'react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api, ApiError } from '../api'
import type { Job, JobsResponse, LaneProgress, ScheduledResponse, TaskId } from '../types'
import { ActivityContext, type ActivityApi } from './useActivity'
import { useToast } from './useToast'
import { Bar, ProgressBar, type BarTone } from './ProgressBar'
import { JobLog, JobResult } from './settings/JobLog'
import { TextButton } from './settings/parts'
import { absoluteTime, cx, loadJson, relativeTime, saveJson } from '../lib/format'
import { endedBadly, inText, isPast, jobName, jobTook, lowerFirst, progressText, troubleText, usageText } from '../lib/jobs'
import { useTaskLabels } from '../lib/useTaskLabels'
import { keys, useJobs } from '../lib/queries'

/** The newest finished run Activity has shown, so the top bar stops flagging its failure. */
const SEEN_KEY = 'upnext.activitySeen'

/** What a finished job changes, refreshed when one ends. */
const CHANGED_BY_JOBS = [
  'stats',
  'profile',
  'games',
  'collections',
  'play-later',
  'game',
  'history',
  'scheduled',
  'jobHistory',
  'aiUsage',
  'backups',
  'second-chances',
  'gems',
  'wishlist',
  'taste',
  'taste-sources',
]

function errText(e: unknown) {
  return e instanceof ApiError ? e.message : String(e)
}

/** Polls the jobs; when any ends, refreshes what it changed and, when it ended badly, says so with a toast that opens its log. */
function useJobWatcher(openLog: (id: number) => void) {
  const qc = useQueryClient()
  const toast = useToast()
  const labels = useTaskLabels()
  const labelsRef = useRef(labels)
  useEffect(() => {
    labelsRef.current = labels
  })
  const jobs = useJobs({
    refetchInterval: (q) => (q.state.data?.current?.status === 'running' ? 1500 : 15_000),
  })
  // The newest finished job seen so far; undefined until the first answer,
  // so the page load that first sees a finished job doesn't toast it.
  const seen = useRef<number | undefined>(undefined)
  useEffect(() => {
    const recent = jobs.data?.recent
    if (!recent) return
    const newest = recent[0]?.id ?? 0
    if (seen.current === undefined || newest <= seen.current) {
      seen.current = Math.max(seen.current ?? 0, newest)
      return
    }
    const since = seen.current
    seen.current = newest
    let ended = recent.filter((j) => j.id > since)
    const replaced = ended.filter((j) => j.kind === 'restore' || j.kind === 'import')
    if (replaced.length) {
      // A restore or an import replaced everything, settings included, and
      // History is the backup's now: only the restore itself is news.
      qc.invalidateQueries()
      ended = replaced
    } else {
      for (const key of CHANGED_BY_JOBS) qc.invalidateQueries({ queryKey: [key] })
    }
    for (const job of replaced.filter((j) => j.status === 'done').reverse()) {
      toast.success(job.kind === 'import' ? 'Database imported' : 'Backup restored', job.summary ?? undefined)
    }
    // A subscription's bars are read again after an AI job, not polled.
    if (ended.some((j) => j.ai_usage)) qc.invalidateQueries({ queryKey: ['aiPlan'] })
    // Oldest first, when several ended between two polls.
    for (const job of ended.filter(endedBadly).reverse()) {
      const name = jobName(job, labelsRef.current)
      const title = troubleText(job, name)
      toast.error(title, job.status === 'failed' ? (job.error ?? undefined) : (job.summary ?? undefined), {
        label: 'Open the log',
        onClick: () => openLog(job.id),
      })
    }
  }, [jobs.data, qc, toast, openLog])
}

export function ActivityProvider({ children }: { children: ReactNode }) {
  const [shown, setShown] = useState<{ logFor: number | null } | null>(null)
  const [seenId, setSeenId] = useState<number>(() => loadJson<number>(SEEN_KEY, 0))
  const open = useCallback((logFor?: number) => setShown({ logFor: logFor ?? null }), [])
  const close = useCallback(() => setShown(null), [])
  const markSeen = useCallback((id: number) => {
    setSeenId((cur) => {
      if (id <= cur) return cur
      saveJson(SEEN_KEY, id)
      return id
    })
  }, [])
  useJobWatcher(open)
  const value = useMemo<ActivityApi>(() => ({ open, close, isOpen: shown != null, seenId }), [open, close, shown, seenId])
  return (
    <ActivityContext.Provider value={value}>
      {children}
      {shown && <ActivitySheet key={shown.logFor ?? 'none'} logFor={shown.logFor} onClose={close} onSeen={markSeen} />}
    </ActivityContext.Provider>
  )
}

function laneTone(lane: LaneProgress): BarTone {
  return lane.state === 'waiting' ? 'warn' : lane.state === 'stopped' ? 'bad' : 'accent'
}

/** What a lane is doing: the title it's on, what it waits for and until when, or why it stopped. */
function laneText(lane: LaneProgress): string {
  switch (lane.state) {
    case 'waiting': {
      const why = lane.note ?? 'Waiting'
      if (lane.retry_at == null) return why
      return isPast(lane.retry_at) ? `${why}, trying again now` : `${why}, trying again ${inText(lane.retry_at)}`
    }
    case 'stopped':
      return lane.note ? `Stopped: ${lowerFirst(lane.note)}` : 'Stopped'
    case 'done':
      return 'Done'
    default:
      return lane.current ?? 'Working'
  }
}

function LaneRow({ lane }: { lane: LaneProgress }) {
  const counts = lane.total > 0 ? `${lane.done.toLocaleString()} of ${lane.total.toLocaleString()}` : lane.done > 0 ? lane.done.toLocaleString() : ''
  return (
    <li className="py-2">
      <div className="flex items-baseline gap-3">
        <span className="min-w-0 flex-1 truncate text-[14px] font-semibold">{lane.name}</span>
        <span className="shrink-0 font-mono text-[11px] text-dim">
          {counts}
          {lane.failed > 0 && <span className="text-warn"> · {lane.failed.toLocaleString()} failed</span>}
        </span>
      </div>
      <Bar done={lane.done} total={lane.total} label={`${lane.name} progress`} tone={laneTone(lane)} className="mt-1.5" />
      <div
        className={cx(
          'mt-1 truncate text-[12.5px]',
          lane.state === 'waiting' ? 'text-warn' : lane.state === 'stopped' ? 'text-bad' : 'text-dim',
        )}
        title={laneText(lane)}
      >
        {laneText(lane)}
      </div>
    </li>
  )
}

/** The running job: its bar, a bar per lane, its latest warnings, Log and Stop. */
function RunningJob({ job, name, logOpen, onLog }: { job: Job; name: string; logOpen: boolean; onLog: () => void }) {
  const qc = useQueryClient()
  const toast = useToast()
  const cancel = useMutation({
    mutationFn: api.cancelJob,
    onSuccess: () => qc.invalidateQueries({ queryKey: keys.jobs }),
    onError: (e) => toast.error("Couldn't stop it", errText(e)),
  })
  const warnings = useQuery({
    queryKey: ['job', job.id, 'warnings'],
    queryFn: () => api.job(job.id, 0, 500, 'warn'),
    refetchInterval: 3000,
  })
  const lines = warnings.data?.log ?? []
  const latest = lines.slice(-3)
  const started = [`started ${relativeTime(job.started_at)}`, job.source === 'terminal' && 'from a terminal', job.source === 'schedule' && 'by its schedule']
    .filter(Boolean)
    .join(' ')
  return (
    <section aria-label={`${name}, running`} className="border-b border-line pb-4">
      <div className="flex items-baseline gap-3">
        <h3 className="m-0 min-w-0 flex-1 text-[16px] font-bold">{name}</h3>
        <span className="shrink-0 text-[12.5px] text-dim" title={absoluteTime(job.started_at)}>
          {started}
        </span>
      </div>
      <p className="m-0 mt-1 text-[13.5px] text-muted">{progressText(job, { source: false, message: job.lanes.length === 0 }) || 'Starting…'}</p>
      <ProgressBar job={job} label={`${name} progress`} />
      {job.lanes.length > 0 && <ul className="m-0 mt-2 list-none p-0">{job.lanes.map((l) => <LaneRow key={l.name} lane={l} />)}</ul>}
      {latest.length > 0 && (
        <div className="mt-2">
          <div className="text-[12.5px] text-dim">
            {lines.length === 1 ? '1 warning' : `${warnings.data?.log_more ? `${lines.length.toLocaleString()}+` : lines.length.toLocaleString()} warnings`}
            {lines.length > latest.length && ', the latest:'}
          </div>
          <ul className="m-0 mt-1 list-none space-y-1 p-0 font-mono text-[11px] leading-relaxed">
            {latest.map((l) => (
              <li key={l.id} className={cx('break-words', l.level === 'error' ? 'text-bad' : 'text-warn')}>
                {l.lane && <span className="text-dim">{l.lane}: </span>}
                {l.message}
              </li>
            ))}
          </ul>
        </div>
      )}
      <div className="mt-3 flex items-center justify-end gap-3">
        <TextButton aria-expanded={logOpen} onClick={onLog}>
          {logOpen ? 'Hide log' : 'Log'}
        </TextButton>
        <button type="button" className="btn-outline bg-transparent" disabled={cancel.isPending || job.cancel_requested} onClick={() => cancel.mutate()}>
          {cancel.isPending || job.cancel_requested ? 'Stopping…' : 'Stop'}
        </button>
      </div>
    </section>
  )
}

/** When nothing runs: when the next scheduled job starts. */
function NextUp({ labels }: { labels: Record<TaskId, string> }) {
  const scheduled = useQuery<ScheduledResponse>({ queryKey: ['scheduled'], queryFn: api.scheduled, staleTime: 30_000 })
  const data = scheduled.data
  let next: string | null = null
  if (data && !data.enabled) next = 'Schedules are off.'
  else if (data?.waiting_for_first_sync) next = 'Nothing runs by itself until the first sync.'
  else if (data) {
    const soonest = data.tasks
      .filter((t) => t.schedule !== 'off' && t.next_run_at != null)
      .sort((a, b) => a.next_run_at! - b.next_run_at!)[0]
    if (soonest) next = `Next: ${labels[soonest.task] ?? soonest.label} ${isPast(soonest.next_run_at!) ? 'soon' : inText(soonest.next_run_at!)}.`
  }
  return (
    <p className="m-0 border-b border-line pb-4 text-[15px] text-muted">
      Nothing is running.{next && <span className="text-dim"> {next}</span>}
    </p>
  )
}

function AiSummary({ onNavigate }: { onNavigate: () => void }) {
  const usage = useQuery({ queryKey: ['aiUsage', 7], queryFn: () => api.aiUsage(7) })
  const u = usage.data
  return (
    <section aria-labelledby="activity-ai" className="pt-5">
      <div className="flex items-baseline gap-3">
        <h3 id="activity-ai" className="m-0 flex-1 text-[15px] font-bold">
          AI usage
        </h3>
        <Link to="/settings#ai-usage" onClick={onNavigate} className="text-[13.5px] text-muted underline decoration-underline underline-offset-4 hover:text-ink">
          More in Settings
        </Link>
      </div>
      {usage.isError && <p className="m-0 mt-1 text-sm text-bad">Couldn't load it: {errText(usage.error)}</p>}
      {u &&
        (u.last_7_days.calls === 0 ? (
          <p className="m-0 mt-1 text-sm text-dim">No AI calls in the last 7 days.</p>
        ) : (
          <dl className="m-0 mt-1 grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-0.5 text-sm">
            {(
              [
                ['Today', u.today],
                ['Last 7 days', u.last_7_days],
              ] as const
            ).map(([label, t]) => (
              <div key={label} className="contents">
                <dt className="text-dim">{label}</dt>
                <dd className="m-0 text-muted">
                  {t.calls === 0 ? 'none' : `${usageText(t)} · ${t.calls.toLocaleString()} ${t.calls === 1 ? 'call' : 'calls'}`}
                </dd>
              </div>
            ))}
          </dl>
        ))}
    </section>
  )
}

/** Activity: a sheet from the right with what runs now, lane by lane, and the last few runs. */
function ActivitySheet({ logFor, onClose, onSeen }: { logFor: number | null; onClose: () => void; onSeen: (id: number) => void }) {
  const titleId = useId()
  const panelRef = useRef<HTMLDivElement>(null)
  const closeRef = useRef<HTMLButtonElement>(null)
  const qc = useQueryClient()
  const labels = useTaskLabels()
  const jobs = useJobs()
  const running = jobs.data?.current?.status === 'running' ? jobs.data.current : null
  const recent = jobs.data?.recent ?? []
  const [logId, setLogId] = useState<number | null>(logFor)
  // The last few runs, and the one whose log was asked for even when it's older.
  const shownRuns = recent.slice(0, 5)
  const asked = logFor != null ? recent.find((j) => j.id === logFor) : undefined
  if (asked && !shownRuns.includes(asked)) shownRuns.push(asked)

  // Seeing Activity clears the top bar's flag on the newest run.
  const newest = recent[0]?.id
  useEffect(() => {
    if (newest != null) onSeen(newest)
  }, [newest, onSeen])

  const onCloseRef = useRef(onClose)
  useEffect(() => {
    onCloseRef.current = onClose
  }, [onClose])

  // As the game sheet: the page behind stays usable, focus starts on Close,
  // Esc or a click on empty page closes, and focus goes back after.
  useEffect(() => {
    const before = document.activeElement as HTMLElement | null
    closeRef.current?.focus()
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        // Before the game sheet's own handler, so Esc closes only the sheet on top.
        e.preventDefault()
        e.stopPropagation()
        onCloseRef.current()
      }
    }
    const onClick = (e: MouseEvent) => {
      const t = e.target as Element | null
      if (!t || !t.isConnected || panelRef.current?.contains(t)) return
      if (t.closest('a, button, input, select, textarea, label, summary, [data-opens-game], [role="button"], [role="dialog"], [role="menu"], [role="status"], [role="alert"]')) return
      onCloseRef.current()
    }
    document.addEventListener('keydown', onKey, true)
    document.addEventListener('click', onClick)
    return () => {
      document.removeEventListener('keydown', onKey, true)
      document.removeEventListener('click', onClick)
      before?.focus?.()
    }
  }, [])

  // A log asked for from a toast scrolls into view once it's there.
  const askedRef = useRef<HTMLLIElement>(null)
  const hasAsked = !!asked
  useEffect(() => {
    if (hasAsked) askedRef.current?.scrollIntoView({ block: 'nearest' })
  }, [hasAsked])

  const onRetried = (j: Job) => {
    qc.setQueryData<JobsResponse>(keys.jobs, (old) => ({ current: j, recent: old?.recent ?? [] }))
    setLogId(null)
  }
  const toggleLog = (id: number) => setLogId((cur) => (cur === id ? null : id))

  return (
    <div className="pointer-events-none fixed inset-x-0 top-(--titlebar-h) bottom-0 z-[95]">
      <div className="pointer-events-none absolute inset-0 hidden md:block" style={{ background: 'var(--sheet-veil)' }} aria-hidden="true" />
      <div
        ref={panelRef}
        role="dialog"
        aria-labelledby={titleId}
        className="sheet-in pointer-events-auto absolute inset-0 overflow-hidden bg-sheet shadow-2xl sm:inset-4 sm:rounded-2xl md:left-auto md:w-[min(540px,calc(100vw-2rem))]"
      >
        <div className="h-full overflow-y-auto overscroll-contain px-6 pt-5 pb-10 sm:px-8">
          <div className="mb-4 flex items-center gap-3">
            <h2 id={titleId} className="m-0 flex-1 font-display text-[24px] font-[750] tracking-[-0.02em]">
              Activity
            </h2>
            <button
              ref={closeRef}
              type="button"
              onClick={onClose}
              aria-label="Close"
              className="-mr-2 flex h-11 w-11 items-center justify-center rounded-full text-muted hover:bg-raised hover:text-ink"
            >
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
                <path d="M6 6l12 12M18 6L6 18" />
              </svg>
            </button>
          </div>

          {jobs.isError && <p className="m-0 border-b border-line pb-4 text-sm text-bad">Can't reach UpNext: {errText(jobs.error)}</p>}
          {running ? (
            <>
              <RunningJob job={running} name={jobName(running, labels)} logOpen={logId === running.id} onLog={() => toggleLog(running.id)} />
              {logId === running.id && <JobLog job={running} name={jobName(running, labels)} onRetried={onRetried} onClose={() => setLogId(null)} />}
            </>
          ) : (
            jobs.data && <NextUp labels={labels} />
          )}

          <section aria-labelledby="activity-recent" className="pt-5">
            <div className="flex items-baseline gap-3">
              <h3 id="activity-recent" className="m-0 flex-1 text-[15px] font-bold">
                Last runs
              </h3>
              <Link to="/settings#jobs" onClick={onClose} className="text-[13.5px] text-muted underline decoration-underline underline-offset-4 hover:text-ink">
                Every job and schedule
              </Link>
            </div>
            {jobs.data && shownRuns.length === 0 && <p className="m-0 mt-1 text-sm text-dim">No runs yet.</p>}
            <ul className="m-0 list-none p-0">
              {shownRuns.map((j) => {
                const name = jobName(j, labels)
                const open = logId === j.id
                return (
                  <li key={j.id} ref={j.id === logFor ? askedRef : undefined} className="border-b border-line">
                    <div className="flex items-start gap-3 py-3">
                      <div className="min-w-0 flex-1">
                        <div className="flex flex-wrap items-baseline gap-x-2">
                          <span className="text-[14.5px] font-semibold">{name}</span>
                          <span className="text-[12.5px] text-dim" title={absoluteTime(j.finished_at ?? j.started_at)}>
                            {relativeTime(j.finished_at ?? j.started_at)} · took {jobTook(j)}
                          </span>
                        </div>
                        <div className="mt-0.5 text-[13.5px] text-muted">
                          <JobResult job={j} onLog={() => setLogId(j.id)} wrap />
                        </div>
                        {j.ai_usage && <div className="mono mt-1">{usageText(j.ai_usage)}</div>}
                      </div>
                      <TextButton aria-expanded={open} aria-label={`${open ? 'Hide the log' : 'Log'} of ${name}`} onClick={() => toggleLog(j.id)}>
                        {open ? 'Hide log' : 'Log'}
                      </TextButton>
                    </div>
                    {open && <JobLog job={j} name={name} onRetried={onRetried} onClose={() => setLogId(null)} />}
                  </li>
                )
              })}
            </ul>
          </section>

          <AiSummary onNavigate={onClose} />
        </div>
      </div>
    </div>
  )
}
