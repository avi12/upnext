import type { SeedApplied } from '../types'
import { monthYear, plural } from './format'

/** The toast after shared data is applied from Settings › Library data. */
export function appliedToast(r: SeedApplied): [string, string] {
  const kept = r.kept_own > 0 ? ` Your own ${r.kept_own === 1 ? 'summary stays' : `${r.kept_own.toLocaleString()} summaries stay`}.` : ''
  return [
    `Shared data of ${monthYear(r.date)} applied`,
    `${r.matched.toLocaleString()} of its ${plural(r.entries, 'game')} ${r.matched === 1 ? 'is' : 'are'} in your library; ${r.written.toLocaleString()} got ${r.written === 1 ? 'a summary' : 'summaries'}.${kept}`,
  ]
}
