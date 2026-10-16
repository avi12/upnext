import { useEffect, useMemo, useRef, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { api } from '../../api'
import type { GameView } from '../../types'
import { cx } from '../../lib/format'
import { Cover } from '../Cover'

/** How long the pointer rests on the art before the preview starts, so
 *  passing over a card on the way somewhere else changes nothing. */
const PEEK_DELAY_MS = 1000
/** How long each screenshot stays before the next one fades in. */
const STEP_MS = 2600
/** Screenshots the preview goes through at most; the game page has the rest. */
const SHOTS = 6
/** Tags the preview names at most, on one line. */
const TAGS = 3

const reducedMotion = () => typeof window.matchMedia === 'function' && window.matchMedia('(prefers-reduced-motion: reduce)').matches

/** A pick's art. It opens the game page, and says so: a pointer, a slight
 *  zoom and "Details" on hover. Resting the mouse on it for a second starts
 *  a preview inside the same frame: the game's screenshots fade in one
 *  after another, with its top tags along the bottom and a row of dots to
 *  pick one. Moving the mouse away ends it, so it never covers the buttons
 *  below and there is nothing to close. With reduced motion the pictures
 *  don't change on their own; the dots show them one at a time. Keyboard
 *  users get the same through a "Preview" button that shows when it has
 *  focus, with the arrow keys to step. Phones and the big screen have no
 *  hover, so a tap still just opens the page. The title stays the
 *  keyboard's way to the page, so the art itself isn't a tab stop. */
export function PickArt({
  game,
  onOpen,
  className,
}: {
  game: GameView
  onOpen: () => void
  className?: string
}) {
  // Armed on the first visit of the mouse or the keyboard: from then on the
  // full game (a pick carries it without screenshots) is fetched and its
  // screenshots stay in the frame, hidden, so they can fade out as well as in.
  const [armed, setArmed] = useState(false)
  const [peek, setPeek] = useState(false)
  // Set once the user picks a screenshot, which stops the slideshow there.
  const [manual, setManual] = useState(false)
  const [shot, setShot] = useState(0)
  // Screenshots that have loaded, by address, so a card that moves on to
  // another game doesn't take the last one's for loaded.
  const [loaded, setLoaded] = useState<ReadonlySet<string>>(new Set())
  const timer = useRef<number | undefined>(undefined)
  useEffect(() => () => window.clearTimeout(timer.current), [])

  const full = useQuery({ queryKey: ['game', game.id], queryFn: () => api.game(game.id), staleTime: 60_000, enabled: armed })
  const shots = useMemo(() => (full.data?.screenshots ?? []).slice(0, SHOTS), [full.data])
  const tags = (full.data ?? game).tags.slice(0, TAGS)
  const auto = peek && !manual && shots.length > 1 && !reducedMotion()

  // The slideshow moves on to the next screenshot that has loaded, and
  // waits on the current one while the next is still coming.
  useEffect(() => {
    if (!auto) return
    const tick = window.setInterval(() => {
      setShot((at) => {
        for (let by = 1; by < shots.length; by++) {
          const next = (at + by) % shots.length
          if (loaded.has(shots[next].thumb)) return next
        }
        return at
      })
    }, STEP_MS)
    return () => window.clearInterval(tick)
  }, [auto, shots, loaded])

  const start = () => {
    window.clearTimeout(timer.current)
    setShot(0)
    setManual(false)
    setPeek(true)
  }
  const stop = () => {
    window.clearTimeout(timer.current)
    setPeek(false)
    setManual(false)
  }
  const choose = (at: number) => {
    setManual(true)
    setShot(at)
  }
  const openPage = () => {
    stop()
    onOpen()
  }

  // With reduced motion the preview opens on the art itself, and a
  // screenshot shows only once a dot or an arrow key asks for it.
  const showing = peek && !(reducedMotion() && !manual) ? shot : -1

  return (
    <div
      className={cx('relative', className)}
      onPointerEnter={(e) => {
        if (e.pointerType !== 'mouse' || peek) return
        setArmed(true)
        window.clearTimeout(timer.current)
        timer.current = window.setTimeout(start, PEEK_DELAY_MS)
      }}
      onPointerLeave={(e) => {
        // Moving onto the dots is still on the art.
        if (e.pointerType === 'mouse' && !(e.relatedTarget instanceof Node && e.currentTarget.contains(e.relatedTarget))) stop()
      }}
    >
      <button
        type="button"
        onClick={openPage}
        className="group relative block w-full cursor-pointer overflow-hidden rounded-[10px]"
        aria-label={`Open ${game.name}`}
        tabIndex={-1}
      >
        <div className="relative transition-transform duration-200 group-hover:scale-[1.03]">
          <Cover game={game} shape="wide" titleSize="text-[18px]" className="w-full" />
          {armed &&
            shots.map((s, i) => (
              <img
                key={s.thumb}
                src={s.thumb}
                alt=""
                data-shot={i}
                onLoad={() => setLoaded((l) => new Set(l).add(s.thumb))}
                className={cx(
                  'absolute inset-0 h-full w-full object-cover transition-opacity duration-1000 ease-in-out motion-reduce:transition-none',
                  showing === i && loaded.has(s.thumb) ? 'opacity-100' : 'opacity-0',
                )}
              />
            ))}
        </div>
        <span
          aria-hidden="true"
          className={cx(
            'pointer-events-none absolute inset-x-0 bottom-0 flex items-end justify-between gap-3 bg-gradient-to-t from-black/75 to-transparent px-3 pt-8 pb-2 text-[13px] text-white transition-opacity duration-200',
            peek ? 'opacity-100' : 'opacity-0 group-hover:opacity-100',
          )}
        >
          <span className={cx('truncate transition-opacity duration-500', peek && tags.length > 0 ? 'opacity-90' : 'opacity-0')}>
            {tags.join(' · ')}
          </span>
          <span className="shrink-0 font-semibold">Details ›</span>
        </span>
      </button>
      {peek && shots.length > 1 && (
        <div
          role="group"
          aria-label="Screenshots"
          className="absolute top-2 right-2 flex items-center rounded-full bg-black/45 px-1 py-0.5 backdrop-blur-sm"
        >
          {shots.map((s, i) => (
            <button
              key={s.thumb}
              type="button"
              tabIndex={-1}
              aria-label={`Screenshot ${i + 1} of ${shots.length}`}
              aria-current={showing === i ? 'true' : undefined}
              onPointerEnter={() => choose(i)}
              onClick={() => choose(i)}
              className="flex h-5 w-4 cursor-pointer items-center justify-center"
            >
              <span
                className={cx(
                  'block h-1.5 w-1.5 rounded-full transition-colors',
                  showing === i ? 'bg-white' : 'bg-white/45',
                )}
              />
            </button>
          ))}
        </div>
      )}
      {/* Out of sight until it has focus, so the keyboard has the preview
          too without a second button on every card for the mouse. */}
      <button
        type="button"
        aria-pressed={peek}
        aria-keyshortcuts={shots.length > 1 ? 'ArrowLeft ArrowRight' : undefined}
        onClick={() => {
          setArmed(true)
          if (peek) stop()
          else start()
        }}
        onKeyDown={(e) => {
          if (e.key === 'Escape' && peek) {
            e.preventDefault()
            stop()
          } else if ((e.key === 'ArrowRight' || e.key === 'ArrowLeft') && peek && shots.length > 1) {
            e.preventDefault()
            const by = e.key === 'ArrowRight' ? 1 : -1
            choose(((showing < 0 ? (by > 0 ? -1 : 0) : showing) + by + shots.length) % shots.length)
          }
        }}
        onBlur={() => stop()}
        className="sr-only rounded-md bg-ground/85 px-2.5 py-1.5 text-[13px] font-semibold text-ink focus-visible:not-sr-only focus-visible:absolute focus-visible:top-2 focus-visible:left-2"
      >
        Preview
      </button>
      {/* Says which screenshot a step showed; the slideshow stays quiet. */}
      <span className="sr-only" aria-live="polite">
        {peek && manual && showing >= 0 ? `Screenshot ${showing + 1} of ${shots.length}` : ''}
      </span>
    </div>
  )
}
