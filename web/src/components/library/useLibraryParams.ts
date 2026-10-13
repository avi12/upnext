import { useCallback, useEffect } from 'react'
import { useSearchParams } from 'react-router'
import type { BacklogFilter, GamesQuery, LinuxFilter, RatingFilter } from '../../types'
import { useDebounced } from '../../lib/useDebounced'
import { BACKLOG_CHOICES, LINUX_CHOICES, RATING_CHOICES, STATUS_CHOICES, defaultOrder, isLibrarySort, type LibrarySort, type Order, type StatusChoice } from './filters'

export type View = 'covers' | 'table'
const VIEW_KEY = 'upnext.library.view'

function storedView(): View | null {
  try {
    const v = window.localStorage.getItem(VIEW_KEY)
    return v === 'table' || v === 'covers' ? v : null
  } catch {
    return null
  }
}

function rememberView(v: View) {
  try {
    window.localStorage.setItem(VIEW_KEY, v)
  } catch {
    /* private mode */
  }
}

// Defaults follow the view: covers are for choosing, the table is the whole collection.
const DEFAULTS: Record<View, { status: StatusChoice; sort: LibrarySort }> = {
  covers: { status: 'unplayed', sort: 'review' },
  table: { status: 'everything', sort: 'name' },
}

/** The library's filters, as the address holds them. */
export interface LibraryParams {
  view: View
  defaults: { status: StatusChoice; sort: LibrarySort }
  status: StatusChoice
  /** A store id (steam, origin…), or '' for any library. */
  store: string
  linuxChoice: '' | LinuxFilter
  genre: string
  tag: string
  // Review tags: one value each, as the vocabulary spells it.
  difficulty: string
  sessions: string
  mood: string
  praised: string
  notCriticized: string
  installed: boolean
  /** Your own rating, or '' for rated or not. */
  rating: '' | RatingFilter
  /** What most other backlog players said, or '' for anything. */
  backlog: '' | BacklogFilter
  sort: LibrarySort
  order: Order
}

/** Read the filters from the address; anything unknown falls back to the view's defaults. */
export function readLibraryParams(params: URLSearchParams, linuxActive: boolean): LibraryParams {
  const viewParam = params.get('view')
  const view: View = viewParam === 'table' || viewParam === 'covers' ? viewParam : (storedView() ?? 'covers')
  const defaults = DEFAULTS[view]
  const statusParam = params.get('status')
  const status: StatusChoice = STATUS_CHOICES.some((c) => c.value === statusParam) ? (statusParam as StatusChoice) : defaults.status
  const storeParam = params.get('store')
  // Any store id: the stores Galaxy brings in are known once the library's counts load.
  const store = storeParam && /^[a-zA-Z0-9]{1,32}$/.test(storeParam) ? storeParam : ''
  const linuxParam = params.get('linux')
  const linuxChoice: '' | LinuxFilter = LINUX_CHOICES.some((c) => c.value === linuxParam) ? (linuxParam as LinuxFilter) : ''
  const ratingParam = params.get('rating')
  const rating: '' | RatingFilter = RATING_CHOICES.some((c) => c.value === ratingParam) ? (ratingParam as RatingFilter) : ''
  const backlogParam = params.get('backlog')
  const backlog: '' | BacklogFilter = BACKLOG_CHOICES.some((c) => c.value === backlogParam) ? (backlogParam as BacklogFilter) : ''
  const sortParam = params.get('sort')
  // A leftover Linux sort is dropped too once Linux mode is off.
  const sort: LibrarySort = isLibrarySort(sortParam) && (sortParam !== 'linux' || linuxActive) ? sortParam : defaults.sort
  const orderParam = params.get('order')
  const order: Order = orderParam === 'asc' || orderParam === 'desc' ? orderParam : defaultOrder(sort)
  return {
    view,
    defaults,
    status,
    store,
    linuxChoice,
    genre: params.get('genre') ?? '',
    tag: params.get('tag') ?? '',
    difficulty: params.get('difficulty') ?? '',
    sessions: params.get('sessions') ?? '',
    mood: params.get('mood') ?? '',
    praised: params.get('praised') ?? '',
    notCriticized: params.get('not_criticized') ?? '',
    installed: params.get('installed') === '1',
    rating,
    backlog,
    sort,
    order,
  }
}

/** The games request for these filters (without sort and paging). */
export function gamesQuery(p: LibraryParams, q: string, linuxActive: boolean): GamesQuery {
  return {
    status: STATUS_CHOICES.find((c) => c.value === p.status)?.statuses,
    staple: STATUS_CHOICES.find((c) => c.value === p.status)?.staple,
    source: p.store ? [p.store] : undefined,
    installed: p.installed || undefined,
    rating: p.rating || undefined,
    backlog: p.backlog || undefined,
    q: q || undefined,
    genre: p.genre || undefined,
    tag: p.tag || undefined,
    // Ignore a leftover Linux filter once Linux mode is off.
    linux: (linuxActive && p.linuxChoice) || undefined,
    facet: [p.difficulty && `difficulty:${p.difficulty}`, p.sessions && `sessions:${p.sessions}`, p.mood && `mood:${p.mood}`].filter(
      (f): f is string => !!f,
    ),
    praised: p.praised ? [p.praised] : undefined,
    not_criticized: p.notCriticized ? [p.notCriticized] : undefined,
  }
}

/** The library's filters in the address, with the debounced search and the ways to change them. */
export function useLibraryParams(linuxActive: boolean) {
  const [params, setParams] = useSearchParams()
  const p = readLibraryParams(params, linuxActive)
  const q = useDebounced((params.get('q') ?? '').trim(), 250)
  const viewParam = params.get('view')

  /** Write some params (null/'' removes). Each change is a history entry, so Back undoes it. */
  const update = useCallback(
    (patch: Record<string, string | null>) => {
      setParams((cur) => {
        const next = new URLSearchParams(cur)
        for (const [k, v] of Object.entries(patch)) {
          if (v == null || v === '') next.delete(k)
          else next.set(k, v)
        }
        return next
      })
    },
    [setParams],
  )

  // Remember a view that came in through the URL (e.g. /collection -> ?view=table).
  useEffect(() => {
    if (viewParam === 'table' || viewParam === 'covers') rememberView(viewParam)
  }, [viewParam])

  const setView = (v: View) => {
    rememberView(v)
    update({ view: v })
  }
  const setSort = (s: LibrarySort, o: Order) => update({ sort: s, order: o })

  const clearFilters = () =>
    update({
      status: null,
      store: null,
      linux: null,
      genre: null,
      tag: null,
      installed: null,
      rating: null,
      backlog: null,
      q: null,
      difficulty: null,
      sessions: null,
      mood: null,
      praised: null,
      not_criticized: null,
    })

  return { p, q, update, setView, setSort, clearFilters }
}

/** Which filters are set: beyond the first sentence, at all, or none. */
export function filterState(p: LibraryParams, q: string, linuxActive: boolean) {
  const reviewActive = !!p.difficulty || !!p.sessions || !!p.mood || !!p.praised || !!p.notCriticized
  const extraActive = !!p.genre || !!p.tag || p.installed || !!p.rating || !!p.backlog || reviewActive
  const unfiltered = p.status === 'everything' && !p.store && !extraActive && !q && !(linuxActive && p.linuxChoice)
  const anyFilter = p.status !== p.defaults.status || !!p.store || extraActive || !!(linuxActive && p.linuxChoice)
  return { reviewActive, extraActive, unfiltered, anyFilter }
}
