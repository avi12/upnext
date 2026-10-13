import { useState, type ReactNode } from 'react'
import { Link } from 'react-router'
import { useQuery } from '@tanstack/react-query'
import { api } from '../../api'
import type { TasteSourceId, TasteSources } from '../../types'
import { plural, relativeTime } from '../../lib/format'
import { dayText } from '../../lib/statuses'
import { STAR_WORDS } from '../../lib/ratings'
import { Section, TextButton } from '../settings/parts'
import { andList, diaryText, games, reactionText } from './words'
import { Place } from '../Place'
import { possessive, usePlayers } from '../../lib/players'

const STORE: Record<string, string> = { steam: 'Steam', gog: 'GOG', epic: 'Epic', amazon: 'Amazon' }

const STATUS_WORD: Record<string, string> = {
  done: 'beaten',
  shelved: 'shelved',
  abandoned: 'abandoned',
  still_playing: 'still playing',
  burnt_out: 'burnt out',
  hidden: 'not for you',
}

/** What deleting each source does, said before it's done. */
const DELETE_TEXT: Partial<Record<TasteSourceId, string>> = {
  statuses:
    'This clears the status on every game you marked. Beaten games count as played again, and games marked Not for me can be picked again.',
  reactions:
    'This forgets what you did with every pick and how you rated it, why you stopped each game, and your Second Chance answers. Games you turned down can come back to picks. History keeps its lineups, without what you did with them.',
  answers:
    'This deletes your favourites, likes, dislikes, hard nos, habits and routines, and brings back the patterns you said weren’t right.',
  diary: 'This deletes every hour and launch the play diary recorded. It stays on or off as it is.',
  ratings: 'This clears your rating on every game you rated. How you rated picks stays, with your reactions.',
  wishlist:
    'This deletes the wishlist, the suggestions and your answers to them, so a game you said no to can be suggested again. While the wishlist is on, your Steam wishlist comes back at its next update.',
}

function count(n: number | undefined, one: string, many: string): string | null {
  return n ? `${n.toLocaleString()} ${n === 1 ? one : many}` : null
}

/** "Where it comes from": each source, what it holds, and See it and Delete it. */
export function SourcesSection({
  data,
  onDelete,
  onSeeAnswers,
  onDiary,
}: {
  data: TasteSources | undefined
  onDelete: (source: TasteSourceId) => Promise<void>
  onSeeAnswers: () => void
  /** Turn the play diary on or off. */
  onDiary: (on: boolean) => Promise<void>
}) {
  const [asking, setAsking] = useState<TasteSourceId | null>(null)
  const [busy, setBusy] = useState(false)
  const [seeing, setSeeing] = useState(false)
  const [seeingDiary, setSeeingDiary] = useState(false)
  const [seeingRatings, setSeeingRatings] = useState(false)
  const [switching, setSwitching] = useState(false)
  // With several players, which accounts this player's hours come from.
  const { multi } = usePlayers()
  const accounts = useQuery({ queryKey: ['accounts'], queryFn: api.accounts, enabled: multi })

  if (!data) return null
  const hoursFrom = (accounts.data?.accounts ?? [])
    .filter((a) => a.mine || (a.uses && a.plays_on))
    .map((a) => (a.mine ? `your ${a.name}` : `${possessive(a.owner)} ${a.name}`))
  const hoursText = !multi || !accounts.data
    ? ''
    : hoursFrom.length
      ? ` Your hours come from ${andList(hoursFrom)}; the other games are shared with you, without their hours.`
      : ' None of your accounts has hours yet: link your own under Accounts, or ask an owner to tick that you play on theirs.'
  const { library, statuses, reactions, answers, diary, wishlist, ratings } = data
  const good = reactions.pick_ratings?.good ?? 0
  const ratedPicks = good + (reactions.pick_ratings?.not_a_fit ?? 0)
  const picks = Object.values(reactions.picks).reduce((a, b) => a + (b ?? 0), 0) + ratedPicks
  const stated = answers.favourites + answers.likes + answers.dislikes + answers.hard_nos + answers.habits + answers.routines

  const del = async (source: TasteSourceId) => {
    setBusy(true)
    try {
      await onDelete(source)
      setAsking(null)
    } finally {
      setBusy(false)
    }
  }

  const row = (
    id: TasteSourceId,
    name: string,
    what: ReactNode,
    see: ReactNode,
    deletable: boolean,
    empty: boolean,
    extra?: ReactNode,
  ) => (
    <li className="border-b border-line py-4">
      <div className="flex flex-wrap items-start gap-x-6 gap-y-1">
        <div className="min-w-0 flex-1 basis-72">
          <p className="m-0 text-[15.5px] font-bold">{name}</p>
          <p className="m-0 mt-0.5 text-[14.5px] leading-[1.55] text-muted">{what}</p>
        </div>
        <div className="flex items-center gap-x-4">
          {see}
          {deletable && !empty && (
            <TextButton onClick={() => setAsking(asking === id ? null : id)} aria-expanded={asking === id}>
              Delete it
            </TextButton>
          )}
        </div>
      </div>
      {asking === id && (
        <div className="mt-2 flex max-w-[760px] flex-col gap-3 rounded-[10px] border border-line-strong p-4">
          <p className="m-0 text-[14.5px] leading-[1.55]">{DELETE_TEXT[id]} It can’t be undone.</p>
          <div className="flex gap-3">
            <button type="button" className="btn-outline h-10 border-bad text-bad" disabled={busy} onClick={() => void del(id)}>
              {busy ? 'Deleting…' : 'Delete it'}
            </button>
            <button type="button" className="btn-quiet h-10" onClick={() => setAsking(null)}>
              Keep it
            </button>
          </div>
        </div>
      )}
      {extra}
    </li>
  )

  const statusText = Object.entries(statuses.by)
    .sort((a, b) => (b[1] ?? 0) - (a[1] ?? 0))
    .map(([s, n]) => `${n} ${STATUS_WORD[s] ?? s}`)
  const pickText = [
    count(reactions.picks.chosen, 'played', 'played'),
    count(reactions.picks.not_tonight, 'not tonight', 'not tonight'),
    count(reactions.picks.too, '“It’s too ___”', '“It’s too ___”'),
    count(reactions.picks.not_for_me, 'never recommend this', 'never recommend this'),
    count(reactions.picks.started, '“I’ve started it”', '“I’ve started it”'),
  ].filter(Boolean) as string[]
  // The hit rate: how many of the picks you rated fit what you asked for.
  const hitText = ratedPicks
    ? `Of the ${ratedPicks.toLocaleString()} ${ratedPicks === 1 ? 'pick' : 'picks'} you rated, ${good.toLocaleString()} ${good === 1 ? 'was a good pick' : 'were good picks'}. `
    : ''
  const starText = [5, 4, 3, 2, 1]
    .filter((n) => ratings.by[String(n)])
    .map((n) => `${ratings.by[String(n)]} “${STAR_WORDS[n - 1]}”`)
  const answerText = [
    count(answers.favourites, 'favourite', 'favourites'),
    count(answers.likes, 'like', 'likes'),
    count(answers.dislikes, 'dislike', 'dislikes'),
    count(answers.hard_nos, 'hard no', 'hard nos'),
    count(answers.habits, 'habit', 'habits'),
    count(answers.routines, 'routine', 'routines'),
    count(answers.not_really, 'pattern you said wasn’t right', 'patterns you said weren’t right'),
  ].filter(Boolean) as string[]

  const wishText = [
    count(wishlist.steam, 'game from your Steam wishlist', 'games from your Steam wishlist'),
    count(wishlist.added, 'game you added', 'games you added'),
    count(wishlist.suggested, 'suggestion waiting', 'suggestions waiting'),
    count(wishlist.answered, 'suggestion you said no to or own elsewhere', 'suggestions you said no to or own elsewhere'),
    count(wishlist.bought, 'game you got after wishlisting it', 'games you got after wishlisting them'),
  ].filter(Boolean) as string[]
  const wishTotal = wishlist.steam + wishlist.added + wishlist.suggested + wishlist.answered + wishlist.bought

  return (
    <Section id="sources" title="Where it comes from" desc="Every source, and how much it holds">
      <ul className="m-0 list-none p-0 xl:grid xl:grid-cols-2 xl:gap-x-14">
        {row(
          'library',
          'Your library and playtime',
          <>
            {games(library.games)} from {andList(library.stores.map((s) => STORE[s] ?? s))}, {library.played.toLocaleString()} of them with
            playtime ({plural(library.hours, 'hour')}). Read from your stores at each sync
            {library.last_sync ? ` (the last one ${relativeTime(library.last_sync)})` : ''}.{hoursText} It comes back at the next sync, so it
            isn’t deleted here: disconnect a store under <Place section="Accounts" />.
          </>,
          <Link to="/library?status=everything&sort=playtime&order=desc" className="text-sm text-muted underline decoration-underline underline-offset-4 hover:text-ink">
            See it
          </Link>,
          false,
          false,
        )}
        {row(
          'statuses',
          'Statuses you set',
          statuses.games ? (
            <>
              {games(statuses.games)}: {andList(statusText)}. From the status menus on the game page, the Lists tab and the pick card.
            </>
          ) : (
            'None yet. Beat it, Shelved, Not for me and the rest come from the status menus.'
          ),
          <Link to="/lists?list=done" className="text-sm text-muted underline decoration-underline underline-offset-4 hover:text-ink">
            See it
          </Link>,
          true,
          statuses.games === 0,
        )}
        {row(
          'reactions',
          'Your reactions',
          picks + reactions.stop_reasons + reactions.second_chance_answers ? (
            <>
              {pickText.length ? `On pick cards: ${andList(pickText)}. ` : ''}
              {hitText}
              {reactions.stop_reasons ? `Why you stopped, for ${games(reactions.stop_reasons)}. ` : ''}
              {reactions.second_chance_answers ? `${count(reactions.second_chance_answers, 'Second Chance answer', 'Second Chance answers')}.` : ''}
            </>
          ) : (
            'None yet. What you do on pick cards and how you rate picks, why you stopped a game, and Second Chance answers land here.'
          ),
          <TextButton onClick={() => setSeeing((v) => !v)} aria-expanded={seeing} disabled={!(picks + reactions.stop_reasons + reactions.second_chance_answers)}>
            {seeing ? 'Hide' : 'See it'}
          </TextButton>,
          true,
          picks + reactions.stop_reasons + reactions.second_chance_answers === 0,
          seeing ? <ReactionList /> : null,
        )}
        {row(
          'ratings',
          'Your ratings of games',
          ratings.games ? (
            <>
              {games(ratings.games)}: {andList(starText)}. From the stars on each game’s page; picks lean toward games like the ones you rated 4 or 5.
            </>
          ) : (
            'None yet. Rate a game you’ve played from 1 to 5 stars on its page.'
          ),
          <TextButton onClick={() => setSeeingRatings((v) => !v)} aria-expanded={seeingRatings} disabled={!ratings.games}>
            {seeingRatings ? 'Hide' : 'See it'}
          </TextButton>,
          true,
          ratings.games === 0,
          seeingRatings ? <RatingList /> : null,
        )}
        {row(
          'answers',
          'What you told it',
          stated + answers.not_really ? (
            <>{andList(answerText)}, below. Stated things never fade.</>
          ) : (
            'Nothing yet. Your favourites, likes, dislikes, hard nos, habits and routines go here, below.'
          ),
          <TextButton onClick={onSeeAnswers}>See it</TextButton>,
          true,
          stated + answers.not_really === 0,
        )}
        {row(
          'diary',
          'The play diary',
          diaryText(diary),
          <>
            <TextButton
              disabled={switching}
              onClick={() => {
                setSwitching(true)
                void onDiary(!diary.on).finally(() => setSwitching(false))
              }}
            >
              {switching ? (diary.on ? 'Turning off…' : 'Turning on…') : diary.on ? 'Turn it off' : 'Turn it on'}
            </TextButton>
            <TextButton onClick={() => setSeeingDiary((v) => !v)} aria-expanded={seeingDiary} disabled={!(diary.rows + diary.launches)}>
              {seeingDiary ? 'Hide' : 'See it'}
            </TextButton>
          </>,
          true,
          diary.rows + diary.launches === 0,
          seeingDiary ? <DiaryList /> : null,
        )}
        {row(
          'wishlist',
          'The wishlist',
          <>
            {wishTotal ? `${andList(wishText)}. ` : 'Nothing yet. '}
            {wishlist.on
              ? 'Suggestions come from your series; your Steam wishlist is read, never changed.'
              : <>It’s off (<Place section="Picks" />), so nothing new is suggested or read.</>}
          </>,
          wishlist.on ? (
            <Link to="/lists?list=wishlist" className="text-sm text-muted underline decoration-underline underline-offset-4 hover:text-ink">
              See it
            </Link>
          ) : null,
          true,
          wishTotal === 0,
        )}
      </ul>
    </Section>
  )
}

/** The play diary's rows, newest first. */
function DiaryList() {
  const rows = useQuery({ queryKey: ['taste-sources', 'diary'], queryFn: api.tasteDiaryRows })
  if (rows.isLoading) return <p className="m-0 mt-3 text-sm text-dim">Loading…</p>
  if (rows.error) return <p className="m-0 mt-3 text-sm text-bad">Couldn’t load it: {(rows.error as Error).message}</p>
  const time = (t: number) => new Date(t * 1000).toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' })
  const day = (t: number) => new Date(t * 1000).toLocaleDateString(undefined, { weekday: 'short', month: 'short', day: 'numeric' })
  return (
    <ul className="m-0 mt-3 max-h-[420px] list-none overflow-y-auto p-0 text-[14px]">
      {(rows.data ?? []).map((r) => (
        <li key={r.id} className="flex flex-wrap gap-x-3 border-t border-line py-2">
          <span className="w-28 shrink-0 text-dim tabular-nums">{day(r.hour)}</span>
          <span className="w-20 shrink-0 text-dim tabular-nums">{time(r.source === 'launch' ? r.at : r.hour)}</span>
          <span className="min-w-0 flex-1">
            <span className="text-ink">{r.name}</span>
            <span className="text-muted">
              {' · '}
              {r.source === 'launch' ? 'started from UpNext' : `${r.minutes} ${r.minutes === 1 ? 'minute' : 'minutes'} in that hour`}
            </span>
          </span>
        </li>
      ))}
    </ul>
  )
}

/** Your ratings of games, newest first. */
function RatingList() {
  const rows = useQuery({ queryKey: ['taste-sources', 'ratings'], queryFn: api.tasteGameRatings })
  if (rows.isLoading) return <p className="m-0 mt-3 text-sm text-dim">Loading…</p>
  if (rows.error) return <p className="m-0 mt-3 text-sm text-bad">Couldn’t load them: {(rows.error as Error).message}</p>
  return (
    <ul className="m-0 mt-3 max-h-[420px] list-none overflow-y-auto p-0 text-[14px]">
      {(rows.data ?? []).map((r) => (
        <li key={r.title_id} className="flex flex-wrap gap-x-3 border-t border-line py-2">
          <span className="w-24 shrink-0 text-dim tabular-nums">{dayText(r.at)}</span>
          <span className="min-w-0 flex-1">
            <span className="text-ink">{r.name}</span>
            <span className="text-muted">
              {' · '}
              {r.stars}/5, {STAR_WORDS[r.stars - 1]?.toLowerCase()}
            </span>
          </span>
        </li>
      ))}
    </ul>
  )
}

/** Every reaction, newest first. */
function ReactionList() {
  const rows = useQuery({ queryKey: ['taste-sources', 'reactions'], queryFn: api.tasteReactions })
  const reasons = useQuery({ queryKey: ['stop-reasons'], queryFn: api.stopReasons, staleTime: Infinity })
  if (rows.isLoading) return <p className="m-0 mt-3 text-sm text-dim">Loading…</p>
  if (rows.error) return <p className="m-0 mt-3 text-sm text-bad">Couldn’t load them: {(rows.error as Error).message}</p>
  const label = (code: string) => reasons.data?.find((r) => r.code === code)?.label.toLowerCase() ?? code
  const detail = (text: string | null) => {
    try {
      const codes = JSON.parse(text ?? '[]') as string[]
      return codes.length ? andList(codes.map(label)) : 'cleared'
    } catch {
      return text ?? ''
    }
  }
  return (
    <ul className="m-0 mt-3 max-h-[420px] list-none overflow-y-auto p-0 text-[14px]">
      {(rows.data ?? []).map((r, i) => (
        <li key={i} className="flex flex-wrap gap-x-3 border-t border-line py-2">
          <span className="w-24 shrink-0 text-dim tabular-nums">{dayText(r.at)}</span>
          <span className="min-w-0 flex-1">
            <span className="text-ink">{r.name}</span>
            <span className="text-muted">
              {' · '}
              {r.what === 'reasons' ? `why you stopped: ${detail(r.text)}${r.note ? `, “${r.note}”` : ''}` : reactionText(r.what, r.text)}
            </span>
          </span>
        </li>
      ))}
    </ul>
  )
}
