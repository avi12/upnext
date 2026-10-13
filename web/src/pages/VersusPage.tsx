import { useState, type ReactNode } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Link, useSearchParams } from 'react-router'
import { api, ApiError } from '../api'
import type { DecideResult, GameView } from '../types'
import { Cover } from '../components/Cover'
import { GameSheet } from '../components/GameSheet'
import { compactNumber, cx, hoursToText, linuxClass, minutesToText, partOfDay, relativeTime } from '../lib/format'
import { PlayIcon, TrailerIcon } from '../components/pick/icons'
import { linuxShown, storesText } from '../components/pick/text'
import { yearOf } from '../lib/gameText'
import { patchGame } from '../components/pick/useGameActions'
import { usageText } from '../lib/jobs'
import { Money } from '../components/Money'
import { useToast } from '../components/useToast'
import { useConfig } from '../lib/queries'

const GRID = 'grid grid-cols-[minmax(0,1fr)_72px_minmax(0,1fr)] md:grid-cols-[minmax(0,1fr)_120px_minmax(0,1fr)]'

type Tone = 'ink' | 'dim' | 'linux'
interface Row {
  label: string
  small?: boolean
  weight?: 'strong' | 'normal'
  tone: (g: GameView) => Tone
  value: (g: GameView) => ReactNode
}

function storyText(g: GameView): string | null {
  if (g.hltb_main == null || g.hltb_main <= 0) return null
  const h = hoursToText(g.hltb_main)
  if (!h) return null
  if (g.hltb_main < 1) return h
  const n = h.replace(/h$/, '')
  return `${n} hour${n === '1' ? '' : 's'}`
}

function likedText(g: GameView): string | null {
  if (g.review_pct == null) return null
  return g.review_total ? `${g.review_pct}% of ${compactNumber(g.review_total)}` : `${g.review_pct}%`
}

function playedText(g: GameView): string {
  if (!g.playtime_known && g.playtime_minutes <= 0) return 'Not tracked'
  if (g.playtime_minutes <= 0) return 'Never'
  const when = g.last_played ? `, last ${relativeTime(g.last_played)}` : ''
  return `${minutesToText(g.playtime_minutes)}${when}`
}

function tagsText(g: GameView): string | null {
  const genres = new Set(g.genres.map((x) => x.toLowerCase()))
  const tags = g.tags.filter((t) => !genres.has(t.toLowerCase())).slice(0, 6)
  return tags.length ? tags.join(', ') : null
}

const ROWS: Row[] = [
  { label: 'Story length', value: (g) => storyText(g) ?? 'Unknown', tone: (g) => (storyText(g) ? 'ink' : 'dim') },
  { label: 'Liked it', value: (g) => likedText(g) ?? 'No reviews yet', tone: (g) => (likedText(g) ? 'ink' : 'dim') },
  // Other backlog players' verdicts, when either game has them.
  {
    label: 'Backlog players',
    small: true,
    value: (g) => g.backlog?.headline ?? 'Too few verdicts yet',
    tone: (g) => (g.backlog ? 'ink' : 'dim'),
  },
  { label: 'On Linux', value: (g) => linuxShown(g) ?? 'Unknown', tone: (g) => (linuxShown(g) ? 'linux' : 'dim') },
  { label: 'Played', value: playedText, tone: (g) => (g.playtime_minutes > 0 ? 'ink' : 'dim') },
  { label: 'Genres', value: (g) => g.genres.join(', ') || 'Unknown', tone: (g) => (g.genres.length ? 'ink' : 'dim') },
  {
    label: 'Vibes',
    small: true,
    weight: 'normal',
    value: (g) => g.vibes ?? "Reviews don't say",
    tone: (g) => (g.vibes ? 'ink' : 'dim'),
  },
  { label: 'Tags', small: true, weight: 'normal', value: (g) => tagsText(g) ?? 'No tags yet', tone: (g) => (tagsText(g) ? 'ink' : 'dim') },
  {
    label: 'People say',
    small: true,
    weight: 'normal',
    value: (g) => g.digest ?? 'No summary yet',
    tone: () => 'dim',
  },
  {
    label: 'Released',
    weight: 'normal',
    value: (g) => [yearOf(g), storesText(g)].filter(Boolean).join(' · '),
    tone: () => 'dim',
  },
]

function toneClass(tone: Tone, g: GameView): string {
  if (tone === 'linux') return linuxClass(g.linux)
  return tone === 'dim' ? 'text-muted' : 'text-ink'
}

function parseId(v: string | null): number | null {
  const n = Number(v)
  return v && Number.isInteger(n) && n > 0 ? n : null
}

export function VersusPage() {
  const [params] = useSearchParams()
  const qc = useQueryClient()
  const aId = parseId(params.get('a'))
  const bId = parseId(params.get('b'))
  const prompt = params.get('prompt') ?? ''
  const [detail, setDetail] = useState<GameView | null>(null)

  const { data: config } = useConfig()
  const linuxActive = config?.linux_active === true
  // Help me decide stays an AI feature: with no AI set up it says so.
  const noAi = config != null && !config.claude_available
  const a = useQuery({ queryKey: ['game', aId], queryFn: () => api.game(aId!), enabled: aId != null })
  const b = useQuery({ queryKey: ['game', bId], queryFn: () => api.game(bId!), enabled: bId != null })

  const toast = useToast()
  const decide = useMutation({
    mutationFn: () => api.decide([aId!, bId!], prompt || undefined),
    onError: (e) => toast.error("Couldn't get an answer", e instanceof ApiError || e instanceof Error ? e.message : String(e)),
  })
  const verdict: DecideResult | undefined = decide.data
  const pod = partOfDay()
  const back = pod === 'late night' ? '← Back to the late-night lineup' : `← Back to ${pod}'s lineup`

  const top = (
    <section className="flex items-center justify-between pt-7">
      <Link to="/" className="-my-3 py-3 text-[14.5px] text-muted hover:text-ink">
        {back}
      </Link>
    </section>
  )

  if (aId == null || bId == null || aId === bId) {
    return (
      <div className="px-4 pb-16 sm:px-10">
        {top}
        <p className="pt-10 text-[15px] text-muted">Pick two different games from the lineup to compare them here.</p>
      </div>
    )
  }

  const failed = a.error ?? b.error
  if (failed) {
    return (
      <div className="px-4 pb-16 sm:px-10">
        {top}
        <div className="flex flex-col items-start gap-3 pt-10">
          <p className="text-[15px] text-muted">Couldn't load both games: {failed instanceof Error ? failed.message : String(failed)}</p>
          <button type="button" className="btn-outline" onClick={() => (a.refetch(), b.refetch())}>
            Try again
          </button>
        </div>
      </div>
    )
  }

  if (!a.data || !b.data) {
    return (
      <div className="px-4 pb-16 sm:px-10" aria-busy="true">
        {top}
        <div className={cx(GRID, 'items-end pt-6')} aria-hidden="true">
          <div className="shimmer aspect-[460/215] rounded-xl" />
          <div />
          <div className="shimmer aspect-[460/215] rounded-xl" />
        </div>
      </div>
    )
  }

  const ga = a.data
  const gb = b.data
  const pickedId = verdict?.pick_id
  const picked = pickedId === ga.id ? ga : pickedId === gb.id ? gb : null

  const onUpdated = (g: GameView) => {
    patchGame(qc, g)
    setDetail((d) => (d && d.id === g.id ? g : d))
  }

  // Store headers are about 460 px wide, so each cover stops at 520 px and
  // sits against the "vs" rather than being blown up to half the window.
  const cover = (g: GameView, side: 'left' | 'right') => {
    const isPick = picked?.id === g.id
    return (
      <button
        type="button"
        onClick={() => setDetail(g)}
        aria-label={`Open ${g.name}`}
        className={cx(
          'block w-full max-w-[520px] min-w-0 rounded-xl transition-[box-shadow,opacity]',
          side === 'left' ? 'justify-self-end' : 'justify-self-start',
          isPick && 'ring-[3px] ring-accent ring-offset-4 ring-offset-ground',
          picked && !isPick && 'opacity-60',
        )}
      >
        <Cover game={g} shape="wide" className="w-full rounded-xl" titleSize="text-[20px]" />
      </button>
    )
  }

  const title = (g: GameView, side: 'left' | 'right') => (
    <div className={cx('flex min-w-0 flex-col gap-1', side === 'left' && 'items-end text-right')}>
      <h2 className="font-display text-[22px] leading-[1.15] font-[750] tracking-[-0.02em] md:text-[30px]">{g.name}</h2>
      {picked?.id === g.id && <span className="font-mono text-[11px] text-accent">The pick</span>}
    </div>
  )

  const actions = (g: GameView, side: 'left' | 'right') => {
    const play = g.run_url ? (
      <a key="play" href={g.run_url} onClick={() => void api.diaryLaunch(g.id)} className="btn-ink h-11 min-w-0 px-5">
        <PlayIcon />
        <span className="max-w-[14rem] truncate">Play {g.name}</span>
      </a>
    ) : null
    const trailer = g.trailer_url ? (
      <a
        key="trailer"
        href={g.trailer_url}
        target="_blank"
        rel="noreferrer"
        className="inline-flex h-11 items-center gap-[7px] text-[14.5px] text-muted hover:text-ink"
      >
        <TrailerIcon />
        Trailer
      </a>
    ) : null
    return (
      <div className={cx('flex flex-wrap items-center gap-3.5', side === 'left' && 'justify-end')}>
        {side === 'left' ? [trailer, play] : [play, trailer]}
      </div>
    )
  }

  return (
    <div className="px-4 pb-16 sm:px-10">
      {top}

      <section aria-label="The two games" className="flex flex-col gap-3.5 pt-6">
        <div className={cx(GRID, 'items-center')}>
          {cover(ga, 'left')}
          <div aria-hidden="true" className="text-center font-display text-[32px] font-extrabold text-accent italic md:text-[44px]">
            vs
          </div>
          {cover(gb, 'right')}
        </div>
        <div className={cx(GRID, 'items-start')}>
          {title(ga, 'left')}
          <div />
          {title(gb, 'right')}
        </div>
      </section>

      <section aria-label="Side by side" className="pt-4">
        <dl>
          {ROWS.filter((r) => (linuxActive || r.label !== 'On Linux') && (r.label !== 'Backlog players' || ga.backlog || gb.backlog)).map((r) => (
            <div key={r.label} className={cx(GRID, 'items-baseline border-b border-line py-3.5')}>
              <dt className="order-2 text-center font-mono text-[10.5px] text-dim">{r.label}</dt>
              <dd
                className={cx(
                  'order-1 pr-2 text-right leading-[1.55]',
                  r.small ? 'text-[15.5px]' : 'text-[17px]',
                  r.weight === 'normal' ? 'font-normal' : 'font-semibold',
                  toneClass(r.tone(ga), ga),
                )}
              >
                <span className="sr-only">{ga.name}: </span>
                {r.value(ga)}
              </dd>
              <dd
                className={cx(
                  'order-3 pl-2 leading-[1.55]',
                  r.small ? 'text-[15.5px]' : 'text-[17px]',
                  r.weight === 'normal' ? 'font-normal' : 'font-semibold',
                  toneClass(r.tone(gb), gb),
                )}
              >
                <span className="sr-only">{gb.name}: </span>
                {r.value(gb)}
              </dd>
            </div>
          ))}
        </dl>
      </section>

      <section aria-label="Play" className={cx(GRID, 'pt-7')}>
        {actions(ga, 'left')}
        <div />
        {actions(gb, 'right')}
      </section>

      <section aria-labelledby="decide-heading" className="mt-9 flex flex-col gap-3 rounded-[14px] bg-raised px-5 py-6 sm:px-7">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <h2 id="decide-heading" className="font-display text-lg font-bold">
            Still can't choose?
          </h2>
          {noAi ? null : verdict ? (
            <button type="button" className="btn-outline h-11" onClick={() => decide.mutate()} disabled={decide.isPending}>
              Ask again
            </button>
          ) : (
            <button type="button" className="btn-accent text-base" onClick={() => decide.mutate()} disabled={decide.isPending}>
              {decide.isPending ? 'Thinking…' : 'Help me decide'}
            </button>
          )}
        </div>
        {noAi && !verdict && (
          <p className="m-0 text-base leading-[1.6] text-muted">
            Help me decide needs an AI to weigh the two, and none is set up.{' '}
            <Link to="/settings#picks" className="underline decoration-underline underline-offset-4 hover:text-ink">
              Set one up
            </Link>
          </p>
        )}
        <div aria-live="polite">
          {decide.isPending ? (
            <p className="flex items-center gap-2.5 text-base leading-[1.6] text-muted">
              <span className="h-[7px] w-[7px] animate-pulse rounded-full bg-accent" aria-hidden="true" />
              Weighing the two against {prompt ? 'what you asked for' : 'your mood'} and what you usually finish…
            </p>
          ) : decide.error ? (
            <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
              <p className="text-base leading-[1.6] text-muted">
                <span className="mr-2 inline-block h-[7px] w-[7px] rounded-full bg-bad align-middle" aria-hidden="true" />
                Couldn't get an answer: {decide.error instanceof ApiError || decide.error instanceof Error ? decide.error.message : String(decide.error)}
              </p>
              <button type="button" className="text-muted underline decoration-underline underline-offset-4 hover:text-ink" onClick={() => decide.mutate()}>
                Try again
              </button>
            </div>
          ) : verdict ? (
            <div className="flex flex-col gap-1.5">
              <p className="font-display text-[17px] font-bold">{picked ? `Go with ${picked.name}.` : 'Here’s the call.'}</p>
              <p className="text-base leading-[1.6] text-muted">{verdict.reason}</p>
              {(verdict.model || verdict.usage) && (
                <p className="mono mt-1">
                  {[verdict.model, verdict.usage && usageText(verdict.usage)].filter(Boolean).join(' · ')}
                  <Money cost={verdict} link prefix=" · " />
                </p>
              )}
            </div>
          ) : noAi ? null : (
            <p className="text-base leading-[1.6] text-muted">
              I'll weigh both against {prompt ? <>what you asked for (“{prompt}”)</> : 'your mood'} and what you tend to finish, then pick one.
            </p>
          )}
        </div>
      </section>

      <GameSheet game={detail} onClose={() => setDetail(null)} onUpdated={onUpdated} />
    </div>
  )
}
