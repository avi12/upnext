import type { MyVerdict, VerdictAnswers, VerdictContract, VerdictField, VerdictSource } from '../types'

/** The outcomes that ask what stopped you. */
const STOPPED = ['shelved', 'stopped', 'bounced']

/** The marks that end something, after which the game page offers a verdict. */
export const ENDING_MARKS = ['done', 'shelved', 'abandoned', 'burnt_out', 'hidden']

/** "from your mark", beside a pre-filled answer. */
export const SOURCE_WORDS: Record<VerdictSource, string> = {
  mark: 'from your mark',
  playing: "you're playing it",
  playtime: 'from your playtime',
  diary: 'from your play diary',
  stop_reasons: 'from why you stopped',
  no_ending: 'it has no ending',
}

/** Whether a field shows for these answers: factors only with an answer
 *  on limited time, the stopping note with a stopping point, and what
 *  stopped you only for shelved, stopped and bounced. */
export function fieldShows(f: VerdictField, a: VerdictAnswers): boolean {
  if (f.with && !a[f.with as keyof VerdictAnswers]) return false
  if (f.only_for && !f.only_for.includes(a.outcome ?? '')) return false
  return true
}

/** The answers as sent: what a hidden field holds is dropped, so a
 *  verdict never carries an answer its form didn't show. */
export function shownAnswers(fields: VerdictField[], a: VerdictAnswers): VerdictAnswers {
  const out: VerdictAnswers = { ...a }
  for (const f of fields) {
    if (fieldShows(f, a)) continue
    const k = f.name as keyof VerdictAnswers
    if (f.many) (out as Record<string, unknown>)[k] = []
    else (out as Record<string, unknown>)[k] = null
  }
  if (!STOPPED.includes(a.outcome ?? '')) out.stopped_for = []
  return out
}

/** Why typed text would be refused, in the form's words, or null: the
 *  contract's own checks (the service runs the same), after the invisible
 *  characters it ignores are taken out. */
export function textRefusal(contract: VerdictContract, text: string, max: number): string | null {
  if (!text.trim()) return null
  const n = [...text].length
  if (n > max) return `It's ${n} characters; at most ${max}.`
  let cleaned = text
  try {
    cleaned = text.replace(new RegExp(contract.text_ignored.pattern, 'gu'), '')
  } catch {
    /* an older browser: check the text as typed */
  }
  for (const c of contract.text_checks) {
    let re: RegExp
    try {
      re = new RegExp(c.pattern, 'iu')
    } catch {
      continue
    }
    if (re.test(cleaned)) return `It ${c.why}; take it out to send.`
  }
  return null
}

/** One verdict's standing, in a few words. */
export function stateText(v: MyVerdict): string {
  const day = v.sent_at ? new Date(v.sent_at * 1000).toLocaleDateString(undefined, { day: 'numeric', month: 'long' }) : null
  switch (v.state) {
    case 'draft':
      return v.remote_id ? 'Changes not sent; your earlier verdict is still up' : 'Draft, not sent'
    case 'queued':
      return v.sent_at ? 'Your changes are waiting to be sent' : 'Waiting to be sent'
    case 'sent':
      return day ? `Sent ${day}` : 'Sent'
    case 'held':
      return 'Sent, held by the service for now'
    case 'withdrawn':
      return 'Taken down'
    case 'removed':
      return "Removed by the service's moderators"
  }
}

/** "9 October 2026", from the snapshot's day. */
export function snapshotDay(date: string): string {
  const d = new Date(`${date}T00:00:00Z`)
  if (Number.isNaN(d.getTime())) return date
  return d.toLocaleDateString('en-GB', { day: 'numeric', month: 'long', year: 'numeric', timeZone: 'UTC' })
}
