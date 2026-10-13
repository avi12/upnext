import { memo } from 'react'
import type { GameView } from '../../types'
import { cx, hoursToText, linuxClass, linuxLabel, storeName } from '../../lib/format'
import type { LibrarySort, Order } from './filters'
import { gameYear, hoursMinutes } from '../../lib/gameText'
import { StatusDot } from './StatusDot'
import { RatingMark } from './RatingMark'
import { STAR_WORDS } from '../../lib/ratings'

interface Column {
  label: string
  /** None for a column the server can't sort by. */
  sort?: LibrarySort
  align?: 'right'
  /** Classes for the column's cells, header included. */
  className?: string
}

// Between 768 and 1023 px the table must fit the page without scrolling in
// its box, and there the rating's column would push it past the edge; the
// sort menu still sorts by it.
const RATING_CELL = 'md:max-lg:hidden'
// Other backlog players' headline needs the room only wide screens have.
const BACKLOG_CELL = 'max-xl:hidden'
const BACKLOG = 'Backlog players'

const COLUMNS: Column[] = [
  { label: 'Name', sort: 'name' },
  { label: 'Owned on', sort: 'store' },
  { label: 'Status', sort: 'status' },
  { label: 'Played', sort: 'playtime', align: 'right' },
  { label: 'Story', sort: 'hltb', align: 'right' },
  { label: 'Liked', sort: 'review', align: 'right' },
  { label: BACKLOG, className: BACKLOG_CELL },
  { label: 'Year', sort: 'release', align: 'right' },
  { label: 'Yours', sort: 'rating', align: 'right', className: RATING_CELL },
  { label: 'Linux', sort: 'linux' },
]

const TD = 'border-b border-line-soft py-3 pr-4 align-middle'
const NUM = 'text-right font-mono text-[11.5px]'

/** Table view. Every header is a sort button; clicking the active one flips it. */
/** One row. Memoised: opening a game re-renders the page, not every row. */
const TableRow = memo(function TableRow({
  g,
  linuxActive,
  backlogOn,
  onOpen,
}: {
  g: GameView
  linuxActive: boolean
  backlogOn: boolean
  onOpen: (g: GameView) => void
}) {
  const owned = g.owned_on?.length ? g.owned_on : [g.source]
  const story = hoursToText(g.hltb_main)
  const linux = linuxLabel(g.linux)
  return (
    <tr onClick={() => onOpen(g)} data-opens-game className="cursor-pointer hover:bg-raised/60">
      <td className={cx(TD, 'max-w-[28rem] font-semibold')}>
        <button
          type="button"
          onClick={(e) => {
            e.stopPropagation()
            onOpen(g)
          }}
          className="block max-w-full truncate text-left hover:underline hover:decoration-underline hover:underline-offset-4"
          title={g.name}
        >
          {g.name}
        </button>
      </td>
      <td className={cx(TD, 'text-muted')}>{owned.map(storeName).join(', ')}</td>
      <td className={cx(TD, 'whitespace-nowrap text-muted')}>
        <StatusDot status={g.status} />
      </td>
      <td className={cx(TD, NUM, 'whitespace-nowrap text-muted')} title={g.playtime_known ? undefined : 'Not tracked'}>
        {g.playtime_known && g.playtime_minutes > 0 ? hoursMinutes(g.playtime_minutes) : '—'}
      </td>
      <td className={cx(TD, NUM)}>{story ?? '—'}</td>
      <td className={cx(TD, NUM)} title={g.review_desc ?? undefined}>
        {g.review_pct != null ? `${g.review_pct}%` : '—'}
      </td>
      {backlogOn && (
        <td className={cx(TD, BACKLOG_CELL, 'whitespace-nowrap text-muted')} title={g.backlog?.headline}>
          {g.backlog?.short ?? '—'}
        </td>
      )}
      <td className={cx(TD, NUM, 'text-muted')}>{gameYear(g) || '—'}</td>
      <td className={cx(TD, NUM, 'whitespace-nowrap', RATING_CELL)} title={g.my_rating ? `You rated it ${g.my_rating}/5: ${STAR_WORDS[g.my_rating - 1]}` : undefined}>
        {g.my_rating ? <RatingMark stars={g.my_rating} /> : '—'}
      </td>
      {linuxActive && <td className={cx(TD, 'pr-0', linux ? linuxClass(g.linux) : 'text-faint')}>{linux ?? '—'}</td>}
    </tr>
  )
})

export function LibraryTable({
  items,
  sort,
  order,
  onSort,
  onOpen,
  linuxActive,
  backlogOn = false,
  loading,
  dimmed,
}: {
  items: GameView[]
  sort: LibrarySort
  order: Order
  onSort: (s: LibrarySort) => void
  onOpen: (g: GameView) => void
  linuxActive: boolean
  /** Other backlog players' verdicts are on: a column for their headline. */
  backlogOn?: boolean
  loading?: boolean
  dimmed?: boolean
}) {
  const cols = COLUMNS.filter((c) => (linuxActive || c.sort !== 'linux') && (backlogOn || c.label !== BACKLOG))
  // Below 768 px the table scrolls sideways in its own box, so the page
  // keeps the screen's width; its headers stop sticking there.
  return (
    <div className="max-md:relative max-md:overflow-x-auto">
      <table className={cx('w-full border-collapse text-[14.5px] transition-opacity', dimmed && 'opacity-70')}>
        <thead>
          <tr className="text-left font-mono text-[10.5px] text-dim">
            {cols.map((c, i) => {
              const active = c.sort === sort
              const last = i === cols.length - 1
              return (
                <th
                  key={c.label}
                  scope="col"
                  aria-sort={active ? (order === 'asc' ? 'ascending' : 'descending') : undefined}
                  className={cx(
                    'sticky top-[calc(var(--titlebar-h)+4rem)] z-10 border-b border-line-strong bg-ground pb-1 pt-2 font-normal',
                    !last && 'pr-4',
                    c.align === 'right' && 'text-right',
                    c.className,
                  )}
                >
                  {c.sort ? (
                    <button
                      type="button"
                      onClick={() => onSort(c.sort!)}
                      className={cx(
                        'inline-flex min-h-9 items-center gap-1.5 hover:text-ink',
                        active ? 'text-ink' : 'text-dim',
                      )}
                    >
                      {c.label}
                      {active && (
                        <span aria-hidden="true" className="text-accent">
                          {order === 'asc' ? '▲' : '▼'}
                        </span>
                      )}
                    </button>
                  ) : (
                    <span className="inline-flex min-h-9 items-center text-dim">{c.label}</span>
                  )}
                </th>
              )
            })}
          </tr>
        </thead>
        <tbody>
          {loading &&
            Array.from({ length: 14 }).map((_, i) => (
              <tr key={i} aria-hidden="true">
                <td className={TD} colSpan={cols.length}>
                  <div className="shimmer h-4 w-1/3 rounded" />
                </td>
              </tr>
            ))}
          {!loading &&
            items.map((g) => <TableRow key={g.id} g={g} linuxActive={linuxActive} backlogOn={backlogOn} onOpen={onOpen} />)}
        </tbody>
      </table>
    </div>
  )
}
