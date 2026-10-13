import { useEffect, useId, useRef, useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { api } from '../../api'
import type { GameView } from '../../types'
import { compactNumber, cx, hoursToText, linuxClass } from '../../lib/format'
import { Cover } from '../Cover'
import { linuxShown } from './text'

/** How long the pointer rests on the art before the quick look opens, so
 *  passing over a card on the way somewhere else doesn't open it. */
const OPEN_DELAY_MS = 450
/** How long the quick look waits after the pointer leaves, so it can cross
 *  the gap from the art to the quick look. */
const CLOSE_DELAY_MS = 180
/** Tags the quick look shows at most. */
const TAGS = 5

/** A pick's art. It opens the game page, and says so: a pointer, a
 *  slight zoom and "Details" on hover. Resting the mouse on it opens a quick
 *  look (screenshots, story length, reviews, Linux and the top tags) to look
 *  before choosing; keyboard users reach the same through a "Quick look"
 *  button that shows when it has focus. Phones have no hover, so a tap
 *  still just opens the page. The title stays the keyboard's way to the
 *  page, so the art itself isn't a tab stop. */
export function PickArt({
  game,
  onOpen,
  showLinux,
  className,
}: {
  game: GameView
  onOpen: () => void
  showLinux: boolean
  className?: string
}) {
  const qc = useQueryClient()
  const id = useId()
  const [open, setOpen] = useState(false)
  const timer = useRef<number | undefined>(undefined)
  const wrapRef = useRef<HTMLDivElement>(null)
  const triggerRef = useRef<HTMLButtonElement>(null)
  const wait = (fn: () => void, ms: number) => {
    window.clearTimeout(timer.current)
    timer.current = window.setTimeout(fn, ms)
  }
  useEffect(() => () => window.clearTimeout(timer.current), [])

  // Focus goes back to "Quick look" only when it was in the quick look, so
  // Esc after a mouse hover doesn't put a focus ring on the card.
  const close = (refocus = false) => {
    window.clearTimeout(timer.current)
    setOpen(false)
    if (refocus && wrapRef.current?.contains(document.activeElement)) triggerRef.current?.focus()
  }
  const openPage = () => {
    close()
    onOpen()
  }

  return (
    <div
      ref={wrapRef}
      className={cx('relative', className)}
      onPointerEnter={(e) => {
        if (e.pointerType !== 'mouse') return
        // The whole game, with its screenshots, is fetched once the pointer
        // arrives, so the quick look usually has them when it opens.
        void qc.prefetchQuery({ queryKey: ['game', game.id], queryFn: () => api.game(game.id), staleTime: 60_000 })
        wait(() => setOpen(true), open ? 0 : OPEN_DELAY_MS)
      }}
      onPointerLeave={(e) => {
        if (e.pointerType === 'mouse') wait(() => setOpen(false), CLOSE_DELAY_MS)
      }}
    >
      <button
        type="button"
        onClick={openPage}
        className="group relative block w-full cursor-pointer overflow-hidden rounded-[10px]"
        aria-label={`Open ${game.name}`}
        tabIndex={-1}
      >
        <Cover game={game} shape="wide" titleSize="text-[18px]" className="w-full transition-transform duration-200 group-hover:scale-[1.03]" />
        <span
          aria-hidden="true"
          className="pointer-events-none absolute inset-x-0 bottom-0 flex justify-end bg-gradient-to-t from-black/70 to-transparent px-3 pt-6 pb-2 text-[13px] font-semibold text-white opacity-0 transition-opacity duration-200 group-hover:opacity-100"
        >
          Details ›
        </span>
      </button>
      {/* Out of sight until it has focus, so the keyboard has the quick look
          too without a second button on every card for the mouse. */}
      <button
        ref={triggerRef}
        type="button"
        onClick={() => (open ? close() : setOpen(true))}
        aria-expanded={open}
        aria-controls={open ? id : undefined}
        className="sr-only rounded-md bg-ground/85 px-2.5 py-1.5 text-[13px] font-semibold text-ink focus-visible:not-sr-only focus-visible:absolute focus-visible:top-2 focus-visible:left-2"
      >
        Quick look
      </button>
      {open && <QuickLook id={id} game={game} showLinux={showLinux} onOpen={openPage} onClose={() => close(true)} />}
    </div>
  )
}

/** The quick look itself: a screenshot to step through, the numbers that
 *  decide most picks, the top tags, and Open for the full page. */
function QuickLook({
  id,
  game: slim,
  showLinux,
  onOpen,
  onClose,
}: {
  id: string
  game: GameView
  showLinux: boolean
  onOpen: () => void
  onClose: () => void
}) {
  // A pick carries the game without its screenshots; the full one has them.
  const full = useQuery({ queryKey: ['game', slim.id], queryFn: () => api.game(slim.id), staleTime: 60_000 })
  const g = full.data ?? slim
  const shots = full.data?.screenshots ?? []
  const [shot, setShot] = useState(0)
  const at = Math.min(shot, Math.max(0, shots.length - 1))

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault()
        onClose()
      }
    }
    document.addEventListener('keydown', onKey)
    return () => document.removeEventListener('keydown', onKey)
  }, [onClose])

  const story = hoursToText(g.hltb_main)
  const linux = showLinux ? linuxShown(g) : null
  const tags = g.tags.slice(0, TAGS)
  const step = (by: number) => setShot((at + by + shots.length) % shots.length)

  return (
    <div
      id={id}
      role="dialog"
      aria-label={`Quick look: ${g.name}`}
      // As wide as the art, so it never reaches into the next pick's column.
      className="fade-up absolute inset-x-0 top-full z-30 mt-2 flex flex-col gap-3 rounded-xl border border-line-strong bg-sheet p-3 shadow-[0_12px_32px_rgb(0_0_0/0.35)]"
    >
      {full.isPending ? (
        <div className="shimmer aspect-video w-full rounded-lg" aria-hidden="true" />
      ) : shots.length > 0 ? (
        <div className="relative aspect-video w-full overflow-hidden rounded-lg bg-raised">
          <img src={shots[at].thumb} alt={`${g.name}, screenshot ${at + 1} of ${shots.length}`} className="h-full w-full object-cover" />
          {shots.length > 1 && (
            <>
              <button
                type="button"
                onClick={() => step(-1)}
                aria-label="Previous screenshot"
                className="absolute top-1/2 left-1.5 flex h-8 w-8 -translate-y-1/2 items-center justify-center rounded-full bg-ground/70 text-ink hover:bg-ground/90"
              >
                ‹
              </button>
              <button
                type="button"
                onClick={() => step(1)}
                aria-label="Next screenshot"
                className="absolute top-1/2 right-1.5 flex h-8 w-8 -translate-y-1/2 items-center justify-center rounded-full bg-ground/70 text-ink hover:bg-ground/90"
              >
                ›
              </button>
              <span className="absolute right-2 bottom-1.5 rounded bg-ground/70 px-1.5 font-mono text-[10.5px] text-ink">
                {at + 1} / {shots.length}
              </span>
            </>
          )}
        </div>
      ) : null}

      <dl className="m-0 grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-[13.5px]">
        <dt className="text-dim">Story</dt>
        <dd className="m-0 text-ink">{story ? `${story} to beat` : 'Not known yet'}</dd>
        <dt className="text-dim">Liked it</dt>
        <dd className="m-0 text-ink">
          {g.review_pct != null ? `${g.review_pct}%${g.review_total ? ` · ${compactNumber(g.review_total)} ${g.review_total === 1 ? 'review' : 'reviews'}` : ''}` : 'No reviews yet'}
        </dd>
        {linux && (
          <>
            <dt className="text-dim">Linux</dt>
            <dd className={cx('m-0', linuxClass(g.linux))}>● {linux}</dd>
          </>
        )}
      </dl>

      {tags.length > 0 && (
        <ul className="m-0 flex list-none flex-wrap gap-1.5 p-0" aria-label="Top tags">
          {tags.map((t) => (
            <li key={t} className="rounded-full bg-raised px-2.5 py-0.5 text-[12.5px] text-muted">
              {t}
            </li>
          ))}
        </ul>
      )}

      <div className="flex items-center justify-between gap-3">
        <button type="button" className="btn-accent h-9 px-4 text-[14px]" onClick={onOpen}>
          Open
        </button>
        <span className="text-[12.5px] text-dim">Esc to close</span>
      </div>
    </div>
  )
}
