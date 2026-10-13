import { Link } from 'react-router'
import type { Gem, GemsResponse, Job } from '../../types'
import { Cover } from '../Cover'
import { PlayLink, TrailerLink } from './GameActions'
import { compactNumber, hoursToText, plural } from '../../lib/format'
import { dayText } from '../../lib/statuses'
import { Place } from '../Place'

/** What players think of it: "97% of 20k reviews", or GOG's stars. */
function ratingText(g: Gem['game']): string | null {
  if (g.review_pct != null && g.review_total) return `${g.review_pct}% of ${compactNumber(g.review_total)} ${g.review_total === 1 ? 'review' : 'reviews'}`
  if (g.gog_rating != null && g.gog_rating_count) return `${g.gog_rating.toFixed(1)} of 5 from ${compactNumber(g.gog_rating_count)} on GOG`
  return null
}

function GemRow({ gem, onOpen }: { gem: Gem; onOpen: (g: Gem['game']) => void }) {
  const g = gem.game
  const r = gem.rating
  const meta = [
    hoursToText(g.hltb_main),
    ratingText(g),
    r ? `studio ${r.studio}/3 · mechanics ${r.mechanics}/3 · play ${r.play}/3` : 'not rated yet',
  ]
    .filter(Boolean)
    .join(' · ')
  return (
    <li className="flex flex-wrap items-center gap-x-6 gap-y-3 border-b border-line py-4">
      <button type="button" onClick={() => onOpen(g)} className="shrink-0 rounded-[8px]" aria-label={`Open ${g.name}`}>
        <Cover game={g} className="w-16 rounded-[8px]" titleSize="text-[10px]" />
      </button>
      <div className="flex min-w-0 flex-1 basis-56 flex-col gap-1.5">
        <button
          type="button"
          onClick={() => onOpen(g)}
          className="self-start text-left font-display text-[19px] leading-[1.2] font-bold tracking-[-0.01em] hover:text-accent"
        >
          {g.name}
        </button>
        {r?.line && <p className="m-0 max-w-2xl text-[15px] leading-[1.5] text-ink">{r.line}</p>}
        <div className="mono">{meta}</div>
      </div>
      <div className="flex items-center gap-x-6 text-[14.5px]">
        <PlayLink game={g} />
        <TrailerLink game={g} />
      </div>
    </li>
  )
}

/** Lists, Hidden gems: the shelf, what's behind it, and the job that rates it. */
export function HiddenGems({
  data,
  running,
  onRun,
  onOpen,
}: {
  data: GemsResponse
  running: Job | null
  onRun: () => void
  onOpen: (g: Gem['game']) => void
}) {
  const { gems, pool, unrated, party, ai, task, rules } = data
  const finding = running?.task === 'hidden_gems'
  const status = finding
    ? 'Rating them now…'
    : <>{task.last_run ? `Last run ${dayText(task.last_run.started_at)}` : 'Not run yet'}{
        task.enabled && task.next_run_at && task.schedule !== 'off'
          ? ` · next ${dayText(task.next_run_at)}`
          : <>. It runs when you ask, or on a schedule you set in <Place section="Jobs" />.</>
      }</>
  return (
    <>
      <div className="flex max-w-[900px] flex-col gap-2 pb-2 text-[15.5px] leading-[1.6] text-muted">
        <p className="m-0 text-[17px]">
          {gems.length > 0 ? (
            <>
              <span className="text-ink">
                {gems.length} hidden {gems.length === 1 ? 'gem' : 'gems'}
              </span>{' '}
              in your library, most unusual first.
            </>
          ) : (
            <span className="text-ink">No hidden gems yet.</span>
          )}{' '}
          They come from {pool} unplayed {pool === 1 ? 'game' : 'games'} that {rules.min_positive}% or more of at least{' '}
          {plural(rules.min_reviews, 'player')} rate well, with a main story of {plural(rules.max_hours, 'hour')} or less. The AI rates each on how little-known
          the studio is, how rare its mechanics are and how odd its play is.
          {party > 0 && ` ${party} party ${party === 1 ? 'game is' : 'games are'} left out.`}
        </p>
        {ai.available ? (
          <p className="m-0 flex flex-wrap items-center gap-x-3 gap-y-1">
            <span>
              {status}
              {!finding && unrated > 0 && ` ${unrated} ${unrated === 1 ? 'game needs' : 'games need'} rating.`}
            </span>
            <button
              type="button"
              onClick={onRun}
              disabled={!!running}
              className="btn-quiet text-ink underline decoration-underline underline-offset-4 disabled:opacity-50"
            >
              Find hidden gems
            </button>
          </p>
        ) : (
          <p className="m-0">
            No AI is set up, so UpNext can’t rate what’s unusual.{' '}
            <Link to="/settings#picks" className="text-ink underline decoration-underline underline-offset-4">
              Set one up
            </Link>
          </p>
        )}
      </div>
      {gems.length > 0 && (
        <ul aria-label="Hidden gems" className="max-w-[1100px]">
          {gems.map((gem) => (
            <GemRow key={gem.game.id} gem={gem} onOpen={onOpen} />
          ))}
        </ul>
      )}
    </>
  )
}
