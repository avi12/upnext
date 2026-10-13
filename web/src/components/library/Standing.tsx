import { useQuery } from '@tanstack/react-query'
import { Link } from 'react-router'
import { api } from '../../api'
import type { GameView } from '../../types'
import { cx, hoursToText, minutesToText } from '../../lib/format'
import { dayText } from '../../lib/statuses'
import { STATUS_DOT, STATUS_WORD } from './status'
import { useSetEnding } from './sheetActions'
import { MyRating } from './MyRating'
import { offersRating } from '../../lib/ratings'
import { useSettings } from '../../lib/queries'

/** Where a started game stands, under the sheet's actions: your rating, the
 *  status UpNext worked out when you haven't set one, whether it's one of your
 *  staples, why you stopped, and whether it can be played endlessly. */
export function Standing({ game, onUpdated }: { game: GameView; onUpdated: (g: GameView) => void }) {
  const settings = useSettings()
  const reasons = useQuery({ queryKey: ['stop-reasons'], queryFn: api.stopReasons, staleTime: Infinity, enabled: !!game.stop_reasons })
  const ending = useSetEnding(onUpdated)
  const persona = settings.data?.steam_persona
  const played = game.playtime_known && game.playtime_minutes > 0 ? minutesToText(game.playtime_minutes) : null

  const inferred = game.override == null ? inferredText(game, played) : null
  const back = game.status === 'burnt_out' && game.burnt_out_until ? `Back in picks ${dayText(game.burnt_out_until)}.` : null
  const why = game.stop_reasons
  const whyText = why
    ? [
        why.codes.map((c) => reasons.data?.find((r) => r.code === c)?.label ?? c).join(', '),
        why.note ? `“${why.note}”` : null,
      ]
        .filter(Boolean)
        .join('. ')
    : null
  // The Second Chance page's answer, while it stands (it lapses when you play the game again).
  const answer = game.second_chance?.answered_at && game.second_chance.answered_at > game.last_played ? game.second_chance : null
  const stillNo = game.status === 'abandoned' && answer?.answer === 'still_no'
  // Turned down as a second chance: picks leave it alone now.
  const spent = game.status === 'abandoned' && game.rejections > 0 && !game.abandoned_by_picks && !stillNo
  // What reviews since it was put down say about why, from the Second Chance page.
  const since = game.status === 'abandoned' && game.second_chance?.now && !game.second_chance.outdated ? game.second_chance.line : null
  const chosen = game.chosen && ['playing', 'shelved', 'abandoned'].includes(game.status) ? game.chosen : null

  return (
    <div className="flex flex-col gap-1.5 text-[14px] leading-snug text-muted">
      {offersRating(game) && <MyRating game={game} onUpdated={onUpdated} />}
      {inferred && (
        <p className="m-0 flex items-baseline gap-2">
          <span aria-hidden="true" className={cx('inline-block h-[7px] w-[7px] shrink-0 translate-y-[-1px] rounded-full', STATUS_DOT[game.status])} />
          <span>
            <span className="font-semibold text-ink">{STATUS_WORD[game.status]}</span>
            {inferred.byUpNext && ' by UpNext'}: {inferred.text}.
          </span>
        </p>
      )}
      {back && <p className="m-0">{back}</p>}
      {spent && <p className="m-0">Turned down as a second chance in picks, so picks leave it alone now.</p>}
      {stillNo && answer?.answered_at && (
        <p className="m-0">You said still no on the Second Chance page ({dayText(answer.answered_at)}), so picks leave it alone now.</p>
      )}
      {game.second_try && answer?.answered_at && (
        <p className="m-0">
          You said you’d give it another go ({dayText(answer.answered_at)}). Turn it down once in picks and it’s gone for good.
        </p>
      )}
      {since && (
        <p className="m-0">
          Since you stopped: <span className="text-ink">{since}</span>.{' '}
          <Link to="/lists?list=abandoned" className="underline decoration-underline underline-offset-4 hover:text-ink">
            Second chance
          </Link>
        </p>
      )}
      {chosen && (
        <p className="m-0">
          Picked {dayText(chosen.at)} for <span className="text-ink [overflow-wrap:anywhere]">{chosen.prompt ? `“${chosen.prompt}”` : 'a surprise'}</span>.
        </p>
      )}
      {game.from_wishlist && (
        <p className="m-0">
          From your wishlist{game.from_wishlist.source === 'steam' ? ' on Steam' : ''} ({dayText(game.from_wishlist.wishlisted_at)})
          {game.from_wishlist.reason ? (
            <>
              : <span className="text-ink">{game.from_wishlist.reason}</span>
            </>
          ) : (
            '.'
          )}
        </p>
      )}
      {game.too && (
        <p className="m-0">
          You said it's too <span className="text-ink">{game.too.word}</span> ({dayText(game.too.at)}).
        </p>
      )}
      {game.staple && (
        <p className="m-0">
          One of {persona ? `${persona}'s` : 'your'} staples{played ? `, ${played}` : ''}.
        </p>
      )}
      {whyText && (
        <p className="m-0">
          Why you stopped: <span className="text-ink">{whyText}</span>
        </p>
      )}
      <EndingLine game={game} busy={ending.isPending} set={(noEnding) => ending.mutate({ id: game.id, noEnding })} />
    </div>
  )
}

/** Whether the game can be played endlessly, said as a statement with a way
 *  to correct it. A game UpNext takes to have an ending says nothing:
 *  "Mark as…" offers "Can be played endlessly". So a game with no tags yet
 *  shows no guess it couldn't have made. */
function EndingLine({ game, busy, set }: { game: GameView; busy: boolean; set: (noEnding: boolean | null) => void }) {
  const endless = game.kind === 'endless'
  const link = 'btn-quiet min-h-10 text-[13px] text-dim underline decoration-underline underline-offset-4 hover:text-ink'
  if (game.no_ending == null && !endless) return null
  return (
    <p className="m-0 flex flex-wrap items-center gap-x-3">
      <span>
        <span className="text-ink">{endless ? 'This game can be played endlessly.' : 'This game has an ending.'}</span>{' '}
        <span className="text-[13px] text-dim">{game.no_ending == null ? 'UpNext’s guess, from its tags.' : 'You said so.'}</span>
      </span>
      {game.no_ending == null ? (
        <button type="button" disabled={busy} onClick={() => set(false)} className={link}>
          Wrong?
        </button>
      ) : (
        <button type="button" disabled={busy} onClick={() => set(null)} className={link}>
          Let UpNext guess
        </button>
      )}
    </p>
  )
}

/** What UpNext went on: "last played Jul 3, 14h of a 21h story". */
function inferredText(g: GameView, played: string | null): { byUpNext: boolean; text: string } | null {
  const last = g.last_played ? `last played ${dayText(g.last_played)}` : null
  const story = hoursToText(g.hltb_main)
  switch (g.status) {
    case 'playing':
      if (!g.playtime_known && g.chosen) return { byUpNext: false, text: `you chose it from picks ${dayText(g.chosen.at)}, and this store doesn't report playtime` }
      return last ? { byUpNext: false, text: last } : null
    case 'shelved':
    case 'abandoned': {
      const time = played && story ? `${played} of a ${story} story` : played
      const why = g.abandoned_by_picks
        ? g.second_chance?.answer === 'willing'
          ? 'turned down in picks after you gave it another go'
          : 'turned down twice in picks'
        : null
      return { byUpNext: true, text: [why, last ?? 'no store says when you last played', time].filter(Boolean).join(', ') }
    }
    default:
      return null
  }
}
