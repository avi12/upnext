import { useCallback, useEffect, useId, useRef, useState, type ReactNode } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { Bookmark, BookmarkCheck, Clapperboard, ExternalLink } from 'lucide-react'
import { api } from '../api'
import type { GameView, OverrideStatus, Screenshot } from '../types'
import { PlayMark } from './Shell'
import { compactNumber, cx, hoursToText, linuxClass, linuxLabel, minutesToText, plural, toneVar } from '../lib/format'
import { useLater, useSetEnding, useSetStatus } from './library/sheetActions'
import { StatusMenu, type Reasons } from './StatusMenu'
import { FullImage, Lightbox, ScreenshotStrip } from './Gallery'
import { editionsText, ownedText, playableCopies, playUrl, seriesFor, storeVia } from './library/sheetText'
import { gameYear } from '../lib/gameText'
import { Standing } from './library/Standing'
import { HowItPlays, PeopleSay, PlayerReviews, SeriesLine, Tags, Vibes } from './library/SheetSections'
import { DataLine, Details } from './library/SheetDetails'
import { BacklogVerdict } from './verdicts/BacklogVerdict'
import { ENDING_MARKS } from '../lib/verdicts'
import { useConfig } from '../lib/queries'

interface Props {
  game: GameView | null
  onClose: () => void
  onUpdated?: (g: GameView) => void
}

/** The game detail panel. Every page opens games through this one component;
 *  pass `game` to open it and `null` to close it. */
export function GameSheet({ game, onClose, onUpdated }: Props) {
  if (!game) return null
  // No key: opening another game while one is showing swaps it in place.
  return <SheetPanel game={game} onClose={onClose} onUpdated={onUpdated} />
}


const HERO_BTN =
  'flex h-10 w-10 items-center justify-center rounded-full bg-black/55 text-white backdrop-blur-sm transition hover:bg-black/75 disabled:pointer-events-none disabled:opacity-0'

/** The top of the sheet: the game's wide art, or the screenshot picked below
 *  it. ‹ › step through every screenshot in place; the corner button opens the
 *  full-screen viewer. */
function Hero({
  game,
  shots,
  selected,
  onSelect,
  onExpand,
}: {
  game: GameView
  shots: Screenshot[]
  /** Screenshot index, or -1 for the game's art. */
  selected: number
  onSelect: (index: number) => void
  onExpand: (index: number) => void
}) {
  const sources = [
    game.hero_url,
    game.appid ? `https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/${game.appid}/library_hero.jpg` : null,
    game.header_image,
  ].filter((s): s is string => !!s)
  const [idx, setIdx] = useState(0)
  const src = sources[idx]
  // With no art of its own, the first screenshot stands in for it.
  const min = src ? -1 : 0
  const cur = Math.max(selected, shots.length ? min : -1)
  const shot = cur >= 0 ? shots[cur] : null
  // One frame for the art and the screenshots, so picking a screenshot moves
  // nothing below it. The store's header, the last fallback, carries
  // the title in it, so it's shown whole over a blurred copy of itself.
  const whole = src === game.header_image && shots.length > 0
  return (
    <div
      className={cx('relative w-full shrink-0 overflow-hidden', shots.length ? 'aspect-video' : 'h-[200px] sm:h-[260px]')}
      style={{ background: toneVar(game.id) }}
    >
      {shot ? (
        <FullImage key={shot.full} shot={shot} className="h-full w-full object-cover" />
      ) : whole ? (
        <>
          <img src={src} alt="" aria-hidden="true" className="absolute inset-0 h-full w-full scale-110 object-cover opacity-60 blur-xl" />
          <img src={src} alt="" className="relative h-full w-full object-contain" onError={() => setIdx((i) => i + 1)} />
        </>
      ) : (
        src && <img src={src} alt="" className="h-full w-full object-cover" onError={() => setIdx((i) => i + 1)} />
      )}
      {shots.length > 0 && (
        <>
          <button
            type="button"
            onClick={() => onSelect(cur - 1)}
            disabled={cur <= min}
            aria-label="Previous screenshot"
            className={cx(HERO_BTN, 'absolute left-3 top-1/2 -translate-y-1/2')}
          >
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" aria-hidden="true">
              <path d="M15 18l-6-6 6-6" />
            </svg>
          </button>
          <button
            type="button"
            onClick={() => onSelect(cur + 1)}
            disabled={cur >= shots.length - 1}
            aria-label="Next screenshot"
            className={cx(HERO_BTN, 'absolute right-3 top-1/2 -translate-y-1/2')}
          >
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" aria-hidden="true">
              <path d="M9 18l6-6-6-6" />
            </svg>
          </button>
          <button
            type="button"
            onClick={() => onExpand(Math.max(0, cur))}
            aria-label="View screenshots full screen"
            title="Full screen"
            className={cx(HERO_BTN, 'absolute right-[68px] top-3.5 h-11 w-11')}
          >
            <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <path d="M14 4h6v6M10 20H4v-6M20 4l-7 7M4 20l7-7" />
            </svg>
          </button>
          {shot && (
            <span className="absolute bottom-3 left-3 rounded-full bg-black/55 px-2.5 py-0.5 font-mono text-[11px] text-white">
              {cur + 1} / {shots.length}
            </span>
          )}
        </>
      )}
    </div>
  )
}

function Stat({ label, children, className, sub }: { label: string; children: ReactNode; className?: string; sub?: string }) {
  return (
    <div className="min-w-0">
      <dt className="font-mono text-[10.5px] text-dim">{label}</dt>
      <dd className={cx('m-0 mt-1 truncate font-display text-2xl font-bold', className)}>{children}</dd>
      {sub && <dd className="m-0 mt-0.5 truncate text-[12.5px] text-dim">{sub}</dd>}
    </div>
  )
}

const STAT_LINK = 'underline decoration-underline decoration-2 underline-offset-[5px] hover:decoration-accent'

function SheetPanel({ game: initial, onClose, onUpdated }: { game: GameView; onClose: () => void; onUpdated?: (g: GameView) => void }) {
  const titleId = useId()
  const panelRef = useRef<HTMLDivElement>(null)
  const closeRef = useRef<HTMLButtonElement>(null)
  // The game on show: the one opened, or one picked from "In this series".
  const [shown, setShown] = useState(initial)
  const id = shown.id
  const setId = (next: number) => setShown((cur) => (cur.id === next ? cur : { ...cur, id: next }))
  const scrollRef = useRef<HTMLDivElement>(null)
  const openOther = (g: GameView) => {
    setShown(g)
    setGallery(null)
    setShot(-1)
    scrollRef.current?.scrollTo({ top: 0 })
  }

  // Fresh copy of the game; the list item shows until it arrives.
  const fresh = useQuery({
    queryKey: ['game', id],
    queryFn: () => api.game(id),
    placeholderData: shown,
  })
  const game = fresh.data ?? shown
  const shots = game.screenshots ?? []
  const [gallery, setGallery] = useState<number | null>(null)
  // Which screenshot the hero shows (-1: the game's art).
  const [shot, setShot] = useState(-1)
  const heroShot = Math.min(shot, shots.length - 1)

  // A different game opened from the page behind: show it instead.
  const [opened, setOpened] = useState(initial)
  if (initial.id !== opened.id) {
    setOpened(initial)
    setShown(initial)
    setGallery(null)
    setShot(-1)
  }
  // New game on show: start at the top.
  useEffect(() => {
    scrollRef.current?.scrollTo({ top: 0 })
  }, [id])

  const { data: config } = useConfig()
  // Just this game's series (under 'collections' so library changes refresh it).
  const { data: seriesData } = useQuery({ queryKey: ['collections', 'of', id], queryFn: () => api.seriesOf(id), staleTime: 300_000 })
  const collections = seriesData ? [seriesData] : undefined
  const linuxActive = config?.linux_active === true

  const updated = useCallback(
    (g: GameView) => {
      if (g.id !== id) setId(g.id)
      onUpdated?.(g)
    },
    [id, onUpdated],
  )
  const setStatus = useSetStatus(updated)
  const later = useLater(updated)
  const ending = useSetEnding(updated)

  // Keep the latest onClose without re-running the focus effect (parents pass
  // a new function each render, which used to pull focus back to Close).
  const onCloseRef = useRef(onClose)
  useEffect(() => {
    onCloseRef.current = onClose
  }, [onClose])

  // The page behind stays usable (click another game to swap it in). Focus
  // starts on Close, Esc or a click on empty page closes, focus goes back after.
  useEffect(() => {
    const before = document.activeElement as HTMLElement | null
    closeRef.current?.focus()
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault()
        onCloseRef.current()
      }
    }
    const onClick = (e: MouseEvent) => {
      const t = e.target as Element | null
      if (!t || !t.isConnected || panelRef.current?.contains(t)) return
      // The top bar stays usable beside the sheet: its links go, and the sheet with them.
      if (t.closest('header a')) return onCloseRef.current()
      // Controls (another game, a filter, a menu) do their own thing; overlays
      // like the screenshot viewer and toasts sit above the sheet.
      if (t.closest('a, button, input, select, textarea, label, summary, [data-opens-game], [role="button"], [role="dialog"], [role="menu"], [role="status"], [role="alert"]')) return
      onCloseRef.current()
    }
    document.addEventListener('keydown', onKey)
    document.addEventListener('click', onClick)
    return () => {
      document.removeEventListener('keydown', onKey)
      document.removeEventListener('click', onClick)
      before?.focus?.()
    }
  }, [])

  const series = seriesFor(game, collections)
  const seriesName = series?.collection.name ?? game.series?.[0] ?? null
  const copies = playableCopies(game)
  const primary = copies[0] ?? game.copies.find((c) => c.source === game.primary_source) ?? null
  const meta = [
    seriesName ? (series?.position ? `${seriesName} #${series.position}` : seriesName) : null,
    gameYear(game) || null,
    editionsText(game),
    ownedText(game),
  ].filter(Boolean)

  const story = hoursToText(game.hltb_main)
  const linux = linuxLabel(game.linux)
  const played = !game.playtime_known ? 'Not tracked' : game.playtime_minutes > 0 ? minutesToText(game.playtime_minutes) : 'Never'
  const launch = playUrl(game)
  const saved = game.saved_at != null
  const busy = setStatus.isPending

  // After a mark that ends something, the verdicts section offers a verdict,
  // pre-filled from the mark. The offer waits for the form fetched
  // after the mark: the editor keeps the answers it opens with, so a quick
  // "Write a verdict" on the form from before the mark would open it blank.
  const qc = useQueryClient()
  const [offerFor, setOfferFor] = useState<number | null>(null)
  // Only the latest mark's offer: an earlier one's fetch may land after it.
  const marks = useRef(0)
  const mark = (status: OverrideStatus | null, reasons?: Reasons) =>
    setStatus.mutate(
      { id: game.id, status, previous: game.override, reasons },
      {
        onSuccess: (g) => {
          const n = ++marks.current
          const ends = !!status && ENDING_MARKS.includes(status)
          if (!ends) setOfferFor(null)
          void qc.invalidateQueries({ queryKey: ['verdict', g.id] }).then(() => {
            if (ends && n === marks.current) setOfferFor(g.id)
          })
        },
      },
    )

  return (
    // From tablet widths the sheet opens under the top bar, which stays usable.
    <div className="pointer-events-none fixed inset-x-0 top-(--titlebar-h) bottom-0 z-[90] md:top-[calc(var(--titlebar-h)+4rem)]">
      {/* Dims the page on wide screens without blocking it. */}
      <div className="pointer-events-none absolute inset-0 hidden md:block" style={{ background: 'var(--sheet-veil)' }} aria-hidden="true" />
      <div
        ref={panelRef}
        role="dialog"
        data-tour="sheet"
        aria-labelledby={titleId}
        className="sheet-in pointer-events-auto absolute inset-0 overflow-hidden bg-sheet shadow-2xl sm:inset-4 sm:rounded-2xl md:left-auto md:w-[min(720px,calc(100vw-2rem))]"
      >
        <button
          ref={closeRef}
          type="button"
          onClick={onClose}
          aria-label="Close"
          className="absolute right-3.5 top-3.5 z-10 flex h-11 w-11 items-center justify-center rounded-full bg-ground/65 text-ink backdrop-blur-sm hover:bg-ground/85"
        >
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
            <path d="M6 6l12 12M18 6L6 18" />
          </svg>
        </button>

        <div ref={scrollRef} className="h-full overflow-y-auto overscroll-contain">
          <Hero game={game} shots={shots} selected={heroShot} onSelect={setShot} onExpand={setGallery} />

          <div className="flex flex-col gap-4 px-6 pb-10 pt-7 sm:px-10">
            <ScreenshotStrip shots={shots} name={game.name} selected={heroShot} onSelect={setShot} onExpand={setGallery} />
            <div className="font-mono text-[11px] text-dim">{meta.join(' · ')}</div>
            <h2 id={titleId} className="m-0 font-display text-[34px] font-[750] leading-[1.05] tracking-[-0.025em] sm:text-[40px]">
              {game.name}
            </h2>
            {game.short_description && (
              <p className="m-0 max-w-[62ch] text-[16.5px] leading-[1.55] text-muted">{game.short_description}</p>
            )}

            <div className="flex flex-wrap items-center gap-2.5">
              {launch && (
                <a
                  href={launch}
                  onClick={() => void api.diaryLaunch(game.id)}
                  className="btn-accent h-10 px-5 text-base"
                  title={primary ? `Play: ${storeVia(primary)}` : undefined}
                >
                  <PlayMark className="h-[13px] w-[13px]" fill="currentColor" />
                  Play
                </a>
              )}
              {game.trailer_url && (
                <a href={game.trailer_url} target="_blank" rel="noreferrer" className="btn-outline">
                  <Clapperboard aria-hidden="true" className="h-4 w-4 text-muted" />
                  Watch trailer
                </a>
              )}
              <button
                type="button"
                aria-pressed={saved}
                disabled={later.isPending}
                onClick={() => later.mutate({ id: game.id, save: !saved })}
                className="btn-outline"
              >
                {saved ? <BookmarkCheck aria-hidden="true" className="h-4 w-4 text-accent" /> : <Bookmark aria-hidden="true" className="h-4 w-4 text-muted" />}
                {saved ? 'Saved for later' : 'Save for later'}
              </button>
              {game.store_url && (
                <a href={game.store_url} target="_blank" rel="noreferrer" className="btn-outline">
                  <ExternalLink aria-hidden="true" className="h-4 w-4 text-muted" />
                  Store page
                </a>
              )}
              <StatusMenu
                game={game}
                onChange={mark}
                busy={busy}
                onEndless={() => ending.mutate({ id: game.id, noEnding: true })}
                hide={game}
                // Hidden from you, it's gone from the page behind too.
                onHidden={(scope) => scope && scope !== 'others' && onClose()}
              />
            </div>

            <Standing game={game} onUpdated={updated} />

            <dl
              className={cx(
                'mx-0 mb-0 mt-2.5 grid gap-x-3 gap-y-4 border-y border-line py-[18px]',
                linuxActive ? 'grid-cols-2 sm:grid-cols-4' : 'grid-cols-3',
              )}
            >
              <Stat label="Story" className={story ? undefined : 'text-dim'}>
                {story && game.hltb_url ? (
                  <a href={game.hltb_url} target="_blank" rel="noreferrer" className={STAT_LINK} title="How long to beat the main story">
                    {story}
                  </a>
                ) : (
                  (story ?? '—')
                )}
              </Stat>
              <Stat
                label="Liked it"
                className={game.review_pct != null ? undefined : 'text-dim'}
                // How many reviews the share is from, under it as the pick cards show it.
                sub={game.review_pct != null && game.review_total ? `${compactNumber(game.review_total)} ${game.review_total === 1 ? 'review' : 'reviews'}` : undefined}
              >
                <span title={game.review_desc ? `${game.review_desc}${game.review_total ? ` · ${plural(game.review_total, 'review')}` : ''}` : undefined}>
                  {game.review_pct != null ? `${game.review_pct}%` : '—'}
                </span>
              </Stat>
              {linuxActive && (
                <Stat label="Linux" className={linux ? linuxClass(game.linux) : 'text-dim'}>
                  {linux && game.appid ? (
                    <a
                      href={`https://www.protondb.com/app/${game.appid}`}
                      target="_blank"
                      rel="noreferrer"
                      className={STAT_LINK}
                      title={game.protondb_total ? `${plural(game.protondb_total, 'ProtonDB report')}` : 'ProtonDB'}
                    >
                      {linux}
                    </a>
                  ) : (
                    (linux ?? 'Unknown')
                  )}
                </Stat>
              )}
              <Stat label="You've played" className={game.playtime_minutes > 0 ? undefined : 'text-dim'}>
                {played}
              </Stat>
            </dl>

            <div className="flex flex-col gap-6 pt-1">
              <PeopleSay game={game} />
              <Vibes game={game} />
              <HowItPlays game={game} />
              <Tags game={game} />
              <SeriesLine game={game} collections={collections} onOpen={openOther} />
              <PlayerReviews key={game.id} game={game} />
              <BacklogVerdict key={`v${game.id}`} game={game} offer={offerFor === game.id} onOfferDone={() => setOfferFor(null)} />
              <Details
                game={game}
                onUpdated={updated}
              />
              {game.override_note && <p className="m-0 text-[14px] text-muted">Your note: {game.override_note}</p>}
              <DataLine game={game} linuxActive={linuxActive} />
            </div>
          </div>
        </div>
      </div>
      {gallery !== null && shots.length > 0 && (
        <Lightbox shots={shots} start={gallery} name={game.name} onClose={() => setGallery(null)} />
      )}
    </div>
  )
}
