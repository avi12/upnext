import type { BacklogFilter, LinuxFilter, RatingFilter, SortKey, Status } from '../../types'
import { libraryStores, storeName } from '../../lib/format'

/** Status filter choices in the filter sentence. */
export type StatusChoice =
  | 'unplayed'
  | 'everything'
  | 'staples'
  | 'playing'
  | 'shelved'
  | 'played'
  | 'done'
  | 'still_playing'
  | 'burnt_out'
  | 'abandoned'
  | 'hidden'
  | 'not_game'

/** `staple`: your staples, whatever their status. */
export const STATUS_CHOICES: { value: StatusChoice; label: string; statuses?: Status[]; staple?: boolean }[] = [
  { value: 'unplayed', label: 'Unplayed', statuses: ['untouched', 'sampled'] },
  { value: 'everything', label: 'Everything' },
  { value: 'staples', label: 'Staples', staple: true },
  { value: 'playing', label: 'Playing', statuses: ['playing'] },
  { value: 'shelved', label: 'Shelved', statuses: ['shelved'] },
  { value: 'played', label: 'Played', statuses: ['played'] },
  { value: 'done', label: 'Beat it', statuses: ['done'] },
  { value: 'still_playing', label: 'Still playing', statuses: ['still_playing'] },
  { value: 'burnt_out', label: 'Burnt out', statuses: ['burnt_out'] },
  { value: 'abandoned', label: 'Abandoned', statuses: ['abandoned'] },
  { value: 'hidden', label: 'Not for me', statuses: ['hidden'] },
  { value: 'not_game', label: 'Not a game', statuses: ['not_game'] },
]

/** The store choices: any library, UpNext's own stores, then any other
 *  the library has games from (Galaxy's EA app or Xbox copies). "Library",
 *  not "store": it chooses which of your libraries, not a shop. */
export function storeChoices(bySource: Record<string, number> | undefined): { value: string; label: string }[] {
  return [{ value: '', label: 'any library' }, ...libraryStores(bySource, true).map((s) => ({ value: s, label: storeName(s) }))]
}

/** Read as "…games that {label}". */
export const LINUX_CHOICES: { value: '' | LinuxFilter; label: string }[] = [
  { value: '', label: 'run on Linux or not' },
  { value: 'ok', label: 'run well on Linux' },
  { value: 'native', label: 'run natively on Linux' },
  { value: 'issues', label: 'have issues on Linux' },
  { value: 'unknown', label: 'nobody has rated for Linux' },
]

/** Read as "…, {label}." in More filters. */
export const RATING_CHOICES: { value: '' | RatingFilter; label: string }[] = [
  { value: '', label: 'rated or not' },
  { value: 'rated', label: 'rated by you' },
  { value: 'high', label: 'rated 4 or 5 by you' },
  { value: 'low', label: 'rated 1 or 2 by you' },
  { value: 'unrated', label: 'not rated yet' },
]

/** Read as "Backlog players say {label}" in More filters. */
export const BACKLOG_CHOICES: { value: '' | BacklogFilter; label: string }[] = [
  { value: '', label: 'anything' },
  { value: 'respects_time', label: 'it respects limited time' },
  { value: 'clicks_fast', label: 'it clicks fast' },
]

/** Sorts the server knows, plus three the page sorts itself (store, status, linux). */
export type LibrarySort = Exclude<SortKey, 'source'> | 'store'
export type Order = 'asc' | 'desc'

/** How a sort reads in the sentence: `before` + [label] + `after`. */
interface SortPhrase {
  label: string
  before: string
  after: string
}

const PHRASES: Record<LibrarySort, Record<Order, SortPhrase>> = {
  review: {
    desc: { before: '', label: 'best reviewed', after: ' first' },
    asc: { before: '', label: 'worst reviewed', after: ' first' },
  },
  name: {
    asc: { before: 'sorted ', label: 'A to Z', after: '' },
    desc: { before: 'sorted ', label: 'Z to A', after: '' },
  },
  hltb: {
    asc: { before: '', label: 'shortest', after: ' first' },
    desc: { before: '', label: 'longest', after: ' first' },
  },
  release: {
    desc: { before: '', label: 'newest', after: ' first' },
    asc: { before: '', label: 'oldest', after: ' first' },
  },
  playtime: {
    desc: { before: '', label: 'most played', after: ' first' },
    asc: { before: '', label: 'least played', after: ' first' },
  },
  last_played: {
    desc: { before: '', label: 'recently played', after: ' first' },
    asc: { before: '', label: 'longest untouched', after: ' first' },
  },
  metacritic: {
    desc: { before: '', label: 'best Metacritic score', after: ' first' },
    asc: { before: '', label: 'worst Metacritic score', after: ' first' },
  },
  rating: {
    desc: { before: '', label: 'your highest rated', after: ' first' },
    asc: { before: '', label: 'your lowest rated', after: ' first' },
  },
  store: {
    asc: { before: 'sorted by ', label: 'store', after: '' },
    desc: { before: 'sorted by ', label: 'store, reversed', after: '' },
  },
  status: {
    asc: { before: 'sorted by ', label: 'status', after: '' },
    desc: { before: 'sorted by ', label: 'status, reversed', after: '' },
  },
  linux: {
    asc: { before: 'sorted by ', label: 'Linux support', after: '' },
    desc: { before: 'sorted by ', label: 'Linux support, worst first', after: '' },
  },
}

export function sortPhrase(sort: LibrarySort, order: Order): SortPhrase {
  return PHRASES[sort][order]
}

/** The sorts offered in the sentence menu; the current one is added if it isn't here. */
export const SORT_MENU: [LibrarySort, Order][] = [
  ['review', 'desc'],
  ['name', 'asc'],
  ['hltb', 'asc'],
  ['release', 'desc'],
  ['playtime', 'desc'],
  ['last_played', 'desc'],
  ['metacritic', 'desc'],
  ['rating', 'desc'],
]

/** Order a column or menu choice starts with. */
export function defaultOrder(sort: LibrarySort): Order {
  return sort === 'name' || sort === 'hltb' || sort === 'store' || sort === 'status' || sort === 'linux' ? 'asc' : 'desc'
}

const SORT_KEYS: LibrarySort[] = Object.keys(PHRASES) as LibrarySort[]
export function isLibrarySort(v: string | null): v is LibrarySort {
  return v != null && (SORT_KEYS as string[]).includes(v)
}


