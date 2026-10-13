import type { GameView } from '../types'

/** Original release year: Wikidata's first year, else the year in the store date. */
export function yearOf(g: Pick<GameView, 'first_year' | 'release_date'>): number | null {
  if (g.first_year) return g.first_year
  const m = g.release_date?.match(/\b(19|20)\d{2}\b/)
  return m ? Number(m[0]) : null
}

/** The year as text, or '' when unknown. */
export function gameYear(g: Pick<GameView, 'first_year' | 'release_date'>): string {
  return String(yearOf(g) ?? '')
}

/** "45m", "3h", "12h 5m". */
export function hoursMinutes(min: number): string {
  if (min < 60) return `${Math.max(1, Math.round(min))}m`
  const h = Math.floor(min / 60)
  const m = Math.round(min % 60)
  return m ? `${h}h ${m}m` : `${h}h`
}

const WEEKDAYS = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday']

/** "Saved today", "Saved Tuesday", "Saved last week", "Saved last month"… */
export function savedText(unix: number | null | undefined, now: Date = new Date()): string {
  if (!unix) return 'Saved'
  const then = new Date(unix * 1000)
  const startOf = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime()
  const days = Math.round((startOf(now) - startOf(then)) / 86_400_000)
  if (days <= 0) return 'Saved today'
  if (days === 1) return 'Saved yesterday'
  if (days < 7) return `Saved ${WEEKDAYS[then.getDay()]}`
  if (days < 14) return 'Saved last week'
  if (days < 28) return `Saved ${Math.floor(days / 7)} weeks ago`
  if (days < 62) return 'Saved last month'
  return `Saved in ${then.toLocaleDateString(undefined, { month: 'long', ...(then.getFullYear() !== now.getFullYear() ? { year: 'numeric' } : {}) })}`
}
