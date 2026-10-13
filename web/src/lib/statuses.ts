import type { GameKind, OverrideStatus } from '../types'

/** The settings the menu's words depend on (GET /api/settings), with their defaults. */
export interface StatusLines {
  abandon_after_days: number
  burnout_days: number
}

export const DEFAULT_LINES: StatusLines = { abandon_after_days: 730, burnout_days: 90 }

/** The statuses a person sets on a game, in menu order. Games with an ending
 *  get one menu and endless games another; the words are what the
 *  UI says everywhere. */
export const OVERRIDES: {
  value: OverrideStatus
  label: string
  /** One line under the label in the Mark as menu. */
  hint: (lines: StatusLines) => string
  /** Dot colour class. */
  dot: string
  /** The menus that offer it. */
  kinds: GameKind[]
  /** Marking it asks why the game was put down. */
  asksWhy: boolean
}[] = [
  { value: 'done', label: 'Beat it', hint: () => 'Finished. Counts as played in series.', dot: 'bg-good', kinds: ['ending'], asksWhy: false },
  {
    value: 'shelved',
    label: 'Shelved',
    hint: (l) => `You mean to finish it. Stays in picks; after ${span(l.abandon_after_days)} untouched it becomes abandoned.`,
    dot: 'bg-warn',
    kinds: ['ending'],
    asksWhy: true,
  },
  {
    value: 'abandoned',
    label: 'Abandoned',
    hint: () => 'Not planning to go back. Out of picks, except for one second chance when reviews say what stopped you has changed.',
    dot: 'bg-bad',
    kinds: ['ending'],
    asksWhy: true,
  },
  { value: 'still_playing', label: 'Still playing', hint: () => 'You play it now and then.', dot: 'bg-accent', kinds: ['endless'], asksWhy: false },
  {
    value: 'burnt_out',
    label: 'Burnt out',
    hint: (l) => `You need a break and will come back. Out of picks for ${span(l.burnout_days)}, then back by itself.`,
    dot: 'bg-warn',
    kinds: ['endless'],
    asksWhy: true,
  },
  { value: 'hidden', label: 'Not for me', hint: () => 'Never recommend it again.', dot: 'bg-faint', kinds: ['ending', 'endless'], asksWhy: false },
]

export function overrideMeta(value: OverrideStatus | null | undefined) {
  return OVERRIDES.find((o) => o.value === value) ?? null
}

/** A length of days in words: 730 is "2 years", 90 "3 months", 14 "2 weeks". */
export function span(days: number): string {
  const unit = (n: number, one: string, many: string) => (n === 1 ? `a ${one}` : `${n} ${many}`)
  if (days % 365 === 0) return unit(days / 365, 'year', 'years')
  if (days % 30 === 0) return unit(days / 30, 'month', 'months')
  if (days % 7 === 0) return unit(days / 7, 'week', 'weeks')
  return unit(days, 'day', 'days')
}

/** "since Sep 28" / "since today". */
export function sinceText(unix: number | null | undefined): string | null {
  if (!unix) return null
  return `since ${dayText(unix)}`
}

/** "today", "Sep 28", or "Sep 28, 2024" in another year. */
export function dayText(unix: number): string {
  const d = new Date(unix * 1000)
  const now = new Date()
  if (d.toDateString() === now.toDateString()) return 'today'
  const opts: Intl.DateTimeFormatOptions =
    d.getFullYear() === now.getFullYear() ? { month: 'short', day: 'numeric' } : { month: 'short', day: 'numeric', year: 'numeric' }
  return d.toLocaleDateString(undefined, opts)
}
