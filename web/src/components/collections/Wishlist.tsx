import { useState, type FormEvent, type ReactNode } from 'react'
import { Link } from 'react-router'
import type { Job, WishlistEntry, WishlistResponse, WishState } from '../../types'
import { Cover } from '../Cover'
import { compactNumber, cx, hoursToText, plural } from '../../lib/format'
import { dayText } from '../../lib/statuses'
import { Place } from '../Place'

export type WishAnswer = Exclude<WishState, 'in_library'>

/** "1999", from Steam's "12 Nov, 1999" or "Coming soon". */
function yearOf(e: WishlistEntry): string | null {
  return e.release_date?.match(/\b(19|20)\d\d\b/)?.[0] ?? e.release_date ?? null
}

function meta(e: WishlistEntry): string {
  return [
    yearOf(e),
    e.developers[0],
    hoursToText(e.hltb_main),
    e.review_pct != null && e.review_total ? `${e.review_pct}% of ${compactNumber(e.review_total)} ${e.review_total === 1 ? 'review' : 'reviews'}` : null,
    e.tags.slice(0, 3).join(', ') || e.genres.slice(0, 3).join(', ') || null,
  ]
    .filter(Boolean)
    .join(' · ')
}

/** Where a wishlisted game came from, for the line under its name. */
function origin(e: WishlistEntry): string | null {
  const when = e.wishlisted_at ? ` ${dayText(e.wishlisted_at)}` : ''
  if (e.source === 'collection') return `Suggested from your ${e.series ?? 'series'}, wishlisted${when}`
  if (e.source === 'pick') return `From a pick, wishlisted${when}`
  if (e.source === 'manual') return `You added it${when}`
  return null
}

const link = 'btn-quiet text-ink underline decoration-underline underline-offset-4 disabled:opacity-50'

function WishRow({ e, children, note }: { e: WishlistEntry; children: ReactNode; note?: ReactNode }) {
  const cover = { id: e.appid, name: e.name, appid: e.appid, art_square: e.portrait, header_image: e.header_image }
  const m = meta(e)
  return (
    <li className="flex flex-wrap items-center gap-x-6 gap-y-3 border-b border-line py-4">
      <Cover game={cover} className="w-16 shrink-0 rounded-[8px]" titleSize="text-[10px]" />
      <div className="flex min-w-0 flex-1 basis-56 flex-col gap-1.5">
        <h3 className="m-0 font-display text-[19px] leading-[1.2] font-bold tracking-[-0.01em]">{e.name}</h3>
        {note}
        {e.reason && <p className="m-0 max-w-2xl text-[15px] leading-[1.5] text-ink">{e.reason}</p>}
        {e.short_description && <p className="m-0 line-clamp-2 max-w-2xl text-[14px] leading-[1.5] text-muted">{e.short_description}</p>}
        {m && <div className="mono">{m}</div>}
      </div>
      <div className="flex flex-wrap items-center gap-x-5 gap-y-2 text-[14.5px]">{children}</div>
    </li>
  )
}

function AddByName({ onAdd }: { onAdd: (name: string) => Promise<boolean> }) {
  const [name, setName] = useState('')
  const [adding, setAdding] = useState(false)
  const submit = async (ev: FormEvent) => {
    ev.preventDefault()
    const n = name.trim()
    if (!n) return
    setAdding(true)
    if (await onAdd(n)) setName('')
    setAdding(false)
  }
  return (
    <form onSubmit={(ev) => void submit(ev)} className="flex max-w-[560px] flex-wrap items-center gap-3 pt-3">
      <label htmlFor="wish-add" className="sr-only">
        A game to add, by name
      </label>
      <input
        id="wish-add"
        value={name}
        onChange={(ev) => setName(ev.target.value)}
        placeholder="Add a game by name"
        autoComplete="off"
        className="field h-11 min-w-0 flex-1 basis-56"
      />
      <button type="submit" className="btn-outline h-11" disabled={!name.trim() || adding}>
        {adding ? 'Looking it up…' : 'Add'}
      </button>
    </form>
  )
}

/** Lists, Wishlist: suggested games you don't own, from your
 *  series, and the wishlist itself, Steam's included. It's not a store, so
 *  there are no links, prices or sales. */
export function Wishlist({
  data,
  running,
  busy,
  onRun,
  onAnswer,
  onAdd,
}: {
  data: WishlistResponse
  running: Job | null
  busy: Set<number>
  onRun: () => void
  onAnswer: (e: WishlistEntry, a: WishAnswer) => void
  onAdd: (name: string) => Promise<boolean>
}) {
  const { suggested, wishlist, dismissed, owned_elsewhere, in_library, task, rules, steam_account } = data
  const updating = running?.task === 'wishlist'
  const status = updating
    ? 'Updating now…'
    : <>{task?.last_run ? `Last updated ${dayText(task.last_run.started_at)}` : 'Not updated yet'}{
        task?.enabled && task.next_run_at && task.schedule !== 'off'
          ? ` · next ${dayText(task.next_run_at)}`
          : task?.schedule === 'after:details'
            ? task.enabled
              ? '. It updates after each Fetch details, or when you ask.'
              : <>. It updates when you ask, or after each Fetch details once scheduled jobs are on in <Place section="Jobs" />.</>
            : <>. It updates when you ask, or on a schedule you set in <Place section="Jobs" />.</>
      }</>
  const answered = [
    dismissed > 0 && `${dismissed} you said no to`,
    owned_elsewhere > 0 && `${owned_elsewhere} you own elsewhere`,
    in_library > 0 && `${in_library} in your library now`,
  ].filter(Boolean)

  return (
    <>
      <div className="flex max-w-[900px] flex-col gap-2 pb-2 text-[15.5px] leading-[1.6] text-muted">
        <p className="m-0 text-[17px]">
          Games you don’t own. UpNext suggests the ones from series that mean something to you: {plural(rules?.series_hours ?? 10, 'hour')} or more
          played across the series, an entry beaten, or a favourite. Each is on Steam’s store and rated well by{' '}
          {rules?.min_positive ?? 80}% or more of at least {plural(rules?.min_reviews ?? 50, 'player')}, with up to {rules?.per_series ?? 3} from a
          series waiting at once.
          {!steam_account && (
            <>
              {' '}
              Your Steam wishlist shows here once your Steam account is set up in{' '}
              <Link to="/settings#accounts" className="text-ink underline decoration-underline underline-offset-4">
                Settings
              </Link>
              .
            </>
          )}
        </p>
        <p className="m-0 flex flex-wrap items-center gap-x-3 gap-y-1">
          <span>{status}</span>
          <button type="button" onClick={onRun} disabled={!!running} className={link}>
            Update now
          </button>
        </p>
      </div>

      <h2 className="m-0 pt-6 pb-1 font-display text-[22px] font-bold tracking-[-0.01em]">
        Suggested <span className="font-mono text-[12px] font-normal text-dim">{suggested.length}</span>
      </h2>
      {suggested.length === 0 ? (
        <p className="m-0 py-3 text-[15.5px] text-muted">
          Nothing waiting. New suggestions come when a series you care about has a game you don’t own.
        </p>
      ) : (
        <ul aria-label="Suggested" className="max-w-[1100px]">
          {suggested.map((e) => (
            <WishRow key={e.id} e={e}>
              <button type="button" className={link} disabled={busy.has(e.id)} onClick={() => onAnswer(e, 'wishlisted')}>
                Add to wishlist
              </button>
              <button type="button" className={link} disabled={busy.has(e.id)} onClick={() => onAnswer(e, 'dismissed')}>
                Not interested
              </button>
              <button type="button" className={link} disabled={busy.has(e.id)} onClick={() => onAnswer(e, 'owned_elsewhere')}>
                I own it elsewhere
              </button>
            </WishRow>
          ))}
        </ul>
      )}

      <h2 className="m-0 pt-8 pb-1 font-display text-[22px] font-bold tracking-[-0.01em]">
        Your wishlist <span className="font-mono text-[12px] font-normal text-dim">{wishlist.length}</span>
      </h2>
      {wishlist.length === 0 && (
        <p className="m-0 py-3 text-[15.5px] text-muted">
          Nothing on it yet. Add a suggestion, or a game by name{steam_account ? '; your Steam wishlist shows here too' : ''}.
        </p>
      )}
      {wishlist.length > 0 && (
        <ul aria-label="Your wishlist" className="max-w-[1100px]">
          {wishlist.map((e) => {
            const from = origin(e)
            return (
              <WishRow
                key={e.id}
                e={e}
                note={
                  (e.on_steam || from) && (
                    <p className="m-0 flex flex-wrap items-center gap-x-3 text-[13.5px] text-dim">
                      {e.on_steam && <span className={cx('rounded-full border border-line px-2 py-0.5 text-ink')}>On your Steam wishlist</span>}
                      {from && <span>{from}</span>}
                    </p>
                  )
                }
              >
                <button
                  type="button"
                  className={link}
                  disabled={busy.has(e.id)}
                  title={e.on_steam ? 'It stays on your Steam wishlist; UpNext only reads that' : undefined}
                  onClick={() => onAnswer(e, 'dismissed')}
                >
                  {e.on_steam ? 'Remove here' : 'Remove'}
                </button>
                <button type="button" className={link} disabled={busy.has(e.id)} onClick={() => onAnswer(e, 'owned_elsewhere')}>
                  I own it elsewhere
                </button>
              </WishRow>
            )
          })}
        </ul>
      )}
      <AddByName onAdd={onAdd} />
      {answered.length > 0 && (
        <p className="m-0 max-w-[900px] pt-6 text-[14px] text-dim">
          Not shown: {answered.join(', ')}. None of them is suggested again. Delete what UpNext keeps under{' '}
          <Place section="Profile" />
          .
        </p>
      )}
    </>
  )
}
