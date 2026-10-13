import type { GameView, Source, WishGame } from '../../types'
import { SOURCES, compactNumber, hoursToText, linuxLabel, minutesToText, storeName } from '../../lib/format'
import { yearOf } from '../../lib/gameText'
import { dayText } from '../../lib/statuses'

export function storesText(g: Pick<GameView, 'owned_on' | 'source'>): string {
  const stores = g.owned_on?.length ? g.owned_on : [g.source]
  return stores.map(storeName).join(', ')
}

/** "2.1h story · 96% liked it (53K) · Steam · 2017" */
export function metaLine(g: GameView): string {
  const parts: string[] = []
  const story = hoursToText(g.hltb_main)
  if (story) parts.push(`${story} story`)
  if (g.review_pct != null) {
    parts.push(g.review_total ? `${g.review_pct}% liked it (${compactNumber(g.review_total)})` : `${g.review_pct}% liked it`)
  }
  parts.push(storesText(g))
  const year = yearOf(g)
  if (year) parts.push(String(year))
  return parts.join(' · ')
}

/** Linux value worth showing, or null (unknown or still pending). */
export function linuxShown(g: Pick<GameView, 'linux'>): string | null {
  if (!g.linux || g.linux === 'pending') return null
  return linuxLabel(g.linux)
}


/** Why a game you've played is in the lineup: "Shelved · last played Mar 3
 *  · 14h in", "A second chance · put down in 2022", "One of your staples ·
 *  140h", "Endless · 40h played". Null for a game you haven't started. */
export function historyLine(g: GameView): string | null {
  const played = g.playtime_known && g.playtime_minutes > 0 ? minutesToText(g.playtime_minutes) : null
  const line = (...parts: (string | null | false)[]) => parts.filter(Boolean).join(' · ')
  const putDown = g.last_played > 0 && `put down in ${new Date(g.last_played * 1000).getFullYear()}`
  switch (g.status) {
    case 'shelved':
      if (g.second_try) return line('The second chance you said yes to', putDown, played && `${played} in`)
      return line('Shelved', g.last_played > 0 && `last played ${dayText(g.last_played)}`, played && `${played} in`)
    case 'abandoned':
      return line('A second chance', putDown, played && `${played} in`)
  }
  if (g.staple) return line('One of your staples', played)
  if (g.kind === 'endless') return line('Endless', played && `${played} played`)
  return null
}

/** "2025", from Steam's "Sep 4, 2025" (or "Coming soon" as it is). */
function yearText(g: WishGame): string | null {
  return g.release_date?.match(/\b(19|20)\d\d\b/)?.[0] ?? g.release_date ?? null
}

/** "30h story · 95% liked it (2K) · 2025" */
export function wishMeta(g: WishGame): string {
  const story = hoursToText(g.hltb_main)
  return [
    story && `${story} story`,
    g.review_pct != null &&
      (g.review_total ? `${g.review_pct}% liked it (${compactNumber(g.review_total)})` : `${g.review_pct}% liked it`),
    yearText(g),
  ]
    .filter(Boolean)
    .join(' · ')
}

/** Art for a game you don't own, from what the store gave. */
export function wishCover(g: WishGame) {
  return { id: g.appid, name: g.name, appid: g.appid, art_square: g.portrait, header_image: g.header_image ?? '' }
}

/** Which of your libraries picks come from: "Any library", "From Steam",
 *  "From Steam and GOG". "Library", not "store": it isn't a shop. */
export function storesLabel(sources: Source[]): string {
  if (sources.length === SOURCES.length) return 'Any library'
  const names = sources.map(storeName)
  if (names.length === 1) return `From ${names[0]}`
  return `From ${names.slice(0, -1).join(', ')} and ${names[names.length - 1]}`
}
