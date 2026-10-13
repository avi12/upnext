import type { Collection, Copy, GameView } from '../../types'
import { SOURCES, storeName, upperFirst } from '../../lib/format'

export function launcherName(launcher: string): string {
  switch (launcher) {
    case 'galaxy':
      return 'GOG Galaxy'
    case 'heroic':
      return 'Heroic'
    case 'steam':
      return 'Steam'
    default:
      return upperFirst(launcher)
  }
}

/** "GOG via Heroic", or just "Steam" when the store is its own launcher;
 *  "EA app, added by hand" for a game on a list typed in. */
export function storeVia(c: { source: string; launcher: string }): string {
  if (c.launcher === 'manual') return `${storeName(c.source)}, added by hand`
  return c.launcher === c.source ? storeName(c.source) : `${storeName(c.source)} via ${launcherName(c.launcher)}`
}

/** The title's editions for the sheet's header, as "2 editions: Original,
 *  Remastered", or the one edition every copy is ("Remastered"). Null when
 *  every copy is the plain release. A remaster is an edition of the same
 *  title, so it says so here rather than only in the copies list. */
export function editionsText(game: Pick<GameView, 'copies' | 'editions'>): string | null {
  const named = game.editions ?? []
  if (!named.length) return null
  const all = (game.copies ?? []).some((c) => !c.edition) ? ['Original', ...named] : named
  return all.length === 1 ? all[0] : `${all.length} editions: ${all.join(', ')}`
}

/** "Owned on Steam and Epic": every store the title is owned on, UpNext's
 *  own four first in their usual order, then any other by name, so no
 *  store is put first because Play launches it. */
export function ownedText(game: Pick<GameView, 'owned_on' | 'source'>): string {
  const own = (s: string) => SOURCES.indexOf(s as (typeof SOURCES)[number])
  const names = [...new Set(game.owned_on?.length ? game.owned_on : [game.source])]
    .sort((a, b) => {
      const [x, y] = [own(a), own(b)]
      if (x >= 0 || y >= 0) return (x < 0 ? 99 : x) - (y < 0 ? 99 : y)
      return storeName(a).localeCompare(storeName(b))
    })
    .map(storeName)
  const list = names.length > 1 ? `${names.slice(0, -1).join(', ')} and ${names[names.length - 1]}` : names.join('')
  return `Owned on ${list}`
}

/** Launchable copies, the preferred one first. */
export function playableCopies(game: GameView): Copy[] {
  const copies = (game.copies ?? []).filter((c) => c.run_url)
  return copies.sort((a, b) => (a.source === game.primary_source ? -1 : b.source === game.primary_source ? 1 : 0))
}

/** Where "Play" launches: the preferred copy's launch link, else the title's. */
export function playUrl(game: GameView): string | null {
  return playableCopies(game)[0]?.run_url || game.run_url || null
}

/** Steam reviews come with BBCode ([h1], [b], [url=…]); drop the tags. */
export function stripBBCode(text: string): string {
  return text
    .replace(/\[\/?(?:h\d|b|i|u|s|strike|spoiler|noparse|hr|list|olist|\*|quote|code|table|tr|td|th|url)(?:=[^\]]*)?\]/gi, '')
    .replace(/\r\n/g, '\n')
    .replace(/\n{3,}/g, '\n\n')
    .trim()
}

/** The collection this game belongs to, split into the main line and side entries. */
export function seriesFor(game: GameView, collections: Collection[] | undefined) {
  const c = collections?.find((x) => x.members.some((m) => m.id === game.id))
  if (!c) return null
  const byId = new Map(c.members.map((m) => [m.id, m]))
  let main: GameView[]
  if (c.guide && c.guide.order.length) {
    main = c.guide.order.map((id) => byId.get(id)).filter((m): m is GameView => !!m)
  } else {
    main = c.members
  }
  const mainIds = new Set(main.map((m) => m.id))
  const side = c.members.filter((m) => !mainIds.has(m.id))
  const pos = main.findIndex((m) => m.id === game.id)
  return { collection: c, main, side, position: pos >= 0 ? pos + 1 : null }
}
