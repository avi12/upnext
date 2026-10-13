import type { PickBatch, PlanSummary, PlanWindow, Usage } from '../types'
import { upperFirst } from './format'

/** What UpNext can't know about a subscription. */
export const PLAN_WARNING =
  "UpNext doesn't know your plan's limits, so it can't promise how much of your plan picks or review summaries will use."

const CODEX = 'codex-cli'

/** ChatGPT's plan types as its plans are named; others are spelled out from the id. */
const CHATGPT_PLANS: Record<string, string> = {
  prolite: 'Pro Lite',
  promax: 'Pro Max',
  edu_plus: 'Edu Plus',
  edu_pro: 'Edu Pro',
}

/** "Your Claude plan (Max)", "Your ChatGPT plan (Plus)". */
export function planTitle(p: Pick<PlanSummary, 'backend' | 'plan'>): string {
  const whose = p.backend === CODEX ? 'ChatGPT' : 'Claude'
  if (!p.plan) return `Your ${whose} plan`
  const name = CHATGPT_PLANS[p.plan] ?? upperFirst(p.plan.replace(/_/g, ' '))
  return `Your ${whose} plan (${name})`
}

/** Whose other use moves the same bars: "Claude", "Codex". */
export function otherUse(backend: string): string {
  return backend === CODEX ? 'Codex' : 'Claude'
}

/** "This session", "This week", "This week, Fable"; the Codex CLI's windows keep UpNext's label for them ("5-hour limit"). */
export function windowName(id: string, label: string, backend?: string): string {
  if (backend === CODEX) return label
  if (id === 'session') return 'This session'
  if (id === 'week') return 'This week'
  if (id.startsWith('week:')) return `This week, ${id.slice(5)}`
  return label
}

/** "the session", "the week", "Fable's week": a window mid-sentence. The Codex CLI's session is "the 5-hour window". */
export function windowNoun(id: string, backend?: string): string {
  if (id === 'session') return backend === CODEX ? 'the 5-hour window' : 'the session'
  if (id === 'week') return 'the week'
  if (id.startsWith('week:')) return `${id.slice(5)}'s week`
  return id
}

/** When a window resets: the Claude CLI's wording without its time zone when that's this browser's ("Oct 4, 3pm"), or the Codex CLI's time in this browser's ("Oct 1, 3:34 AM"). */
export function resetsText(w: Pick<PlanWindow, 'resets' | 'resets_at'>): string | null {
  if (w.resets_at != null) {
    return new Date(w.resets_at * 1000).toLocaleString(undefined, { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' })
  }
  if (!w.resets) return null
  const zone = Intl.DateTimeFormat().resolvedOptions().timeZone
  return w.resets.replace(` (${zone})`, '')
}

/** An estimated share: "about 0.3%", "about 0.05%", "about 41%", "under 0.01%". */
export function pctText(p: number): string {
  if (p >= 10) return `about ${Math.round(p)}%`
  if (p >= 0.1) return `about ${p.toFixed(1).replace(/\.0$/, '')}%`
  if (p >= 0.01) return `about ${p.toFixed(2)}%`
  return 'under 0.01%'
}

/** What some calls took of each measured window, in percent: their dollars or tokens over a point's worth. */
export function shares(plan: PlanSummary, used: { api_usd: number | null; usage: Usage | null }): { id: string; percent: number }[] {
  const m = plan.measured
  if (!m) return []
  const amount = m.measure === 'api_usd' ? used.api_usd : used.usage ? used.usage.input_tokens + used.usage.output_tokens : null
  if (amount == null) return []
  return m.windows.filter((w) => w.per_point > 0).map((w) => ({ id: w.id, percent: amount / w.per_point }))
}

/** "about 0.3% of the session and 0.04% of the week", for a pick's footer. */
export function pickShareText(plan: PlanSummary, batch: Pick<PickBatch, 'api_usd' | 'usage'>): string | null {
  const s = shares(plan, batch).filter((x) => !x.id.includes(':'))
  if (!s.length) return null
  return s.map((x) => `${pctText(x.percent)} of ${windowNoun(x.id, plan.backend)}`).join(' and ')
}
