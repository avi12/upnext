import { useEffect, useRef, useState } from 'react'
import { cx } from '../lib/format'
import { thumbFor, titleBarHeight, type Thumb } from '../lib/frame'

/** How long the bar stays after the page last scrolled or the pointer left it. */
const SHOW_MS = 1200
/** How close to the window's right edge the pointer brings the bar back. */
const NEAR_EDGE = 16

/** Where the bar starts: below the slim title bar, or below the window's
 *  buttons at the end of the top bar, so it never covers them. */
function ceiling(): number {
  const controls = document.querySelector('[data-testid="window-controls"]')
  return Math.max(titleBarHeight(), Math.round(controls?.getBoundingClientRect().bottom ?? 0))
}

/** The page's scrollbar in the Windows app window. WebView2
 *  keeps a strip down the window's right side for the page's own scrollbar,
 *  top bar included, so the close button couldn't sit in the window's
 *  corner. There the page's scrollbar is hidden (index.css) and this one
 *  floats over the page instead, as Windows 11's own apps' do: it shows
 *  while the page scrolls or the pointer is near the right edge. The page
 *  still scrolls itself, so the wheel, the keys and the UI's own scrolling
 *  work as they do in a browser. */
export function PageScrollbar() {
  const [thumb, setThumb] = useState<Thumb | null>(null)
  const [shown, setShown] = useState(false)
  const [held, setHeld] = useState(false)
  const [top, setTop] = useState(0)
  const hide = useRef<number | undefined>(undefined)
  const drag = useRef<{ y: number; scroll: number; perPixel: number } | null>(null)

  useEffect(() => {
    const measure = () => {
      const doc = document.documentElement
      const from = ceiling()
      setTop(from)
      setThumb(thumbFor(window.scrollY, doc.scrollHeight, window.innerHeight, from))
    }
    const show = () => {
      measure()
      setShown(true)
      window.clearTimeout(hide.current)
      hide.current = window.setTimeout(() => setShown(false), SHOW_MS)
    }
    const near = (e: MouseEvent) => {
      if (window.innerWidth - e.clientX <= NEAR_EDGE) show()
    }
    window.addEventListener('scroll', show, { passive: true })
    window.addEventListener('resize', measure)
    window.addEventListener('mousemove', near, { passive: true })
    return () => {
      window.clearTimeout(hide.current)
      window.removeEventListener('scroll', show)
      window.removeEventListener('resize', measure)
      window.removeEventListener('mousemove', near)
    }
  }, [])

  if (!thumb) return null
  const visible = shown || held
  return (
    <div
      aria-hidden="true"
      data-testid="page-scrollbar"
      className={cx('group fixed right-0 bottom-0 z-[999] w-3 transition-opacity duration-200', visible ? 'opacity-100' : 'pointer-events-none opacity-0')}
      style={{ top }}
      // A click on the track pages up or down, as a scrollbar's does.
      onMouseDown={(e) => {
        if (e.button !== 0 || e.target !== e.currentTarget) return
        e.preventDefault()
        window.scrollBy({ top: (e.clientY < thumb.top ? -1 : 1) * window.innerHeight * 0.9 })
      }}
    >
      <div
        className={cx(
          'absolute right-0.5 rounded-full bg-key transition-[width] group-hover:w-2 group-hover:bg-dim',
          held ? 'w-2 bg-dim' : 'w-1',
        )}
        style={{ top: thumb.top - top, height: thumb.height }}
        onPointerDown={(e) => {
          if (e.button !== 0) return
          e.preventDefault()
          e.currentTarget.setPointerCapture(e.pointerId)
          const doc = document.documentElement
          const track = window.innerHeight - top - thumb.height
          drag.current = { y: e.clientY, scroll: window.scrollY, perPixel: track > 0 ? (doc.scrollHeight - window.innerHeight) / track : 0 }
          setHeld(true)
        }}
        onPointerMove={(e) => {
          const from = drag.current
          if (from) window.scrollTo({ top: from.scroll + (e.clientY - from.y) * from.perPixel })
        }}
        onPointerUp={() => {
          drag.current = null
          setHeld(false)
        }}
        onPointerCancel={() => {
          drag.current = null
          setHeld(false)
        }}
      />
    </div>
  )
}
