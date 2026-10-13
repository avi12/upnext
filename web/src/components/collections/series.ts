import { yearOf as baseYearOf } from '../../lib/gameText'
import type { Collection, GameView, SeriesEntry, SideKind } from '../../types'

export type SeriesShow = 'started' | 'all' | 'unstarted' | 'finished'
export type SeriesOrder = 'play' | 'release'

export const SHOW_LABEL: Record<SeriesShow, string> = {
  started: "Series you've started",
  all: 'All series',
  unstarted: 'Series not started yet',
  finished: 'Series you finished',
}

export const ORDER_LABEL: Record<SeriesOrder, string> = {
  play: 'play order',
  release: 'release order',
}

/** Touched in any way: sampled, played, beaten, put down or still going. */
export function isStarted(g: GameView): boolean {
  return g.status !== 'untouched' && g.status !== 'hidden' && g.status !== 'not_game'
}

function isFinished(g: GameView): boolean {
  return g.status === 'played' || g.status === 'done'
}

/** Original release year: Wikidata's per-series year, then the game's own. */
export function yearOf(c: Collection, g: GameView): number | null {
  return c.years[String(g.id)] ?? baseYearOf(g)
}

export interface SideGroup {
  label: string
  games: GameView[]
}

/** One cover in a series row: a game you own, or one you don't. */
export type RowItem = { kind: 'owned'; g: GameView } | { kind: 'missing'; e: SeriesEntry }

export interface Series {
  c: Collection
  /** Main-line games, in the chosen order. */
  main: GameView[]
  /** The main line with the games you don't own placed by release year. */
  row: RowItem[]
  side: SideGroup[]
  started: number
  finished: boolean
}

const SIDE_GROUPS: { kinds: SideKind[]; one: string; many: string }[] = [
  { kinds: ['spin-off'], one: 'Spin-off', many: 'Spin-offs' },
  { kinds: ['remaster', 'duplicate'], one: 'Remaster or duplicate', many: 'Remasters and duplicates' },
  { kinds: ['expansion'], one: 'Expansion', many: 'Expansions' },
  { kinds: ['multiplayer'], one: 'Multiplayer', many: 'Multiplayer' },
  { kinds: ['tool'], one: 'Tool', many: 'Tools' },
  { kinds: ['other'], one: 'Also in the series', many: 'Also in the series' },
]

export function buildSeries(c: Collection, order: SeriesOrder): Series {
  const byId = new Map(c.members.map((m) => [m.id, m]))
  const kindOf = new Map<number, SideKind>()
  for (const s of c.guide?.side ?? []) kindOf.set(s.id, s.kind)

  let main: GameView[]
  if (c.guide) {
    main = c.guide.order.map((id) => byId.get(id)).filter((g): g is GameView => !!g)
    // Owned games the guide doesn't mention (bought after it was written).
    const placed = new Set([...c.guide.order, ...kindOf.keys()])
    for (const m of c.members) if (!placed.has(m.id)) kindOf.set(m.id, 'other')
  } else {
    main = [...c.members]
  }

  if (order === 'release') {
    main = main
      .map((g, i) => ({ g, i, y: yearOf(c, g) ?? 9999 }))
      .sort((a, b) => a.y - b.y || a.i - b.i)
      .map((x) => x.g)
  }

  const side: SideGroup[] = []
  for (const grp of SIDE_GROUPS) {
    const games = c.members.filter((m) => {
      const k = kindOf.get(m.id)
      return k != null && grp.kinds.includes(k)
    })
    if (games.length) side.push({ label: games.length === 1 ? grp.one : grp.many, games })
  }

  // Games you don't own go by year: after the last owned game released no
  // later than they were (in whichever order the owned ones are shown).
  const row: RowItem[] = main.map((g) => ({ kind: 'owned', g }))
  for (const e of c.missing ?? []) {
    const y = e.year ?? 9999
    let at = 0
    row.forEach((item, i) => {
      const iy = item.kind === 'owned' ? (yearOf(c, item.g) ?? 9999) : (item.e.year ?? 9999)
      if (iy <= y) at = i + 1
    })
    row.splice(at, 0, { kind: 'missing', e })
  }

  const started = main.filter(isStarted).length
  return { c, main, row, side, started, finished: main.length > 0 && main.every(isFinished) }
}

export function matchesShow(s: Series, show: SeriesShow): boolean {
  switch (show) {
    case 'all':
      return true
    case 'started':
      return s.started > 0
    case 'unstarted':
      return s.started === 0
    case 'finished':
      return s.finished
  }
}


export function hltbShort(h: number | null): string | null {
  if (h == null || h <= 0) return null
  return `${Math.round(h)}h`
}
