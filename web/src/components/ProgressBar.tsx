import type { Job } from '../types'
import { cx } from '../lib/format'
import { waitingLane } from '../lib/jobs'

export type BarTone = 'accent' | 'warn' | 'bad' | 'good'

const FILL: Record<BarTone, string> = { accent: 'bg-accent', warn: 'bg-warn', bad: 'bg-bad', good: 'bg-good' }

/** A thin line of progress; it pulses while the total isn't known. */
export function Bar({
  done,
  total,
  label,
  tone = 'accent',
  className,
}: {
  done: number
  total: number
  label: string
  tone?: BarTone
  className?: string
}) {
  const p = total > 0 ? Math.min(100, (done / total) * 100) : null
  return (
    <div
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={total || undefined}
      aria-valuenow={total ? done : undefined}
      className={cx('h-1 overflow-hidden rounded-sm bg-line', className)}
    >
      <div
        className={cx('h-1 rounded-sm transition-[width] duration-300', FILL[tone], p == null && 'w-1/3 animate-pulse')}
        style={p != null ? { width: `${p}%` } : undefined}
      />
    </div>
  )
}

/** A running job's overall bar: in the warning colour while a service makes it wait. */
export function ProgressBar({ job, label, className = 'mt-2' }: { job: Job; label: string; className?: string }) {
  return <Bar done={job.done} total={job.total} label={label} tone={waitingLane(job) ? 'warn' : 'accent'} className={className} />
}
