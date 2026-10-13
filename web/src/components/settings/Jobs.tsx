import { Fragment, useEffect, useRef, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api, ApiError } from '../../api'
import type { Job, JobsResponse, ScheduleChange, ScheduledResponse, ScheduledTask, Settings, TaskId } from '../../types'
import { useToast } from '../useToast'
import { useActivity } from '../useActivity'
import { absoluteTime, cx, relativeTime, upperFirst } from '../../lib/format'
import { canFollow, inText, isPast, jobName, labelMid, jobTook, nextRunNoun, progressText, tokensText } from '../../lib/jobs'
import { SentenceSelect, type SentenceOption } from '../library/SentenceSelect'
import { JobLog, JobResult } from './JobLog'
import { ProgressBar } from '../ProgressBar'
import { Checkbox, Section, Segmented, TextButton } from './parts'
import { keys, useGog, useJobs } from '../../lib/queries'

type View = 'scheduled' | 'history'

const EVERY: SentenceOption[] = [
  { value: 'off', label: 'Off' },
  { value: 'daily', label: 'Daily' },
  { value: 'weekly', label: 'Weekly' },
  { value: 'monthly', label: 'Monthly' },
]

const KEEP = [1, 2, 3, 4, 5, 6, 8, 10, 12, 26, 52]

/** Titles an AI job's run takes at most: a big first run in parts. */
const LIMITS = [0, 100, 200, 300, 500, 1000]

/** A word on cost next to the schedule. */
const NOTE: Partial<Record<TaskId, string>> = {
  digests: 'uses the AI',
  series_guides: 'uses the AI',
  second_chances: 'uses the AI',
  hidden_gems: 'uses the AI',
  wishlist: 'no AI; reads Steam, and HowLongToBeat once its job is on',
  taste: 'counting only, no AI',
  hltb: 'slow',
  verdicts: "no AI; downloads other players' verdicts, sends nothing about you",
}

const TH = 'py-2.5 pr-3 text-[12.5px] font-normal text-dim'
const TD = 'py-3 pr-3 align-top'

function errText(e: unknown) {
  return e instanceof ApiError ? e.message : String(e)
}

function scheduleText(schedule: string, labels: Partial<Record<TaskId, string>>) {
  if (schedule.startsWith('after:')) {
    const first = schedule.slice(6) as TaskId
    return `After ${labelMid(first, labels[first] ?? first)}`
  }
  return EVERY.find((o) => o.value === schedule)?.label ?? schedule
}

export function JobsSection({ s }: { s: Settings }) {
  const qc = useQueryClient()
  const toast = useToast()
  const activity = useActivity()
  const [view, setView] = useState<View>('scheduled')
  const [logJob, setLogJob] = useState<{ job: Job; name: string } | null>(null)
  const [optionsFor, setOptionsFor] = useState<TaskId | null>(null)

  const jobs = useJobs({
    refetchInterval: (q) => (q.state.data?.current?.status === 'running' ? 1000 : 10_000),
  })
  const scheduled = useQuery({ queryKey: ['scheduled'], queryFn: api.scheduled, refetchInterval: 30_000 })
  const running = jobs.data?.current?.status === 'running' ? jobs.data.current : null

  // A job starting or ending changes the table's last and next runs.
  const runningId = running?.id ?? null
  const seen = useRef<number | null>(runningId)
  useEffect(() => {
    if (seen.current !== runningId) {
      qc.invalidateQueries({ queryKey: ['scheduled'] })
      qc.invalidateQueries({ queryKey: ['jobHistory'] })
    }
    seen.current = runningId
  }, [runningId, qc])

  // With the wishlist off (Settings › Picks) its job never runs, so it isn't listed.
  const rows = (scheduled.data?.tasks ?? []).filter((r) => s.wishlist || r.task !== 'wishlist')
  const labels: Partial<Record<TaskId, string>> = Object.fromEntries(rows.map((r) => [r.task, r.label]))

  const onStarted = (j: Job) => {
    qc.setQueryData<JobsResponse>(keys.jobs, (old) => ({ current: j, recent: old?.recent ?? [] }))
  }
  const run = useMutation({
    mutationFn: ({ task, refresh }: { task: TaskId; refresh?: boolean }) => api.runTask(task, refresh),
    onSuccess: onStarted,
    onError: (e) => toast.error("Couldn't start that", errText(e)),
  })
  const cancel = useMutation({
    mutationFn: api.cancelJob,
    onSuccess: () => qc.invalidateQueries({ queryKey: keys.jobs }),
    onError: (e) => toast.error("Couldn't stop it", errText(e)),
  })
  const change = useMutation({
    mutationFn: ({ task, change }: { task: TaskId; change: ScheduleChange }) => api.setSchedule(task, change),
    onSuccess: (data) => qc.setQueryData<ScheduledResponse>(['scheduled'], data),
    onError: (e) => toast.error("Couldn't change that", errText(e)),
  })
  const setEnabled = async (on: boolean) => {
    try {
      qc.setQueryData(keys.settings, await api.updateSettings({ auto_sync: on }))
      qc.invalidateQueries({ queryKey: ['scheduled'] })
    } catch (e) {
      toast.error("Couldn't save that", errText(e))
    }
  }

  const openLog = (job: Job) => {
    const name = jobName(job, labels)
    setLogJob((cur) => (cur?.job.id === job.id ? null : { job, name }))
  }
  // The open log follows the running job's latest state.
  const shownLog = logJob && running?.id === logJob.job.id ? { ...logJob, job: running } : logJob

  const stopButton = (
    <button type="button" className="btn-outline bg-transparent" disabled={cancel.isPending} onClick={() => cancel.mutate()}>
      {cancel.isPending || running?.cancel_requested ? 'Stopping…' : 'Stop'}
    </button>
  )

  return (
    <Section
      id="jobs"
      title="Jobs"
      desc={
        <>
          What UpNext runs by itself, when each job last ran and how it went.{' '}
          <button type="button" onClick={() => activity.open()} className="text-muted underline decoration-underline underline-offset-4 hover:text-ink">
            Activity
          </button>{' '}
          shows a running job source by source.
        </>
      }
    >
      <div className="flex flex-wrap items-center justify-between gap-x-6 gap-y-2 border-b border-line py-3">
        <Segmented<View>
          label="Jobs view"
          options={[
            { value: 'scheduled', label: 'Scheduled' },
            { value: 'history', label: 'History' },
          ]}
          value={view}
          onChange={setView}
        />
        {view === 'scheduled' && (
          <Checkbox checked={s.auto_sync} onChange={(v) => void setEnabled(v)} note="while UpNext runs; a run missed while it was closed happens when it starts">
            Run jobs on their schedules
          </Checkbox>
        )}
      </div>

      {view === 'scheduled' && (
        <>
          {scheduled.isError && <p className="py-3 text-sm text-bad">Couldn't load the jobs: {errText(scheduled.error)}</p>}
          {scheduled.data?.waiting_for_first_sync && s.auto_sync && (
            <p className="m-0 border-b border-line py-3 text-sm text-dim">Nothing runs by itself until the first sync.</p>
          )}
          {running && !running.task && (
            <div className="flex items-center gap-4 border-b border-line py-3">
              <div className="min-w-0 flex-1">
                <span className="text-[15px] font-bold">{jobName(running, labels)}</span>
                <span className="text-[13.5px] text-dim"> · {progressText(running) || 'starting…'}</span>
                <ProgressBar job={running} label={`${jobName(running, labels)} progress`} />
              </div>
              <TextButton onClick={() => openLog(running)}>Log</TextButton>
              {stopButton}
            </div>
          )}
          {/* Relative, so the headers' screen-reader text (absolutely placed)
              scrolls with the table instead of widening the page. */}
          <div className="relative overflow-x-auto">
            <table className="w-full min-w-[820px] border-collapse text-left text-sm">
              <thead>
                <tr className="border-b border-line">
                  <th scope="col" className={TH}>
                    Job
                  </th>
                  <th scope="col" className={TH}>
                    Schedule
                  </th>
                  <th scope="col" className={TH}>
                    Last run
                  </th>
                  <th scope="col" className={TH}>
                    Result
                  </th>
                  <th scope="col" className={TH}>
                    Took
                  </th>
                  <th scope="col" className={TH}>
                    Next run
                  </th>
                  <th scope="col" className={TH}>
                    <span className="sr-only">Actions</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {rows.map((row) => {
                  const mine = running?.task === row.task ? running : null
                  const last = row.last_run
                  return (
                    <Fragment key={row.task}>
                      <tr className={cx(optionsFor !== row.task && 'border-b border-line')}>
                        <th scope="row" className={cx(TD, 'text-[15px] font-bold whitespace-nowrap')}>
                          {row.label}
                        </th>
                        <td className={cx(TD, 'whitespace-nowrap')}>
                          <ScheduleCell
                            row={row}
                            rows={rows}
                            labels={labels}
                            here={!!s.listen.this_computer}
                            onChange={(c) => change.mutate({ task: row.task, change: c })}
                          />
                        </td>
                        <td className={cx(TD, 'whitespace-nowrap')}>
                          {last ? (
                            <span title={absoluteTime(last.started_at)}>{relativeTime(last.started_at)}</span>
                          ) : (
                            <span className="text-dim">Never</span>
                          )}
                        </td>
                        <td className={cx(TD, 'min-w-[200px]')}>
                          {mine ? (
                            <>
                              <span className="text-muted">{progressText(mine) || 'Starting…'}</span>
                              <ProgressBar job={mine} label={`${row.label} progress`} />
                            </>
                          ) : last ? (
                            <JobResult job={last} onLog={() => openLog(last)} />
                          ) : (
                            <span className="text-dim">–</span>
                          )}
                        </td>
                        <td className={cx(TD, 'whitespace-nowrap text-muted')}>{mine ? jobTook(mine) : last ? jobTook(last) : ''}</td>
                        <td className={cx(TD, 'whitespace-nowrap')}>
                          <NextRun row={row} data={scheduled.data} running={!!mine} />
                        </td>
                        <td className="py-1.5 align-top">
                          <div className="flex items-center justify-end gap-1.5">
                            {(mine ?? last) && (
                              <TextButton aria-label={`${row.label} log`} onClick={() => openLog((mine ?? last)!)}>
                                Log
                              </TextButton>
                            )}
                            {row.options && (
                              <TextButton aria-expanded={optionsFor === row.task} onClick={() => setOptionsFor((o) => (o === row.task ? null : row.task))}>
                                Options
                              </TextButton>
                            )}
                            {mine ? (
                              stopButton
                            ) : (
                              <button
                                type="button"
                                className="btn-outline whitespace-nowrap"
                                aria-label={`Run ${labelMid(row.task, row.label)} now`}
                                disabled={!!running || run.isPending}
                                title={running ? 'Another job is running' : undefined}
                                onClick={() => run.mutate({ task: row.task })}
                              >
                                Run now
                              </button>
                            )}
                          </div>
                        </td>
                      </tr>
                      {optionsFor === row.task && row.options && (
                        <tr className="border-b border-line">
                          <td colSpan={7} className="pb-3">
                            <TaskOptions
                              row={row}
                              s={s}
                              onChange={(options) => change.mutate({ task: row.task, change: { options } })}
                              onRefresh={running ? undefined : () => run.mutate({ task: row.task, refresh: true })}
                            />
                          </td>
                        </tr>
                      )}
                    </Fragment>
                  )
                })}
              </tbody>
            </table>
          </div>
        </>
      )}

      {view === 'history' && <History labels={labels} running={running} openLog={openLog} openId={logJob?.job.id ?? null} />}

      {shownLog && <JobLog job={shownLog.job} name={shownLog.name} onRetried={onStarted} onClose={() => setLogJob(null)} />}
    </Section>
  )
}

/** A task's schedule as a sentence of menus: when it runs, backups kept,
 *  and how many titles an AI job's run takes. The setup wizard uses it too. */
export function ScheduleCell({
  row,
  rows,
  labels,
  here = true,
  onChange,
}: {
  row: ScheduledTask
  rows: ScheduledTask[]
  labels: Partial<Record<TaskId, string>>
  /** On the computer UpNext runs on, the only place backups kept is set. */
  here?: boolean
  onChange: (c: ScheduleChange) => void
}) {
  const after: SentenceOption[] = rows
    .filter((r) => r.task !== row.task && (canFollow(row.task, r.task, rows) || row.schedule === `after:${r.task}`))
    .map((r) => ({ value: `after:${r.task}`, label: `After ${labelMid(r.task, r.label)}` }))
  const note = NOTE[row.task]
  return (
    <>
      <SentenceSelect
        label={`When ${labelMid(row.task, row.label)} runs`}
        display={scheduleText(row.schedule, labels)}
        value={row.schedule}
        options={[...EVERY, ...after]}
        onChange={(v) => onChange({ schedule: v })}
      />
      {row.keep != null && (
        <>
          <span className="text-muted">, keep </span>
          {here ? (
            <SentenceSelect
              label="Backups kept"
              display={String(row.keep)}
              value={String(row.keep)}
              options={(KEEP.includes(row.keep) ? KEEP : [...KEEP, row.keep].sort((a, b) => a - b)).map((n) => ({ value: String(n), label: String(n) }))}
              onChange={(v) => onChange({ keep: Number(v) })}
            />
          ) : (
            // Fewer kept removes the rest, so it's set where backups are deleted.
            <span title="Set on the computer UpNext runs on">{row.keep}</span>
          )}
        </>
      )}
      {row.uses_ai && row.schedule !== 'off' && (
        <>
          <span className="text-muted">, </span>
          <SentenceSelect
            label={`How much ${labelMid(row.task, row.label)} does a run`}
            display={row.limit ? `${row.limit} a run` : 'all at once'}
            value={String(row.limit ?? 0)}
            options={(row.limit == null || LIMITS.includes(row.limit) ? LIMITS : [...LIMITS, row.limit].sort((a, b) => a - b)).map((n) => ({
              value: String(n),
              label: n ? `${n} a run` : 'all at once',
            }))}
            onChange={(v) => onChange({ limit: Number(v) })}
          />
        </>
      )}
      {note && <span className="ml-2 text-[12.5px] text-dim">{note}</span>}
    </>
  )
}

function NextRun({ row, data, running }: { row: ScheduledTask; data: ScheduledResponse | undefined; running: boolean }) {
  if (running) return <span className="text-accent">Running now</span>
  if (row.schedule === 'off') return <span className="text-dim">–</span>
  if (!data?.enabled) return <span className="text-dim" title="Schedules are off">Paused</span>
  if (data.waiting_for_first_sync) return <span className="text-dim">After the first sync</span>
  if (row.next_after) return <span>After the next {nextRunNoun(row.next_after)}</span>
  if (row.next_run_at == null) return null
  if (isPast(row.next_run_at)) return <span title="As soon as no other job is running">Soon</span>
  return <span title={absoluteTime(row.next_run_at)}>{upperFirst(inText(row.next_run_at))}</span>
}

/** Libraries for a sync, sources for Fetch details: saved, so the schedule and Run now both use them. */
function TaskOptions({
  row,
  s,
  onChange,
  onRefresh,
}: {
  row: ScheduledTask
  s: Settings
  onChange: (options: Record<string, boolean>) => void
  onRefresh?: () => void
}) {
  const isSync = row.task === 'sync'
  const gog = useGog({ enabled: isSync })
  const gogConnected = (gog.data ?? s.gog).connected
  const libraries = useQuery({
    queryKey: ['syncSources', gogConnected, s.galaxy_db, s.epic?.connected],
    queryFn: api.syncSources,
    staleTime: 60_000,
    enabled: isSync,
  })
  const sources = useQuery({
    queryKey: ['enrichSources', s.rawg_api_key_set, s.igdb_client_id, s.igdb_secret_set, s.linux_active],
    queryFn: api.enrichSources,
    staleTime: 60_000,
    enabled: !isSync,
  })
  const options = row.options ?? {}
  const list = isSync ? libraries : sources
  const items = (list.data ?? []).filter((o) => o.id in options)
  return (
    <fieldset className="m-0 border-0 p-0">
      <legend className="mb-1 text-[13.5px] text-dim">
        {isSync ? 'Libraries it reads' : "What it fetches, for games that don't have it yet"}. Saved for the schedule and Run now.
      </legend>
      {list.isError && <p className="text-sm text-bad">Couldn't load the list: {errText(list.error)}</p>}
      <div className="grid grid-cols-1 gap-x-8 sm:grid-cols-2 xl:grid-cols-4">
        {items.map((o) => (
          <Checkbox key={o.id} checked={!o.blocked && options[o.id]} disabled={!!o.blocked} onChange={(v) => onChange({ [o.id]: v })} note={o.blocked ?? undefined}>
            {o.label}
          </Checkbox>
        ))}
      </div>
      {!isSync && (
        <p className="m-0 mt-2 text-[13.5px] text-dim">
          <TextButton disabled={!onRefresh} onClick={onRefresh} title={onRefresh ? undefined : 'Another job is running'}>
            Run now, fetching everything again
          </TextButton>{' '}
          · every store page and review again, which is slow for a big library
        </p>
      )}
    </fieldset>
  )
}

const BY: Record<Job['source'], string> = { schedule: 'Schedule', ui: 'By hand', terminal: 'Terminal' }

function History({
  labels,
  running,
  openLog,
  openId,
}: {
  labels: Partial<Record<TaskId, string>>
  running: Job | null
  openLog: (j: Job) => void
  openId: number | null
}) {
  const [which, setWhich] = useState<string>('')
  const [failedOnly, setFailedOnly] = useState(false)
  const task = (which || undefined) as TaskId | 'none' | undefined
  const history = useQuery({
    queryKey: ['jobHistory', which, failedOnly],
    queryFn: () => api.jobs({ limit: 100, task, failed: failedOnly }),
    refetchInterval: 30_000,
  })
  const matches = (j: Job) => !failedOnly && (!which || (which === 'none' ? !j.task : j.task === which))
  const recent = history.data?.recent ?? []
  // Just after it ends, the running job can be in both lists for a moment.
  const list = [...(running && matches(running) && !recent.some((j) => j.id === running.id) ? [running] : []), ...recent]
  const jobOptions: SentenceOption[] = [
    { value: '', label: 'any job' },
    ...Object.entries(labels).map(([id, label]) => ({ value: id, label: labelMid(id as TaskId, label!) })),
    { value: 'none', label: 'other work (retries, restores, imports)' },
  ]
  return (
    <>
      <p className="m-0 border-b border-line py-3 text-[15px] text-muted">
        Showing{' '}
        <SentenceSelect
          label="Which runs"
          display={failedOnly ? 'runs that failed' : 'every run'}
          value={failedOnly ? 'failed' : 'all'}
          options={[
            { value: 'all', label: 'every run' },
            { value: 'failed', label: 'runs that failed or had failures' },
          ]}
          onChange={(v) => setFailedOnly(v === 'failed')}
        />{' '}
        of{' '}
        <SentenceSelect
          label="Which job"
          display={which === 'none' ? 'other work' : which ? labelMid(which as TaskId, labels[which as TaskId] ?? which) : 'any job'}
          value={which}
          options={jobOptions}
          onChange={setWhich}
        />
        , newest first
      </p>
      {history.isError && <p className="py-3 text-sm text-bad">Couldn't load the history: {errText(history.error)}</p>}
      {history.data && list.length === 0 && (
        <p className="m-0 border-b border-line py-3 text-sm text-dim">{failedOnly || which ? 'No runs like that.' : 'No runs yet.'}</p>
      )}
      {list.length > 0 && (
        <div className="relative overflow-x-auto">
          <table className="w-full min-w-[820px] border-collapse text-left text-sm">
            <thead>
              <tr className="border-b border-line">
                <th scope="col" className={TH}>
                  Started
                </th>
                <th scope="col" className={TH}>
                  Job
                </th>
                <th scope="col" className={TH}>
                  By
                </th>
                <th scope="col" className={TH}>
                  Result
                </th>
                <th scope="col" className={TH}>
                  Took
                </th>
                <th scope="col" className={TH}>
                  Tokens
                </th>
                <th scope="col" className={TH}>
                  <span className="sr-only">Log</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {list.map((j) => (
                <tr key={j.id} className={cx('border-b border-line', openId === j.id && 'bg-raised')}>
                  <td className={cx(TD, 'whitespace-nowrap')} title={absoluteTime(j.started_at)}>
                    {relativeTime(j.started_at)}
                  </td>
                  <th scope="row" className={cx(TD, 'font-semibold whitespace-nowrap')}>
                    {jobName(j, labels)}
                  </th>
                  <td className={cx(TD, 'whitespace-nowrap text-muted')}>{BY[j.source]}</td>
                  <td className={cx(TD, 'min-w-[220px]')}>
                    {j.status === 'running' ? (
                      <span className="text-accent">Running · {progressText(j) || 'starting…'}</span>
                    ) : (
                      <JobResult job={j} onLog={() => openLog(j)} />
                    )}
                  </td>
                  <td className={cx(TD, 'whitespace-nowrap text-muted')}>{jobTook(j)}</td>
                  <td className={cx(TD, 'font-mono text-[11.5px] whitespace-nowrap text-muted')}>
                    {j.ai_usage ? `${tokensText(j.ai_usage.input_tokens)} in, ${tokensText(j.ai_usage.output_tokens)} out` : ''}
                  </td>
                  <td className="py-1.5 text-right align-top">
                    <TextButton aria-label={`Log of ${jobName(j, labels)}, ${relativeTime(j.started_at)}`} onClick={() => openLog(j)}>
                      {openId === j.id ? 'Hide log' : 'Log'}
                    </TextButton>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </>
  )
}
