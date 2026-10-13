import { useCallback, useMemo, useState } from 'react'
import { Link, useSearchParams } from 'react-router'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { api } from '../api'
import type { GameView, JobsResponse, OverrideStatus, SecondChanceAnswer, WishlistEntry, WishlistResponse } from '../types'
import { GameSheet } from '../components/GameSheet'
import { StatusMenu, type Reasons } from '../components/StatusMenu'
import { useToast } from '../components/useToast'
import { PLAY_LATER_KEY } from '../components/pick/useGameActions'
import { useOfferNextInSeries } from '../components/nextInSeries'
import { SentenceChoice } from '../components/SentenceMenu'
import { LaterRow } from '../components/collections/LaterRow'
import { SecondChanceRow } from '../components/collections/SecondChanceRow'
import { HiddenGems } from '../components/collections/HiddenGems'
import { Wishlist, type WishAnswer } from '../components/collections/Wishlist'
import { ApiError } from '../api'
import { cx, hoursToText, minutesToText } from '../lib/format'
import { OVERRIDES, dayText, overrideMeta } from '../lib/statuses'
import { Place } from '../components/Place'
import { HiddenByMe } from '../components/collections/HiddenByMe'
import { hiddenLocked, usePlayers } from '../lib/players'
import { keys, useJobs, useSettings } from '../lib/queries'

type ListKey = 'later' | 'gems' | 'wishlist' | 'staples' | 'hidden_by_me' | OverrideStatus
type LaterSort = 'recent' | 'shortest'

const LATER_SORT: { value: LaterSort; label: string }[] = [
  { value: 'recent', label: 'recently saved first' },
  { value: 'shortest', label: 'shortest first' },
]

/** The sentence under the switch, e.g. "3 games you're still playing". */
const PHRASE: Record<ListKey, string> = {
  later: 'saved for later',
  gems: 'that are hidden gems',
  wishlist: "you don't own",
  staples: 'you keep coming back to',
  hidden_by_me: 'you hid',
  done: "you've beaten",
  shelved: 'shelved',
  abandoned: 'abandoned',
  still_playing: "you're still playing",
  burnt_out: "you're burnt out on",
  hidden: 'marked not for you',
}

/** What follows the sentence, where there's more to say. */
const AFTER: Partial<Record<ListKey, string>> = {
  burnt_out: ' Each comes back to picks when its break ends.',
  hidden: " They're never recommended.",
}

const EMPTY: Record<ListKey, string> = {
  later: 'Nothing saved yet. Hit Save for later on a pick to keep it here.',
  gems: 'No hidden gems yet.',
  wishlist: 'Nothing on the wishlist.',
  staples: 'No staples yet. A game you’ve played for 100 hours or more (a setting, under Picks) is one.',
  hidden_by_me: 'Nothing hidden.',
  done: 'Nothing marked as beaten yet. Use Mark as… › Beat it when you finish something.',
  shelved: "Nothing shelved. A game you started and haven't finished lands here once it's been untouched for a while.",
  abandoned: 'Nothing abandoned.',
  still_playing: 'Nothing marked as still playing. It’s for endless games you go back to now and then.',
  burnt_out: 'Nothing burnt out.',
  hidden: 'Nothing marked not for you.',
}

/** Most recent first: when you marked it, else when UpNext found it so. */
const recency = (g: GameView) => g.override_at ?? g.shelved_at ?? g.abandoned_at ?? g.last_played ?? 0


function isList(v: string | null): v is ListKey {
  return v === 'later' || v === 'gems' || v === 'wishlist' || v === 'staples' || v === 'hidden_by_me' || OVERRIDES.some((o) => o.value === v)
}

/** Second chance was a tab of its own; it is now the top of Abandoned, the
 *  list it was drawn from, and old links still land there. */
const listOf = (v: string | null) => (v === 'second_chance' ? 'abandoned' : v)

const WISH_DONE: Record<WishAnswer, string> = {
  suggested: 'back with the suggestions',
  wishlisted: 'on your wishlist',
  dismissed: 'not suggested again',
  owned_elsewhere: 'noted that you own it, so not suggested again',
}

const SECOND_CHANCE_DONE: Record<SecondChanceAnswer, string> = {
  willing: 'back on your shelf, for one more try',
  still_no: 'out of picks for good',
  not_for_me: 'never recommended again',
}

/** Your lists: Play later plus every status you've set, so a game you marked
 *  by accident (or on purpose) is always one click away. */
export function ListsPage() {
  const qc = useQueryClient()
  const toast = useToast()
  const offerNext = useOfferNextInSeries()
  const [params, setParams] = useSearchParams()
  const settings = useSettings()
  const wishOn = settings.data?.wishlist !== false
  const listParam = listOf(params.get('list'))
  const list: ListKey = isList(listParam) && (listParam !== 'wishlist' || wishOn) ? listParam : 'later'
  const sort: LaterSort = params.get('sort') === 'shortest' ? 'shortest' : 'recent'
  const [sheet, setSheet] = useState<GameView | null>(null)
  const [busy, setBusy] = useState<Set<number>>(() => new Set())

  const later = useQuery({ queryKey: PLAY_LATER_KEY, queryFn: api.playLater })
  const stats = useQuery({ queryKey: ['stats'], queryFn: api.stats, staleTime: 30_000 })
  const byStatus = list !== 'later' && list !== 'gems' && list !== 'wishlist' && list !== 'staples' && list !== 'hidden_by_me'
  const status = useQuery({
    queryKey: ['games', 'list', list],
    queryFn: () => api.games({ status: [list as OverrideStatus], limit: 1000 }),
    enabled: byStatus,
  })
  const staples = useQuery({
    queryKey: ['games', 'staples'],
    queryFn: () => api.games({ staple: true, sort: 'playtime', order: 'desc', limit: 1000 }),
    staleTime: 30_000,
  })
  const second = useQuery({ queryKey: ['second-chances'], queryFn: api.secondChances, staleTime: 30_000 })
  const gems = useQuery({ queryKey: ['gems'], queryFn: () => api.gems(), staleTime: 30_000 })
  const wish = useQuery({ queryKey: ['wishlist'], queryFn: api.wishlist, staleTime: 30_000, enabled: wishOn })
  // Hidden by me: a tab with two or more players, or once you've hidden something.
  const { multi } = usePlayers()
  const hidden = useQuery({ queryKey: ['hidden'], queryFn: api.hidden, staleTime: 30_000, retry: false })
  const hiddenTab = multi || (hidden.data?.length ?? 0) > 0 || hiddenLocked(hidden)
  const reasons = useQuery({ queryKey: ['stop-reasons'], queryFn: api.stopReasons, staleTime: Infinity, enabled: list === 'abandoned' })
  // Polled by Activity's job watcher; this only reads it.
  const jobs = useJobs()
  const running = jobs.data?.current?.status === 'running' ? jobs.data.current : null

  const counts: Record<ListKey, number | undefined> = {
    later: later.data?.length,
    gems: gems.data?.gems.length,
    wishlist: wish.data?.enabled ? wish.data.suggested.length + wish.data.wishlist.length : undefined,
    staples: staples.data?.total,
    done: stats.data?.by_status?.done,
    shelved: stats.data?.by_status?.shelved,
    abandoned: stats.data?.by_status?.abandoned,
    still_playing: stats.data?.by_status?.still_playing,
    burnt_out: stats.data?.by_status?.burnt_out,
    hidden: stats.data?.by_status?.hidden,
    hidden_by_me: hidden.data?.length,
  }

  const games = useMemo(() => {
    if (list === 'later') {
      const l = [...(later.data ?? [])]
      if (sort === 'shortest') l.sort((a, b) => (a.hltb_main ?? Infinity) - (b.hltb_main ?? Infinity) || a.name.localeCompare(b.name))
      else l.sort((a, b) => (b.saved_at ?? 0) - (a.saved_at ?? 0))
      return l
    }
    if (list === 'staples') return staples.data?.items ?? []
    if (list === 'gems' || list === 'wishlist' || list === 'hidden_by_me') return []
    // Abandoned games waiting for a second chance are listed above the rest.
    const waiting = new Set(list === 'abandoned' ? second.data?.games.map((e) => e.game.id) : [])
    const l = (status.data?.items ?? []).filter((g) => !waiting.has(g.id))
    l.sort((a, b) => recency(b) - recency(a) || a.name.localeCompare(b.name))
    return l
  }, [list, later.data, status.data, staples.data, second.data, sort])

  const q =
    list === 'later'
      ? later
      : list === 'gems'
        ? gems
        : list === 'wishlist'
          ? wish
          : list === 'staples'
            ? staples
            : list === 'hidden_by_me'
              ? hidden
              : status
  const n = games.length
  // Shelved or abandoned by UpNext rather than by you.
  const inferred = byStatus ? games.filter((g) => g.override == null).length : 0

  // Resolves once the lists on screen have been fetched again.
  const refresh = useCallback(
    () =>
      Promise.all(
        [PLAY_LATER_KEY[0], 'games', 'stats', 'collections', 'history', 'game', 'second-chances'].map((key) =>
          qc.invalidateQueries({ queryKey: [key] }),
        ),
      ),
    [qc],
  )

  const onUpdated = useCallback(
    (g: GameView) => {
      setSheet((cur) => (cur && cur.id === g.id ? g : cur))
      refresh()
    },
    [refresh],
  )

  // The row stays busy until the refreshed list is on screen, so its buttons
  // can't be pressed again in the moment between the answer and the refresh.
  const withBusy = async (id: number, f: () => Promise<void>) => {
    setBusy((s) => new Set(s).add(id))
    try {
      await f()
    } finally {
      await refresh()
      setBusy((s) => {
        const next = new Set(s)
        next.delete(id)
        return next
      })
    }
  }

  const removeLater = (g: GameView) =>
    withBusy(g.id, async () => {
      try {
        await api.removeFromLater(g.id)
        qc.setQueryData<GameView[]>(PLAY_LATER_KEY, (cur) => cur?.filter((x) => x.id !== g.id))
        toast.success(`Removed ${g.name}`, undefined, {
          label: 'Undo',
          onClick: () =>
            void api
              .saveForLater(g.id)
              .then(refresh)
              .catch((e) => toast.error(`Couldn't undo ${g.name}`, e instanceof Error ? e.message : undefined)),
        })
      } catch (e) {
        toast.error(`Couldn't remove ${g.name}`, e instanceof Error ? e.message : undefined)
      }
    })

  const setStatus = (g: GameView, next: OverrideStatus | null, reasons?: Reasons) =>
    withBusy(g.id, async () => {
      const prev = g.override
      try {
        await api.setOverride(g.id, next, reasons)
        const word = overrideMeta(next)?.label
        toast.success(next ? `${g.name}: ${word}` : `${g.name}: status cleared`, undefined, {
          label: 'Undo',
          onClick: () =>
            void api
              .setOverride(g.id, prev)
              .then(refresh)
              .catch((e) => toast.error(`Couldn't undo ${g.name}`, e instanceof Error ? e.message : undefined)),
        })
        offerNext(g.id, next)
      } catch (e) {
        toast.error(`Couldn't update ${g.name}`, e instanceof Error ? e.message : undefined)
      }
    })

  const answer = (g: GameView, a: SecondChanceAnswer) =>
    withBusy(g.id, async () => {
      try {
        await api.secondChance(g.id, { answer: a })
        toast.success(`${g.name}: ${SECOND_CHANCE_DONE[a]}`, undefined, {
          label: 'Undo',
          onClick: () =>
            void api
              .secondChance(g.id, { undo: true })
              .then(refresh)
              .catch((e) => toast.error(`Couldn't undo ${g.name}`, e instanceof Error ? e.message : undefined)),
        })
      } catch (e) {
        toast.error(`Couldn't answer for ${g.name}`, e instanceof Error ? e.message : undefined)
      }
    })

  const saveReasons = async (g: GameView, codes: string[], note: string | null) => {
    let ok = false
    await withBusy(g.id, async () => {
      try {
        await api.secondChance(g.id, { reasons: { codes, note } })
        ok = true
      } catch (e) {
        toast.error(`Couldn't save that for ${g.name}`, e instanceof Error ? e.message : undefined)
      }
    })
    return ok
  }

  const checkNow = async () => {
    try {
      const job = await api.runTask('second_chances')
      qc.setQueryData<JobsResponse>(keys.jobs, (old) => ({ current: job, recent: old?.recent ?? [] }))
      toast.info('Checking second chances', 'Reading recent reviews first; Activity shows how far it is.')
    } catch (e) {
      toast.error("Couldn't start the check", e instanceof Error ? e.message : undefined)
    }
  }

  const findGems = async () => {
    try {
      const job = await api.runTask('hidden_gems')
      qc.setQueryData<JobsResponse>(keys.jobs, (old) => ({ current: job, recent: old?.recent ?? [] }))
      toast.info('Finding hidden gems', 'The AI rates the games not rated yet; Activity shows how far it is.')
    } catch (e) {
      toast.error("Couldn't start finding hidden gems", e instanceof Error ? e.message : undefined)
    }
  }

  const updateWishlist = async () => {
    try {
      const job = await api.runTask('wishlist')
      qc.setQueryData<JobsResponse>(keys.jobs, (old) => ({ current: job, recent: old?.recent ?? [] }))
      toast.info('Updating the wishlist', 'Reading your Steam wishlist and your series; Activity shows how far it is.')
    } catch (e) {
      toast.error("Couldn't start the update", e instanceof Error ? e.message : undefined)
    }
  }

  const wishBusy = useMemo(() => new Set([...busy].filter((id) => id < 0).map((id) => -id)), [busy])

  const answerWish = (e: WishlistEntry, a: WishAnswer) =>
    // Wishlist rows aren't games, so their busy marks are negative.
    withBusy(-e.id, async () => {
      try {
        qc.setQueryData(['wishlist'], await api.wishlistSet(e.id, a))
        const prev = e.state
        toast.success(`${e.name}: ${WISH_DONE[a]}`, undefined, {
          label: 'Undo',
          onClick: () =>
            void api
              .wishlistSet(e.id, prev === 'in_library' ? 'suggested' : prev)
              .then((d) => qc.setQueryData(['wishlist'], d))
              .catch((err) => toast.error(`Couldn't undo ${e.name}`, err instanceof Error ? err.message : undefined)),
        })
      } catch (err) {
        toast.error(`Couldn't update ${e.name}`, err instanceof Error ? err.message : undefined)
      }
    })

  const addWish = async (name: string) => {
    try {
      const before = new Set(wish.data?.wishlist.map((e) => e.id))
      const d = await api.wishlistAdd({ name })
      qc.setQueryData<WishlistResponse>(['wishlist'], d)
      const added = d.wishlist.find((e) => !before.has(e.id))
      toast.success(added ? `${added.name} is on your wishlist` : 'It was on your wishlist already')
      return true
    } catch (e) {
      toast.error(`Couldn't add ${name}`, e instanceof ApiError || e instanceof Error ? e.message : undefined)
      return false
    }
  }

  const choose = (k: ListKey) => {
    const next = new URLSearchParams(params)
    if (k === 'later') next.delete('list')
    else next.set('list', k)
    next.delete('sort')
    setParams(next)
  }

  const setSort = (v: LaterSort) => {
    const next = new URLSearchParams(params)
    if (v === 'recent') next.delete('sort')
    else next.set('sort', v)
    setParams(next, { replace: true })
  }

  const TABS: { key: ListKey; label: string; dot?: string }[] = [
    { key: 'later', label: 'Play later' },
    { key: 'gems', label: 'Hidden gems' },
    ...(wishOn ? [{ key: 'wishlist' as ListKey, label: 'Wishlist' }] : []),
    { key: 'staples', label: 'Staples' },
    ...OVERRIDES.map((o) => ({ key: o.value as ListKey, label: o.label, dot: o.dot })),
    ...(hiddenTab ? [{ key: 'hidden_by_me' as ListKey, label: 'Hidden by me' }] : []),
  ]
  const checking = running?.task === 'second_chances'
  const task = second.data?.task
  const ai = second.data?.ai
  // How many abandoned games are waiting for an answer, shown on this tab.
  const waiting = list === 'abandoned' ? (second.data?.games.length ?? 0) : 0

  return (
    <div className="px-4 pb-16 sm:px-10">
      <section className="flex flex-wrap items-center justify-between gap-4 pt-8 pb-4">
        <div role="group" aria-label="Lists" data-tour="lists" className="seg flex-wrap">
          {TABS.map((t) => (
            <button
              key={t.key}
              type="button"
              aria-pressed={list === t.key}
              onClick={() => choose(t.key)}
              className={cx('seg-item', list === t.key && 'seg-item-on')}
            >
              {t.dot && <span aria-hidden="true" className={cx('h-[7px] w-[7px] rounded-full', t.dot)} />}
              {t.label}
              {counts[t.key] != null && <span className="font-mono text-[10.5px] text-dim">{counts[t.key]}</span>}
            </button>
          ))}
        </div>
      </section>

      {waiting > 0 && (
        <div className="flex max-w-[900px] flex-col gap-2 pb-2 text-[15.5px] leading-[1.6] text-muted">
          <h2 className="m-0 font-display text-[22px] font-bold text-ink">Second chance</h2>
          <p className="m-0 text-[17px]">
            <span className="text-ink">
              {waiting} abandoned {waiting === 1 ? 'game' : 'games'}
            </span>{' '}
            you might give another go: you haven’t answered for {waiting === 1 ? 'it' : 'them'} since you last played. For each, UpNext
            reads the player reviews written since you stopped and checks them against why you stopped; the ones where things changed come
            first.
          </p>
          <p className="m-0">
            Willing to try again puts it back on your shelf, and turning it down once in picks ends it for good. Still no keeps it out of
            picks for good. Not for me means it’s never recommended again.
          </p>
          {ai && !ai.available ? (
            <p className="m-0">
              No AI is set up, so UpNext can’t read the reviews for you; your reasons and answers still work.{' '}
              <Link to="/settings#picks" className="text-ink underline decoration-underline underline-offset-4">
                Set one up
              </Link>
            </p>
          ) : (
            <p className="m-0 flex flex-wrap items-center gap-x-3 gap-y-1">
              <span>
                {checking
                  ? 'Checking now…'
                  : <>{task?.last_run ? `Last checked ${dayText(task.last_run.started_at)}` : 'Not checked yet'}{
                      task?.enabled && task.next_run_at && task.schedule !== 'off'
                        ? ` · next ${dayText(task.next_run_at)}`
                        : <>. It checks when you ask, or on a schedule you set in <Place section="Jobs" />.</>
                    }</>}
              </span>
              <button
                type="button"
                onClick={() => void checkNow()}
                disabled={!!running}
                className="btn-quiet text-ink underline decoration-underline underline-offset-4 disabled:opacity-50"
              >
                Check now
              </button>
            </p>
          )}
        </div>
      )}

      {list === 'gems' && gems.data && (
        <HiddenGems data={gems.data} running={running} onRun={() => void findGems()} onOpen={setSheet} />
      )}

      {list === 'wishlist' && wish.data?.enabled && (
        <Wishlist
          data={wish.data}
          running={running}
          busy={wishBusy}
          onRun={() => void updateWishlist()}
          onAnswer={(e, a) => void answerWish(e, a)}
          onAdd={addWish}
        />
      )}

      {list === 'hidden_by_me' && <HiddenByMe hidden={hidden} onOpen={setSheet} />}

      {waiting > 0 && second.data && ai && (
        <ul aria-label="Second chance" className="max-w-[1100px] pb-10">
          {second.data.games.map((e) => (
            <SecondChanceRow
              key={e.game.id}
              entry={e}
              reasons={reasons.data}
              ai={ai}
              busy={busy.has(e.game.id)}
              onOpen={setSheet}
              onAnswer={(g, a) => void answer(g, a)}
              onReasons={saveReasons}
            />
          ))}
        </ul>
      )}
      {waiting > 0 && n > 0 && <h2 className="m-0 pb-2 font-display text-[22px] font-bold">The rest</h2>}

      {n > 0 && (
        <p className="m-0 pb-2 text-[17px] leading-[1.6] text-muted">
          <span className="text-ink">
            {waiting > 0 && 'The other '}
            {n} {n === 1 ? 'game' : 'games'}
          </span>{' '}
          {PHRASE[list]}
          {list === 'later' ? (
            <>
              , <SentenceChoice label="Sort" value={sort} options={LATER_SORT} onChange={setSort} />
            </>
          ) : list === 'staples' ? (
            ', most played first'
          ) : (
            ', most recent first'
          )}
          {list !== 'later' && '.'}
          {inferred > 0 && ` UpNext put ${inferred === n ? (n === 1 ? 'it' : 'them') : `${inferred} of them`} here from when you last played.`}
          {AFTER[list]}
        </p>
      )}

      {q.isLoading && (
        <div aria-busy="true" aria-label="Loading list" className="pt-4">
          {[0, 1, 2, 3].map((i) => (
            <div key={i} className="flex items-center gap-6 border-b border-line py-4">
              <div className="shimmer aspect-[2/3] w-16 rounded-[8px]" />
              <div className="flex-1">
                <div className="shimmer h-5 w-64 max-w-full rounded-md" />
                <div className="shimmer mt-2.5 h-3 w-40 rounded" />
              </div>
            </div>
          ))}
        </div>
      )}

      {q.error && !q.data && !(list === 'hidden_by_me' && hiddenLocked(hidden)) && (
        <div className="py-10" role="alert">
          <p className="text-[15px] text-ink">Couldn't load this list.</p>
          <p className="mono mt-1">{(q.error as Error).message}</p>
          <button type="button" className="btn-outline mt-4 h-11" onClick={() => q.refetch()} disabled={q.isFetching}>
            {q.isFetching ? 'Trying again…' : 'Try again'}
          </button>
        </div>
      )}

      {q.data && n === 0 && waiting === 0 && list !== 'gems' && list !== 'wishlist' && list !== 'hidden_by_me' && (
        <div className="py-10">
          <p className="text-[15.5px] text-muted">{EMPTY[list]}</p>
          <Link to={list === 'later' ? '/' : '/library'} className="btn-outline mt-4 h-11">
            {list === 'later' ? 'Go to Pick' : 'Browse the library'}
          </Link>
        </div>
      )}

      {n > 0 && (
        <ul aria-label={TABS.find((t) => t.key === list)?.label} className="max-w-[1100px]">
          {games.map((g) =>
            list === 'later' ? (
              <LaterRow key={g.id} game={g} onOpen={setSheet} onRemove={removeLater} removing={busy.has(g.id)} />
            ) : (
              <LaterRow
                key={g.id}
                game={g}
                onOpen={setSheet}
                meta={[
                  list === 'staples' ? `${minutesToText(g.playtime_minutes)} played` : hoursToText(g.hltb_main),
                  list !== 'staples' && g.playtime_known && g.playtime_minutes > 0 ? `played ${hoursToText(g.playtime_minutes / 60)}` : null,
                  list !== 'staples' && g.override_at
                    ? `marked ${dayText(g.override_at)}`
                    : g.last_played
                      ? `last played ${dayText(g.last_played)}`
                      : null,
                ]
                  .filter(Boolean)
                  .join(' · ')}
                action={<StatusMenu game={g} size="sm" busy={busy.has(g.id)} onChange={(s, r) => void setStatus(g, s, r)} hide={g} />}
              />
            ),
          )}
        </ul>
      )}

      <GameSheet game={sheet} onClose={() => setSheet(null)} onUpdated={onUpdated} />
    </div>
  )
}
