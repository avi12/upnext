import type { LinuxTier, Source } from '../types'

export function minutesToText(min: number): string {
  if (!min || min <= 0) return '0m'
  if (min < 60) return `${min}m`
  const h = min / 60
  if (h < 10) return `${h.toFixed(1).replace(/\.0$/, '')}h`
  return `${Math.round(h)}h`
}

export function hoursToText(h: number | null | undefined): string | null {
  if (h == null || h <= 0) return null
  // Minutes that round up to 60 are an hour.
  if (h < 1 && Math.round(h * 60) < 60) return `${Math.round(h * 60)}m`
  if (h < 10) return `${h.toFixed(1).replace(/\.0$/, '')}h`
  return `${Math.round(h)}h`
}

export function relativeTime(unix: number | null | undefined): string {
  if (!unix) return 'never'
  const diff = Date.now() / 1000 - unix
  if (diff < 60) return 'just now'
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`
  if (diff < 86400 * 30) return `${Math.floor(diff / 86400)}d ago`
  return new Date(unix * 1000).toLocaleDateString()
}

export function absoluteTime(unix: number | null | undefined): string {
  if (!unix) return ''
  return new Date(unix * 1000).toLocaleString(undefined, {
    dateStyle: 'medium',
    timeStyle: 'short',
  })
}

/** "250ms", "1.5s", "2m 5s". Each is rounded before it's split, so 59.99 s is "1m 0s", never "60.0s", and 119.6 s "2m 0s", never "1m 60s". */
export function durationText(ms: number): string {
  if (Math.round(ms) < 1000) return `${Math.round(ms)}ms`
  const tenths = Math.round(ms / 100)
  if (tenths < 600) return `${(tenths / 10).toFixed(1)}s`
  const s = Math.round(ms / 1000)
  const m = Math.floor(s / 60)
  return `${m}m ${s - m * 60}s`
}

/** "October 2026" for a YYYY-MM-DD date, read as UTC so no time zone moves the month. */
export function monthYear(date: string | null | undefined): string {
  if (!date) return ''
  const d = new Date(`${date.slice(0, 10)}T00:00:00Z`)
  if (Number.isNaN(d.getTime())) return date
  return d.toLocaleDateString('en-US', { month: 'long', year: 'numeric', timeZone: 'UTC' })
}

export function compactNumber(n: number | null | undefined): string {
  if (n == null) return ''
  return Intl.NumberFormat(undefined, { notation: 'compact', maximumFractionDigits: 1 }).format(n)
}

/** "1 game", "2,000 games": a count with its noun. */
export function plural(n: number, one: string, many = `${one}s`) {
  return `${n.toLocaleString()} ${n === 1 ? one : many}`
}

/** "No candidates" for "no candidates": a server message as a sentence. */
export function upperFirst(s: string): string {
  return s ? s[0].toUpperCase() + s.slice(1) : s
}

export function cx(...parts: Array<string | false | null | undefined>): string {
  return parts.filter(Boolean).join(' ')
}

export const SOURCES: Source[] = ['steam', 'epic', 'gog', 'amazon']

export function loadJson<T>(key: string, fallback: T): T {
  try {
    const raw = window.localStorage.getItem(key)
    if (raw == null) return fallback
    return JSON.parse(raw) as T
  } catch {
    return fallback
  }
}

export function saveJson(key: string, value: unknown): void {
  try {
    window.localStorage.setItem(key, JSON.stringify(value))
  } catch {
    /* ignore */
  }
}

export const LINUX_LABEL: Record<LinuxTier, string> = {
  native: 'Native',
  platinum: 'Platinum',
  gold: 'Gold',
  silver: 'Silver',
  bronze: 'Bronze',
  borked: 'Borked',
  pending: 'Pending',
}

/** "This morning's lineup", "Tonight's lineup"… by the clock. */
export function lineupTitle(now: Date = new Date()): string {
  const h = now.getHours()
  if (h >= 5 && h < 12) return "This morning's lineup"
  if (h >= 12 && h < 17) return "This afternoon's lineup"
  if (h >= 17 && h < 23) return "Tonight's lineup"
  return 'Late-night lineup'
}

/** "Tonight", "this afternoon"… for inline copy ("Back to tonight's lineup"). */
export function partOfDay(now: Date = new Date()): string {
  const h = now.getHours()
  if (h >= 5 && h < 12) return 'this morning'
  if (h >= 12 && h < 17) return 'this afternoon'
  if (h >= 17 && h < 23) return 'tonight'
  return 'late night'
}

/** Placeholder cover colour for a game: one of eight theme tones. */
export function toneVar(id: number): string {
  return `var(--tone-${Math.abs(id) % 8})`
}

/** Linux value as shown in the UI: "Native", "Platinum"… */
export function linuxLabel(tier: string | null | undefined): string | null {
  if (!tier) return null
  return upperFirst(tier)
}

/** Colour class for a Linux value: runs well, has issues, or neutral. */
export function linuxClass(tier: string | null | undefined): string {
  switch (tier) {
    case 'native':
    case 'platinum':
    case 'gold':
      return 'text-good'
    case 'silver':
    case 'bronze':
      return 'text-warn'
    case 'borked':
      return 'text-bad'
    default:
      return 'text-dim'
  }
}

/** The stores UpNext syncs itself, in the order lists show them. */
export const OWN_STORES: Source[] = ['steam', 'gog', 'epic', 'amazon']

/** The stores the library has games from: UpNext's own first, then any a
 *  launcher such as GOG Galaxy brought in (EA app, Xbox…), most games first.
 *  `always` keeps UpNext's own stores when they have none. */
export function libraryStores(bySource: Record<string, number> | undefined, always = false): string[] {
  const counts = bySource ?? {}
  const own = OWN_STORES.filter((s) => always || (counts[s] ?? 0) > 0)
  const others = Object.entries(counts)
    .filter(([s, n]) => n > 0 && !(OWN_STORES as string[]).includes(s))
    .sort(([a, x], [b, y]) => y - x || a.localeCompare(b))
    .map(([s]) => s)
  return [...own, ...others]
}

/** Store names as they appear in copy: Steam, GOG, Epic, Amazon, and the
 *  names of the stores Galaxy brings in (as `platform_label` in src/galaxy.rs). */
export function storeName(source: string): string {
  switch (source) {
    case 'steam':
      return 'Steam'
    case 'gog':
      return 'GOG'
    case 'epic':
      return 'Epic'
    case 'amazon':
      return 'Amazon'
    case 'origin':
    case 'eaApp':
      return 'EA app'
    case 'uplay':
      return 'Ubisoft Connect'
    case 'xboxone':
      return 'Xbox'
    case 'battlenet':
      return 'Battle.net'
    case 'psn':
      return 'PlayStation'
    case 'itch':
      return 'itch.io'
    case 'bethesda':
      return 'Bethesda.net'
    default:
      return upperFirst(source)
  }
}
