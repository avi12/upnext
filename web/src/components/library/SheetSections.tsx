import { Fragment, useRef, useState, type ReactNode } from 'react'
import { Link } from 'react-router'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { api } from '../../api'
import type { Collection, GameView, ReviewTagGroup, ReviewText } from '../../types'
import { VERDICT, scaleOf, useReviewVocabulary } from '../../lib/reviewTags'
import { cx, monthYear, storeName, upperFirst } from '../../lib/format'
import { seriesFor, stripBBCode } from './sheetText'

export function Section({ title, children, id }: { title: string; children: ReactNode; id?: string }) {
  return (
    <section className="flex flex-col gap-1.5" aria-labelledby={id}>
      <h3 id={id} className="m-0 font-display text-base font-bold">
        {title}
      </h3>
      {children}
    </section>
  )
}

const TAG_SOURCE_NAME: Record<string, string> = {
  steam: 'Steam',
  reviews: 'Player reviews',
  gog: 'GOG',
  rawg: 'RAWG',
  igdb: 'IGDB',
  pcgw: 'PCGamingWiki',
}
const TAG_SOURCE_ORDER = ['steam', 'reviews', 'gog', 'rawg', 'igdb', 'pcgw']
const TAG_LIMIT = 16

/** The review summary, then what players praise, criticize or disagree
 *  about, and the short notes from the same reviews. */
export function PeopleSay({ game }: { game: GameView }) {
  const aspects = game.review_tags?.aspects ?? []
  const notes = game.review_tags?.notes ?? []
  if (!game.digest && aspects.length === 0 && notes.length === 0) return null
  return (
    <Section title="People say">
      {game.digest && <p className="m-0 text-base leading-[1.6]">{game.digest}</p>}
      {aspects.length > 0 && (
        <p className="m-0 text-[14.5px] leading-[1.75]">
          {aspects.map((a, i) => {
            const v = VERDICT[a.verdict]
            return (
              <Fragment key={a.aspect}>
                {i > 0 && (
                  <span className="text-faint" aria-hidden="true">
                    {'  ·  '}
                  </span>
                )}
                <span className={cx('whitespace-nowrap', v.className)} title={`${v.says} the ${a.aspect}`}>
                  <span aria-hidden="true">{v.mark} </span>
                  <span className="sr-only">{v.says} </span>
                  {a.aspect}
                  {i < aspects.length - 1 && <span className="sr-only">,</span>}
                </span>
              </Fragment>
            )
          })}
        </p>
      )}
      {notes.length > 0 && <p className="m-0 text-[14px] leading-[1.6] text-muted">Also mentioned: {notes.join(' · ')}</p>}
      {game.seed_date && <p className="m-0 text-[13px] text-dim">From UpNext's shared data, {monthYear(game.seed_date)}</p>}
    </Section>
  )
}

/** One review scale: a marker on a low-to-high track, and the value in words. */
function ScaleRow({ scale, value }: { scale: ReviewTagGroup; value: string }) {
  const at = scale.values.findIndex((v) => v.value === value)
  const last = scale.values.length - 1
  return (
    <Row label={scale.title}>
      <span className="flex items-center gap-3" title={`${upperFirst(scale.means)}.`}>
        <span aria-hidden="true" className="relative mx-1.5 h-4 w-24 shrink-0">
          <span className="absolute inset-x-0 top-1/2 h-px -translate-y-1/2 bg-line-strong" />
          {scale.values.map((v, k) => (
            <span
              key={v.value}
              className={cx(
                'absolute top-1/2 -translate-x-1/2 -translate-y-1/2 rounded-full',
                k === at ? 'h-3 w-3 bg-accent' : 'h-1.5 w-1.5 bg-dim',
              )}
              style={{ left: `${(k / last) * 100}%` }}
            />
          ))}
        </span>
        <span>
          {upperFirst(value)}
          <span className="sr-only">
            {' '}
            (from {scale.values[0].value} to {scale.values[last].value})
          </span>
        </span>
      </span>
    </Row>
  )
}

/** What playing it feels like, in the reviewers' images. */
export function Vibes({ game }: { game: GameView }) {
  if (!game.vibes) return null
  return (
    <Section title="Vibes">
      <p className="m-0 max-w-[62ch] text-base leading-[1.6]">{game.vibes}</p>
    </Section>
  )
}

/** How it plays, from the review tags: the moods, the scales the reviews
 *  speak to, and the skills it asks for. Hidden when reviews say none of it. */
export function HowItPlays({ game }: { game: GameView }) {
  const { data: vocab } = useReviewVocabulary()
  const tags = game.review_tags
  if (!tags || !vocab) return null
  const scales = vocab.scales.filter((s) => tags.scales[s.name])
  if (scales.length === 0 && tags.mood.length === 0 && tags.skills.length === 0) return null
  const words = (values: string[]) => upperFirst(values.join(', '))
  return (
    <Section title="How it plays">
      <dl className="m-0 flex flex-col">
        {tags.mood.length > 0 && <Row label={scaleOf(vocab, 'mood')?.title ?? 'Mood'}>{words(tags.mood)}</Row>}
        {scales.map((s) => (
          <ScaleRow key={s.name} scale={s} value={tags.scales[s.name]} />
        ))}
        {tags.skills.length > 0 && <Row label={scaleOf(vocab, 'skills')?.title ?? 'Asks for'}>{words(tags.skills)}</Row>}
      </dl>
    </Section>
  )
}

/** The searchable tags as a sentence, with a per-source breakdown. RAWG's label links its page, as its terms ask
 *  wherever its data shows; the sheet's "Info from" line links it too. */
export function Tags({ game }: { game: GameView }) {
  const [bySource, setBySource] = useState(false)
  const [all, setAll] = useState(false)
  const sources = Object.keys(game.tags_by_source ?? {})
    .filter((k) => (game.tags_by_source[k]?.length ?? 0) > 0)
    .sort((a, b) => {
      const ia = TAG_SOURCE_ORDER.indexOf(a)
      const ib = TAG_SOURCE_ORDER.indexOf(b)
      return (ia < 0 ? 99 : ia) - (ib < 0 ? 99 : ib)
    })
  if (game.tags.length === 0) return null
  const shown = all ? game.tags : game.tags.slice(0, TAG_LIMIT)
  const hidden = game.tags.length - shown.length
  return (
    <Section title="Tags">
      {!bySource ? (
        <p className="m-0 text-[15px] leading-[1.65] text-muted">
          {shown.join(', ')}
          {hidden > 0 && (
            <>
              {' '}
              <button type="button" className="btn-quiet inline text-ink underline decoration-underline underline-offset-4" onClick={() => setAll(true)}>
                and {hidden} more
              </button>
            </>
          )}
          {sources.length > 0 && (
            <button
              type="button"
              aria-pressed={false}
              className="ml-3 inline text-ink underline decoration-underline underline-offset-4 hover:decoration-accent"
              onClick={() => setBySource(true)}
            >
              By source
            </button>
          )}
        </p>
      ) : (
        <div className="flex flex-col gap-2 text-[15px] leading-[1.65] text-muted">
          <dl className="m-0 flex flex-col gap-1.5">
            {sources.map((s) => (
              <div key={s}>
                <dt className="inline font-mono text-[10.5px] text-dim">
                  {s === 'rawg' && game.rawg_url ? (
                    <a href={game.rawg_url} target="_blank" rel="noreferrer" className="underline decoration-underline underline-offset-4 hover:text-ink">
                      {TAG_SOURCE_NAME[s]}
                    </a>
                  ) : (
                    (TAG_SOURCE_NAME[s] ?? s)
                  )}{' '}
                </dt>
                <dd className="m-0 inline">{game.tags_by_source[s].join(', ')}</dd>
              </div>
            ))}
          </dl>
          <button
            type="button"
            aria-pressed={true}
            className="self-start text-ink underline decoration-underline underline-offset-4 hover:decoration-accent"
            onClick={() => setBySource(false)}
          >
            All together
          </button>
        </div>
      )}
    </Section>
  )
}

/** "Metro 2033 Redux → Last Light Redux → Exodus", linking to Collections. */
export function SeriesLine({
  game,
  collections,
  onOpen,
}: {
  game: GameView
  collections: Collection[] | undefined
  /** Open another game of the series in place. */
  onOpen?: (g: GameView) => void
}) {
  const s = seriesFor(game, collections)
  if (!s || s.main.length + s.side.length + (s.collection.missing?.length ?? 0) < 2) return null
  const item = (m: GameView) =>
    m.id === game.id ? (
      <span className="font-semibold text-ink" aria-current="true">
        {m.name}
      </span>
    ) : onOpen ? (
      <button
        type="button"
        onClick={() => onOpen(m)}
        className="text-left text-ink underline decoration-underline underline-offset-4 hover:decoration-accent"
      >
        {m.name}
      </button>
    ) : (
      <Link to="/collections" className="text-ink underline decoration-underline underline-offset-4 hover:decoration-accent">
        {m.name}
      </Link>
    )
  return (
    <Section title="In this series">
      <p className="m-0 text-[15px] leading-[1.65] text-muted">
        {s.main.map((m, i) => (
          <Fragment key={m.id}>
            {i > 0 && <span aria-label="then"> → </span>}
            {item(m)}
          </Fragment>
        ))}
      </p>
      {s.side.length > 0 && (
        <p className="m-0 text-[14px] leading-[1.65] text-muted">
          Also in the series:{' '}
          {s.side.map((m, i) => (
            <Fragment key={m.id}>
              {i > 0 && ', '}
              {item(m)}
            </Fragment>
          ))}
        </p>
      )}
      {s.collection.missing?.length > 0 && (
        <p className="m-0 text-[14px] leading-[1.65] text-dim">
          Not in your library:{' '}
          {s.collection.missing
            .slice(0, 6)
            .map((e) => (e.year ? `${e.name} (${e.year})` : e.name))
            .join(', ')}
          {s.collection.missing.length > 6 && `, and ${s.collection.missing.length - 6} more`}
        </p>
      )}
    </Section>
  )
}

function reviewMeta(r: ReviewText): string {
  const parts: string[] = []
  if (r.positive === true) parts.push('Recommends it')
  if (r.positive === false) parts.push("Doesn't recommend it")
  if (r.rating != null) parts.push(`${Math.round(r.rating)} of 5 stars`)
  parts.push(storeName(String(r.source)))
  if (r.hours != null) parts.push(`${r.hours}h played`)
  if (r.posted) parts.push(String(new Date(r.posted * 1000).getFullYear()))
  if (r.votes > 0) parts.push(`${r.votes.toLocaleString()} found it helpful`)
  return parts.join(' · ')
}

function Review({ r }: { r: ReviewText }) {
  const [open, setOpen] = useState(false)
  const text = stripBBCode(r.text)
  const long = text.length > 360
  return (
    <li className="flex flex-col gap-1.5 border-t border-line py-4 first:border-t-0 first:pt-2">
      <div className={cx('font-mono text-[10.5px]', r.positive === false ? 'text-warn' : 'text-dim')}>{reviewMeta(r)}</div>
      {r.title && <div className="font-semibold">{r.title}</div>}
      <p className={cx('m-0 whitespace-pre-line text-[15px] leading-[1.6]', !open && long && 'line-clamp-5')}>{text}</p>
      {long && (
        <button type="button" className="btn-quiet self-start text-[13.5px] underline decoration-underline underline-offset-4" onClick={() => setOpen((v) => !v)}>
          {open ? 'Show less' : 'Read the rest'}
        </button>
      )}
    </li>
  )
}

/** The most helpful player reviews, folded away until asked for. A game whose
 *  summary came from UpNext's shared data has none stored, so opening the
 *  list fetches them. */
export function PlayerReviews({ game }: { game: GameView }) {
  const list = game.review_texts ?? []
  const onDemand = list.length === 0 && !!game.seed_date
  const qc = useQueryClient()
  // Asked once per game page: a toggle can arrive twice before a render.
  const asked = useRef(false)
  const fetchNow = useMutation({
    mutationFn: () => api.fetchReviews(game.id),
    onSuccess: (fresh) => qc.setQueryData(['game', game.id], fresh),
  })
  if (list.length === 0 && !onDemand) return null
  return (
    <details
      className="group"
      onToggle={(e) => {
        if (onDemand && e.currentTarget.open && !asked.current) {
          asked.current = true
          fetchNow.mutate()
        }
      }}
    >
      <summary className="flex min-h-11 cursor-pointer list-none items-center gap-2 font-display text-base font-bold [&::-webkit-details-marker]:hidden">
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" aria-hidden="true" className="text-dim transition-transform group-open:rotate-90">
          <path d="M9 5l7 7-7 7" />
        </svg>
        What players wrote
        {list.length > 0 && <span className="font-mono text-[11px] font-normal text-dim">{list.length}</span>}
      </summary>
      {list.length > 0 ? (
        <ul className="m-0 list-none p-0 pl-5">
          {list.map((r, i) => (
            <Review key={`${r.source}-${i}`} r={r} />
          ))}
        </ul>
      ) : (
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1 pl-5 text-[14px] text-dim" aria-live="polite">
          {fetchNow.isError ? (
            <>
              <span>Couldn't fetch them: {fetchNow.error.message}</span>
              <button type="button" className="btn-quiet text-[13.5px] underline decoration-underline underline-offset-4" onClick={() => fetchNow.mutate()}>
                Try again
              </button>
            </>
          ) : fetchNow.isSuccess ? (
            <span>The store has no player reviews in English to show.</span>
          ) : (
            <span>Fetching what players wrote…</span>
          )}
        </div>
      )}
    </details>
  )
}

export function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="grid grid-cols-[7.5rem_1fr] gap-4 py-1.5">
      <dt className="pt-[3px] font-mono text-[10.5px] text-dim">{label}</dt>
      <dd className="m-0 text-[15px] leading-[1.55]">{children}</dd>
    </div>
  )
}
