import { memo, useState } from 'react'
import type { GameView, SeriesEntry } from '../../types'
import { Cover } from '../Cover'
import { cx } from '../../lib/format'
import { hltbShort, isStarted, yearOf, type Series } from './series'
import { hoursMinutes } from '../../lib/gameText'

const NO_GUIDE_NOTE =
  'Oldest first. A guide on where to start appears after the next enrichment with series guides turned on.'

type Tone = 'next' | 'played' | 'quiet'

function stateOf(g: GameView, s: Series): { text: string; tone: Tone } {
  const { c } = s
  const hours = hltbShort(g.hltb_main)
  const withHours = (w: string) => (hours ? `${w} · ${hours}` : w)
  if (g.id === c.next_id && g.status === 'sampled' && g.playtime_minutes > 0) {
    return { text: `Up next · ${hoursMinutes(g.playtime_minutes)} in`, tone: 'next' }
  }
  if (!isStarted(g) && g.status !== 'hidden') {
    if (g.id === c.start_id) return { text: withHours('Start here'), tone: g.id === c.next_id ? 'next' : 'played' }
    if (g.id === c.next_id) return { text: withHours('Up next'), tone: 'next' }
  }
  const time = g.playtime_known && g.playtime_minutes > 0 ? hoursMinutes(g.playtime_minutes) : null
  switch (g.status) {
    case 'done':
      return { text: 'Beat it', tone: 'played' }
    case 'playing':
      return { text: time ? `Playing · ${time} in` : 'Playing', tone: 'played' }
    case 'shelved':
      return { text: time ? `Shelved · ${time} in` : 'Shelved', tone: 'played' }
    case 'still_playing':
      return { text: time ? `Still playing · ${time}` : 'Still playing', tone: 'played' }
    case 'burnt_out':
      return { text: 'Burnt out', tone: 'quiet' }
    case 'played':
      return { text: time ? `Played ${time}` : 'Played', tone: 'played' }
    case 'sampled':
      return { text: time ? `Sampled ${time}` : 'Sampled', tone: 'quiet' }
    case 'abandoned':
      return { text: time ? `Abandoned after ${time}` : 'Abandoned', tone: 'quiet' }
    case 'hidden':
      return { text: 'Not for me', tone: 'quiet' }
    default:
      return { text: withHours('Not started'), tone: 'quiet' }
  }
}

/** A game in the series you don't own: greyed, and a link to where it's sold. */
function MissingCover({ e, n }: { e: SeriesEntry; n: number }) {
  // Read once when the card mounts, so a render never reads the clock.
  const [now] = useState(() => new Date().getFullYear())
  const upcoming = e.year != null && e.year > now
  const href = e.steam_appid
    ? `https://store.steampowered.com/app/${e.steam_appid}`
    : `https://store.steampowered.com/search/?term=${encodeURIComponent(e.name)}`
  const tone = Number(e.qid.replace(/\D/g, '')) || 0
  return (
    <li className="flex w-[132px] shrink-0 flex-col gap-2">
      <a
        href={href}
        target="_blank"
        rel="noreferrer"
        className="group relative block rounded-[10px]"
        title={`${e.name}: not in your library`}
        aria-label={`${e.name}, number ${n}${e.year ? `, ${e.year}` : ''}. Not in your library; opens the store page`}
      >
        {e.cover || (e.steam_appid && !e.cover_checked) ? (
          <Cover
            game={{ id: tone, name: e.name, appid: e.steam_appid, art_square: e.cover, header_image: '' }}
            titleSize="text-[14px]"
            className="w-[132px] opacity-45 grayscale transition group-hover:opacity-80 group-hover:grayscale-0"
          />
        ) : (
          // No art anywhere: an outlined card with the name, so it reads as
          // "exists, not yours" instead of a broken image.
          <div className="flex aspect-[2/3] w-[132px] flex-col justify-end rounded-[10px] border border-dashed border-line-strong p-3 transition group-hover:border-muted">
            <span className="font-display text-[14px] leading-[1.15] font-bold text-muted">{e.name}</span>
          </div>
        )}
        <span
          aria-hidden="true"
          className="pointer-events-none absolute inset-x-0 top-0 rounded-t-[10px] bg-linear-to-b from-black/65 to-transparent px-2.5 pt-2 pb-5 font-mono text-[10.5px] text-white/85"
        >
          #{n}
          {e.year ? ` · ${e.year}` : ''}
        </span>
      </a>
      <div className="text-[13px] leading-[1.35] text-dim">
        <span className="line-clamp-2 text-muted">{e.name}</span>
        {upcoming ? `Coming ${e.year}` : 'Not in your library'}
      </div>
    </li>
  )
}

function SeriesCover({ g, n, s, onOpen }: { g: GameView; n: number; s: Series; onOpen: (g: GameView) => void }) {
  const year = yearOf(s.c, g)
  const st = stateOf(g, s)
  const sub = s.c.subseries?.find((x) => x.ids.includes(g.id))?.name
  const isNext = g.id === s.c.next_id
  return (
    <li className="flex w-[132px] shrink-0 flex-col gap-2">
      <button
        type="button"
        onClick={() => onOpen(g)}
        className="group relative block rounded-[10px] text-left"
        title={g.name}
        aria-label={`${g.name}, number ${n}${year ? `, ${year}` : ''}. ${st.text}`}
      >
        <Cover
          game={g}
          titleSize="text-[14px]"
          className={cx(
            'w-[132px] transition-transform group-hover:-translate-y-0.5',
            isNext && 'ring-2 ring-accent ring-offset-2 ring-offset-ground',
          )}
        />
        <span
          aria-hidden="true"
          className="pointer-events-none absolute inset-x-0 top-0 rounded-t-[10px] bg-linear-to-b from-black/65 to-transparent px-2.5 pt-2 pb-5 font-mono text-[10.5px] text-white/85"
        >
          #{n}
          {year ? ` · ${year}` : ''}
        </span>
      </button>
      <div
        className={cx(
          'text-[13px] leading-[1.35]',
          st.tone === 'next' && 'font-bold text-accent',
          st.tone === 'played' && 'text-ink',
          st.tone === 'quiet' && 'text-dim',
        )}
      >
        {st.text}
        {sub && <span className="block font-mono text-[10.5px] text-dim">{sub.replace(`${s.c.name}: `, '')}</span>}
      </div>
    </li>
  )
}

/** One series. Memoised: opening a game re-renders the page, not ~190 rows. */
export const SeriesRow = memo(function SeriesRow({ s, onOpen }: { s: Series; onOpen: (g: GameView) => void }) {
  const { c, main, side } = s
  const next = main.find((g) => g.id === c.next_id) ?? c.members.find((g) => g.id === c.next_id)
  const guided = !!c.guide
  const missing = c.missing?.length ?? 0
  const countLine = [
    guided ? `${main.length} main ${main.length === 1 ? 'game' : 'games'} owned` : `${main.length} owned`,
    missing > 0 && `${missing} more not in your library`,
    s.finished ? 'All played' : s.started === 0 ? 'Not started yet' : `${s.started} started`,
  ]
    .filter(Boolean)
    .join(' · ')

  return (
    <section
      aria-labelledby={`series-${c.key}`}
      className="grid grid-cols-1 gap-y-6 border-b border-line py-8 lg:grid-cols-[280px_minmax(0,1fr)] lg:gap-x-12"
    >
      <div className="flex flex-col gap-2">
        <h2 id={`series-${c.key}`} className="font-display text-[28px] leading-[1.1] font-[750] tracking-[-0.02em]">
          {c.name}
        </h2>
        <div className="mono">{countLine}</div>
        <p className="mt-2 text-[15px] leading-[1.55] text-muted">
          {c.guide?.note || (c.members.length < 2 ? "You own one game in this series. The others are shown greyed out." : NO_GUIDE_NOTE)}
        </p>
        {c.guide_stale && (
          <p className="text-[13px] text-dim">You've added games since this guide was written. It refreshes on the next enrichment.</p>
        )}
        {next ? (
          <button
            type="button"
            onClick={() => onOpen(next)}
            className="mt-0.5 self-start py-2 text-left text-[15px] font-bold text-accent hover:underline"
          >
            {s.started === 0 || (next.id === c.start_id && !isStarted(next)) ? 'Start with' : 'Continue with'} {next.name}{'\u00a0'}→
          </button>
        ) : (
          s.finished && <p className="mt-1.5 text-[15px] text-muted">You've played the whole main line.</p>
        )}
      </div>

      <div className="flex min-w-0 flex-col gap-[18px]">
        <ol className="-m-1.5 flex gap-[18px] overflow-x-auto p-1.5 pb-2" aria-label={`${c.name} main games`}>
          {s.row.map((item, i) =>
            item.kind === 'owned' ? (
              <SeriesCover key={item.g.id} g={item.g} n={i + 1} s={s} onOpen={onOpen} />
            ) : (
              <MissingCover key={item.e.qid} e={item.e} n={i + 1} />
            ),
          )}
        </ol>
        {side.length > 0 && (
          <div className="flex flex-wrap gap-x-7 gap-y-1 text-[13.5px] text-dim">
            {side.map((grp) => (
              <p key={grp.label}>
                {grp.label}:{' '}
                {grp.games.map((g, i) => (
                  <span key={g.id}>
                    {i > 0 && <span aria-hidden="true"> · </span>}
                    <button
                      type="button"
                      onClick={() => onOpen(g)}
                      className="text-muted underline decoration-transparent underline-offset-4 hover:text-ink hover:decoration-underline"
                    >
                      {g.name}
                    </button>
                  </span>
                ))}
              </p>
            ))}
          </div>
        )}
      </div>
    </section>
  )
})
