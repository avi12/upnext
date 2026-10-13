import { useEffect, useRef } from 'react'
import { useMutation, useQuery } from '@tanstack/react-query'
import { api, ApiError } from '../../api'
import type { Job } from '../../types'
import { useToast } from '../useToast'
import { cx, relativeTime } from '../../lib/format'
import { TextButton } from './parts'
import { isPartial, resultText } from '../../lib/jobs'

function errText(e: unknown) {
  return e instanceof ApiError ? e.message : String(e)
}

/** "Done: 2,000 games, 3 new", with its failures as a link into the log. Cut to one line in tables; `wrap` shows it all. */
export function JobResult({ job, onLog, wrap = false }: { job: Job; onLog: () => void; wrap?: boolean }) {
  const failed =
    job.failed > 0 ? (
      <span className="shrink-0 whitespace-nowrap">
        ,{' '}
        <button type="button" onClick={onLog} className="text-warn underline decoration-underline underline-offset-4 hover:decoration-accent">
          {job.failed.toLocaleString()} failed
        </button>
      </span>
    ) : null
  if (job.status === 'failed') {
    return (
      <span className={cx('block text-bad', !wrap && 'max-w-[280px] truncate')} title={job.error ?? undefined}>
        Failed{job.error ? `: ${job.error}` : ''}
      </span>
    )
  }
  const text = resultText(job)
  return (
    <span className={wrap ? 'block' : 'flex max-w-[280px] min-w-0'}>
      <span className={cx(!wrap && 'truncate')} title={job.summary ?? undefined}>
        {text}
      </span>
      {failed}
    </span>
  )
}

/** A job's log, its failures with "Try these again", and a download. Follows a running job. */
export function JobLog({ job, name, onRetried, onClose }: { job: Job; name: string; onRetried: (j: Job) => void; onClose?: () => void }) {
  const toast = useToast()
  const detail = useQuery({
    queryKey: ['job', job.id],
    queryFn: () => api.job(job.id),
    refetchInterval: (q) => ((q.state.data?.status ?? job.status) === 'running' ? 1500 : false),
  })
  const retry = useMutation({
    mutationFn: () => api.retryFailed(job.id),
    onSuccess: onRetried,
    onError: (e) => toast.error("Couldn't start that", errText(e)),
  })
  const lines = detail.data?.log ?? []
  const failures = detail.data?.failures ?? []
  const ref = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const el = ref.current
    if (el) el.scrollTop = el.scrollHeight
  }, [lines.length])
  const status = detail.data?.status ?? job.status
  return (
    <div className="border-b border-line py-3">
      <div className="mb-2 flex flex-wrap items-baseline gap-x-3 text-[13.5px] text-dim">
        <span>
          <span className="font-semibold text-ink">{name}</span> log · started {relativeTime(job.started_at)}
          {job.source === 'terminal' && ' from a terminal'}
          {job.source === 'schedule' && ' by its schedule'}
          {status !== 'running' && ` · ${status === 'done' ? (isPartial(detail.data ?? job) ? 'finished, partly' : 'finished') : status}`}
          {failures.length > 0 && ` · ${failures.length.toLocaleString()} failed`}
        </span>
        <span className="ml-auto flex items-baseline gap-x-3">
          {detail.data?.can_retry && (
            <TextButton disabled={retry.isPending} onClick={() => retry.mutate()}>
              Try these again
            </TextButton>
          )}
          <a href={`/api/v1/jobs/${job.id}/log.txt`} download className="text-[13.5px] text-muted hover:text-ink">
            Download
          </a>
          {onClose && <TextButton onClick={onClose}>Close</TextButton>}
        </span>
      </div>
      {(detail.data?.error ?? job.error) && <p className="m-0 mb-2 text-sm text-bad">{detail.data?.error ?? job.error}</p>}
      {detail.isError && <p className="m-0 mb-2 text-sm text-bad">Couldn't load the log: {errText(detail.error)}</p>}
      <div
        ref={ref}
        className="max-h-72 overflow-y-auto rounded-[10px] bg-raised p-3 font-mono text-[11px] leading-relaxed text-muted"
        tabIndex={0}
        aria-label={`${name} log`}
      >
        {lines.length === 0 && !detail.isLoading && <div className="text-dim">Nothing logged.</div>}
        {lines.map((line) => (
          <div key={line.id} className={cx('break-words whitespace-pre-wrap', line.level === 'error' ? 'text-bad' : line.level === 'warn' && 'text-warn')}>
            {line.lane && <span className="text-dim">{line.lane}: </span>}
            {line.message}
          </div>
        ))}
        {detail.data?.log_more && <div className="text-dim">The log goes on; download it to read the rest.</div>}
      </div>
    </div>
  )
}
