import type { Job, LaneProgress, ScheduledTask, TaskId, Usage } from '../types'
import { plural } from './format'

/** What "after the next …" calls each task. */
const NEXT_RUN_NOUN: Record<TaskId, string> = {
  sync: 'sync',
  details: 'details run',
  reviews: 'reviews refresh',
  digests: 'People say run',
  series_guides: 'series guides run',
  second_chances: 'second chances run',
  hidden_gems: 'hidden gems run',
  wishlist: 'wishlist update',
  taste: 'profile update',
  hltb: 'HowLongToBeat run',
  backup: 'backup',
  verdicts: 'backlog verdicts download',
}

export function nextRunNoun(task: TaskId): string {
  return NEXT_RUN_NOUN[task]
}

/** A task's label in the middle of a sentence: "after sync libraries", but People say and HowLongToBeat keep their capitals. */
export function labelMid(task: TaskId, label: string): string {
  return task === 'digests' || task === 'hltb' ? label : label.toLowerCase()
}

/** "Looking up RAWG" -> "looking up RAWG" for use mid-sentence. */
export function lowerFirst(s: string): string {
  return s && /^[A-Z][a-z]/.test(s) ? s[0].toLowerCase() + s.slice(1) : s
}

/** "42 s", "3 min", "1 h 38 min", "4 days" */
export function tookText(secs: number): string {
  const s = Math.max(0, Math.round(secs))
  if (s < 60) return `${s} s`
  if (s < 3600) return `${Math.floor(s / 60)} min`
  if (s < 2 * 86400) {
    const h = Math.floor(s / 3600)
    const m = Math.floor((s % 3600) / 60)
    return m ? `${h} h ${m} min` : `${h} h`
  }
  return `${Math.round(s / 86400)} days`
}

/** How long a job took, or has been running. */
export function jobTook(job: Job): string {
  return tookText((job.finished_at ?? Date.now() / 1000) - job.started_at)
}

/** Whether a time has come. */
export function isPast(unix: number): boolean {
  return unix <= Date.now() / 1000
}

/** "in 21 h", "in 6 days" */
export function inText(unix: number): string {
  return `in ${tookText(unix - Date.now() / 1000)}`
}

/** A token count, short: "310", "27.4k", "1.9M". */
export function tokensText(n: number): string {
  if (n < 1000) return String(n)
  if (n < 999_950) return `${(n / 1e3).toFixed(1)}k`
  return `${(n / 1e6).toFixed(1)}M`
}

/** A call's or a sum of calls' tokens: "27.4k tokens in, 310 out". */
export function usageText(u: Pick<Usage, 'input_tokens' | 'output_tokens'>): string {
  return `${tokensText(u.input_tokens)} tokens in, ${tokensText(u.output_tokens)} out`
}

/** An AI job's tokens so far and, while it runs, its estimate: "1.9M tokens so far · about 5.4M in all". */
export function aiProgressText(job: Job): string | null {
  const u = job.ai_usage
  if (!u) return null
  const sofar = `${tokensText(u.input_tokens + u.output_tokens)} tokens so far`
  return u.estimated_tokens != null ? `${sofar} · about ${tokensText(u.estimated_tokens)} in all` : sofar
}

/** What the top bar says a running job is doing: "Syncing", "Writing summaries". */
export function jobVerb(job: Job): string {
  switch (job.task ?? job.kind) {
    case 'sync':
      return 'Syncing'
    case 'backup':
      return 'Backing up'
    case 'restore':
      return 'Restoring'
    case 'import':
      return 'Importing'
    case 'reviews':
      return 'Refreshing reviews'
    case 'digests':
      return 'Writing summaries'
    case 'series_guides':
      return 'Writing guides'
    case 'second_chances':
      return 'Checking second chances'
    case 'hidden_gems':
      return 'Finding hidden gems'
    case 'wishlist':
      return 'Updating the wishlist'
    case 'taste':
      return 'Updating your profile'
    case 'hltb':
      return 'Looking up HowLongToBeat'
    case 'verdicts':
      return 'Downloading backlog verdicts'
    default:
      return 'Fetching details'
  }
}

/** The lane that's waiting on a service, if any: its wait colours the bars. */
export function waitingLane(job: Job) {
  return job.lanes.find((l) => l.state === 'waiting') ?? null
}

/** Whether a finished job ended failed or with failures (a toast, a warning in the top bar). */
export function endedBadly(job: Job): boolean {
  return job.status === 'failed' || job.failed > 0
}

/** A sync that finished with an account failed while the rest synced: partial, not done. */
export function isPartial(job: Job): boolean {
  return job.kind === 'sync' && job.status === 'done' && job.failed > 0
}

/** How a finished job ended, for its row: "Done: 2,000 games, 3 new", a partial sync's own summary ("Partial: …"), or "Cancelled". */
export function resultText(job: Job): string {
  if (job.status === 'cancelled') return 'Cancelled'
  if (isPartial(job)) return job.summary ?? 'Partial'
  return `Done${job.summary ? `: ${job.summary}` : ''}`
}

/** A finished job's trouble in a few words, for a toast or the top bar: "Sync failed", "Sync: partial, 1 failed", "Game details: 3 failed". */
export function troubleText(job: Job, name: string): string {
  if (job.status === 'failed') return `${name} failed`
  const failed = `${job.failed.toLocaleString()} failed`
  if (isPartial(job)) return `${name}: partial, ${failed}`
  return `${name}: ${job.status === 'cancelled' ? 'stopped, ' : ''}${failed}`
}

/** The newest finished run when it ended badly and Activity hasn't shown it yet: the top bar flags it. */
export function unseenTrouble(recent: Job[] | undefined, seenId: number): Job | null {
  const newest = recent?.[0]
  return newest && newest.id > seenId && endedBadly(newest) ? newest : null
}

/** A job's name in a list: its task's, or what it did otherwise ("Game details, 3 titles"). */
export function jobName(job: Job, labels: Partial<Record<TaskId, string>>): string {
  if (job.task) return labels[job.task] ?? job.task
  if (job.kind === 'sync') return 'Sync'
  if (job.kind === 'backup') return 'Backup'
  if (job.kind === 'restore') return 'Restore'
  if (job.kind === 'import') return 'Import'
  if (job.kind === 'taste') return 'Update your profile'
  const titles = Array.isArray(job.options?.titles) ? (job.options.titles as unknown[]).length : 0
  if (titles) return `Game details, ${titles.toLocaleString()} ${titles === 1 ? 'title' : 'titles'}`
  if (typeof job.options?.limit === 'number') return `Game details, the first ${job.options.limit.toLocaleString()}`
  return 'Game details, some sources'
}

/** How far a running job has got: "from a terminal · 1,040 of 1,600 · 1.9M tokens so far · about 5.4M in all · 8 min left". */
export function progressText(job: Job, { source = true, message = true }: { source?: boolean; message?: boolean } = {}): string {
  const parts: string[] = []
  if (source && job.source === 'terminal') parts.push('from a terminal')
  const lanes = lanesText(job)
  if (lanes) parts.push(lanes)
  else if (job.total > 0) parts.push(`${job.done.toLocaleString()} of ${job.total.toLocaleString()}`)
  else if (job.done > 0) parts.push(job.done.toLocaleString())
  const ai = aiProgressText(job)
  if (ai) parts.push(ai)
  if (job.eta_secs != null && job.eta_secs > 0) parts.push(`${tookText(job.eta_secs)} left`)
  else if (message && job.message) parts.push(lowerFirst(job.message))
  return parts.join(' · ')
}

/** While no game is counted done yet but the lanes are working, what they
 *  have done: "2 of 9 sources done", or the busiest one's count,
 *  "Wikidata 120 of 261". Otherwise a run reads "starting" for as long as its
 *  first sources take. */
export function lanesText(job: Job): string | null {
  if (job.done > 0 || job.lanes.length === 0) return null
  const finished = job.lanes.filter((l) => l.state === 'done').length
  if (finished > 0) return `${finished} of ${plural(job.lanes.length, 'source')} done`
  const busiest = job.lanes.reduce<LaneProgress | null>((m, l) => (l.done > (m?.done ?? 0) ? l : m), null)
  if (!busiest) return null
  const count = busiest.total > 0 ? `${busiest.done.toLocaleString()} of ${busiest.total.toLocaleString()}` : busiest.done.toLocaleString()
  return `${busiest.name} ${count}`
}

/** "Sync libraries already follows …": the tasks `task` could run after without them starting each other in a circle. */
export function canFollow(task: TaskId, first: TaskId, rows: ScheduledTask[]): boolean {
  let at: TaskId | undefined = first
  for (let i = 0; at && i < rows.length; i++) {
    if (at === task) return false
    const schedule: string | undefined = rows.find((r) => r.task === at)?.schedule
    at = schedule?.startsWith('after:') ? (schedule.slice(6) as TaskId) : undefined
  }
  return true
}
