import { useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import type { Screenshot } from '../types'
import { cx } from '../lib/format'

/** Four gameplay thumbnails under the hero. Clicking one shows it in the hero;
 *  the last says "+N" when there are more and opens the full-screen viewer.
 *  The four slide along so the one in the hero is always among them. */
export function ScreenshotStrip({
  shots,
  name,
  selected,
  onSelect,
  onExpand,
}: {
  shots: Screenshot[]
  name: string
  /** Screenshot showing in the hero, or -1 for the game's own art. */
  selected: number
  onSelect: (index: number) => void
  onExpand: (index: number) => void
}) {
  if (shots.length === 0) return null
  const start = Math.max(0, Math.min(selected - 2, shots.length - 4))
  const shown = shots.slice(start, start + 4)
  const more = shots.length - (start + shown.length)
  return (
    <ul className="m-0 grid list-none grid-cols-4 gap-2 p-0" aria-label={`${name} screenshots`}>
      {shown.map((s, k) => {
        const i = start + k
        const isMore = more > 0 && k === shown.length - 1
        return (
          <li key={s.thumb}>
            <button
              type="button"
              onClick={() => (isMore ? onExpand(i) : onSelect(i))}
              aria-label={isMore ? `All ${shots.length} screenshots, full screen` : `Show screenshot ${i + 1} of ${shots.length}`}
              aria-pressed={isMore ? undefined : i === selected}
              className={cx(
                'group relative block aspect-video w-full overflow-hidden rounded-lg bg-raised',
                i === selected && !isMore && 'ring-2 ring-accent ring-offset-2 ring-offset-sheet',
              )}
            >
              <img src={s.thumb} alt="" loading="lazy" className="h-full w-full object-cover transition-transform duration-200 group-hover:scale-[1.04]" />
              {isMore && (
                <span className="absolute inset-0 flex items-center justify-center bg-black/55 font-display text-lg font-bold text-white">
                  +{more}
                </span>
              )}
            </button>
          </li>
        )
      })}
    </ul>
  )
}

/** Full-screen screenshot viewer: arrows or ←/→ to move, Esc to close. */
export function Lightbox({
  shots,
  start,
  name,
  onClose,
}: {
  shots: Screenshot[]
  start: number
  name: string
  onClose: () => void
}) {
  const [i, setI] = useState(start)
  const root = useRef<HTMLDivElement>(null)
  const closeBtn = useRef<HTMLButtonElement>(null)
  const n = shots.length
  const go = (d: number) => setI((x) => (x + d + n) % n)
  // Keep the latest onClose without re-running the focus/keys effect.
  const closeRef = useRef(onClose)
  useEffect(() => {
    closeRef.current = onClose
  }, [onClose])

  useEffect(() => {
    const before = document.activeElement as HTMLElement | null
    closeBtn.current?.focus()
    // Capture on window so these keys never reach the game sheet underneath.
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault()
        e.stopPropagation()
        closeRef.current()
      } else if (e.key === 'ArrowRight' || e.key === 'ArrowLeft') {
        e.preventDefault()
        e.stopPropagation()
        setI((x) => (x + (e.key === 'ArrowRight' ? 1 : -1) + n) % n)
      } else if (e.key === 'Tab' && root.current) {
        e.stopPropagation()
        const nodes = Array.from(root.current.querySelectorAll<HTMLElement>('button'))
        const first = nodes[0]
        const last = nodes[nodes.length - 1]
        if (e.shiftKey && document.activeElement === first) {
          e.preventDefault()
          last?.focus()
        } else if (!e.shiftKey && document.activeElement === last) {
          e.preventDefault()
          first?.focus()
        }
      }
    }
    window.addEventListener('keydown', onKey, true)
    return () => {
      window.removeEventListener('keydown', onKey, true)
      before?.focus?.()
    }
  }, [n])

  const shot = shots[i]
  return createPortal(
    <div
      ref={root}
      role="dialog"
      aria-modal="true"
      aria-label={`${name} screenshots`}
      className="fade-up fixed inset-x-0 top-(--titlebar-h) bottom-0 z-[100] flex flex-col bg-black/92 text-white"
      onClick={(e) => e.target === e.currentTarget && onClose()}
    >
      <div className="flex h-14 shrink-0 items-center justify-between px-4 sm:px-6">
        <div className="min-w-0 truncate text-[14.5px]">
          <span className="font-semibold">{name}</span>
          <span className="ml-3 font-mono text-[11px] text-white/60">
            {i + 1} / {n}
          </span>
        </div>
        <button
          ref={closeBtn}
          type="button"
          onClick={onClose}
          aria-label="Close screenshots"
          className="flex h-11 w-11 items-center justify-center rounded-full hover:bg-white/10"
        >
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
            <path d="M6 6l12 12M18 6L6 18" />
          </svg>
        </button>
      </div>

      <div className="relative flex min-h-0 flex-1 items-center justify-center px-4 sm:px-20" onClick={(e) => e.target === e.currentTarget && onClose()}>
        {/* Show the thumbnail instantly, swap to full size when it lands. */}
        <FullImage key={shot.full} shot={shot} />
        {n > 1 && (
          <>
            <button
              type="button"
              onClick={() => go(-1)}
              aria-label="Previous screenshot"
              className="absolute left-2 top-1/2 flex h-12 w-12 -translate-y-1/2 items-center justify-center rounded-full bg-white/10 hover:bg-white/20 sm:left-5"
            >
              <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" aria-hidden="true">
                <path d="M15 18l-6-6 6-6" />
              </svg>
            </button>
            <button
              type="button"
              onClick={() => go(1)}
              aria-label="Next screenshot"
              className="absolute right-2 top-1/2 flex h-12 w-12 -translate-y-1/2 items-center justify-center rounded-full bg-white/10 hover:bg-white/20 sm:right-5"
            >
              <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" aria-hidden="true">
                <path d="M9 18l6-6-6-6" />
              </svg>
            </button>
          </>
        )}
      </div>

      {n > 1 && (
        <div className="flex shrink-0 justify-center gap-2 overflow-x-auto px-4 py-4">
          {shots.map((s, j) => (
            <button
              key={s.thumb}
              type="button"
              onClick={() => setI(j)}
              aria-label={`Screenshot ${j + 1}`}
              aria-current={j === i}
              className={cx(
                'h-14 w-24 shrink-0 overflow-hidden rounded-md opacity-55 transition hover:opacity-90',
                j === i && 'opacity-100 ring-2 ring-white',
              )}
            >
              <img src={s.thumb} alt="" loading="lazy" className="h-full w-full object-cover" />
            </button>
          ))}
        </div>
      )}
    </div>,
    document.body,
  )
}

/** The thumbnail at once, swapped for the full-size image when it has loaded. */
export function FullImage({ shot, className = 'max-h-full max-w-full rounded-lg object-contain shadow-2xl' }: { shot: Screenshot; className?: string }) {
  const [src, setSrc] = useState(shot.thumb)
  return (
    <img
      src={src}
      alt=""
      onLoad={() => {
        if (src === shot.thumb) {
          const full = new Image()
          full.onload = () => setSrc(shot.full)
          full.src = shot.full
        }
      }}
      className={className}
    />
  )
}
