import { useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore, type CSSProperties, type ReactNode } from 'react'
import { createPortal } from 'react-dom'
import { cx } from '../lib/format'
import { frameOs, resizeEdges, windowAction, type Edge, type FrameOs, type WindowState } from '../lib/frame'
import { trackDrag, windowDrag } from '../lib/windowDrag'
import { PageScrollbar } from './PageScrollbar'

// What the window last said about itself, and how many top bars carry the
// window's buttons right now. Shared by the frame and the bars.
let windowState: WindowState = { maximized: false, focused: true }
let hosts = 0
const listeners = new Set<() => void>()
let snapshot = { state: windowState, hosts }

function changed() {
  snapshot = { state: windowState, hosts }
  for (const l of listeners) l()
}

function subscribe(l: () => void) {
  listeners.add(l)
  return () => listeners.delete(l)
}

function useFrame() {
  return useSyncExternalStore(subscribe, () => snapshot)
}

/** The desktop app's window frame (merged into the top
 *  bar). The app's own top bar carries minimize, maximize or restore,
 *  and close at its right end (`WindowControls`), and drags the window
 *  (`useWindowDrag` in lib/windowDrag.ts); this keeps the rest: the
 *  window's state, the resize edges and, on a page with no top bar (setup,
 *  pairing), a slim bar of its own with the buttons. Only in the app's frameless window; a browser shows
 *  nothing. While the window is full screen the buttons step aside. */
export function WindowFrame() {
  const os = frameOs()
  return os ? <Frame os={os} /> : null
}

function Frame({ os }: { os: FrameOs }) {
  const { state, hosts } = useFrame()
  const fullscreen = !!state.fullscreen
  // A page without a top bar gets the slim bar, and the room for it (index.css).
  const bar = hosts === 0 && !fullscreen
  useLayoutEffect(() => {
    const root = document.documentElement
    if (bar) root.dataset.titlebar = ''
    else delete root.dataset.titlebar
    return () => {
      delete root.dataset.titlebar
    }
  }, [bar])

  useEffect(() => {
    const told = (e: Event) => {
      windowState = (e as CustomEvent<WindowState>).detail
      changed()
    }
    window.addEventListener('upnext:window', told)
    // The window waits for this before it takes the system's title bar away
    // for good, and answers with its state.
    windowAction('framed')
    return () => {
      window.removeEventListener('upnext:window', told)
      windowState = { maximized: false, focused: true }
      changed()
    }
  }, [])

  // The window moves once the pointer has moved a little with the button
  // down on a bar.
  useEffect(trackDrag, [])

  if (fullscreen) return os === 'windows' ? <PageScrollbar /> : null
  return (
    <>
      {bar && (
        <div data-testid="title-bar" className="fixed inset-x-0 top-0 z-[1000] flex h-(--titlebar-h) items-stretch justify-end bg-ground select-none" {...windowDrag}>
          <Buttons state={state} />
        </div>
      )}
      {/* The edges the window is resized from, except while it's maximized. */}
      {!state.maximized && resizeEdges(os).map((edge) => <ResizeEdge key={edge} edge={edge} />)}
      {os === 'windows' && <PageScrollbar />}
      {/* A hairline round the window on Linux, where it has no frame or
          shadow to stand out from the windows behind it. */}
      {os === 'linux' && !state.maximized && (
        <div aria-hidden="true" className="pointer-events-none fixed inset-0 z-[1001] border border-line-strong" />
      )}
    </>
  )
}

/** Minimize, maximize or restore, and close, for the right end of a top bar.
 *  Nothing in a browser, or while the window is full screen. */
export function WindowControls({ className }: { className?: string }) {
  return frameOs() ? <Hosted className={className} /> : null
}

/** Where the window's buttons go: over the room kept for them in the bar. */
interface Place {
  top: number
  left: number
  width: number
  height: number
}

function Hosted({ className }: { className?: string }) {
  const { state } = useFrame()
  const room = useRef<HTMLDivElement>(null)
  const [place, setPlace] = useState<Place | null>(null)
  // Before the browser paints, so the slim bar never shows under a top bar.
  useLayoutEffect(() => {
    hosts++
    changed()
    return () => {
      hosts--
      changed()
    }
  }, [])

  // The buttons float in a layer of their own over the room the bar keeps
  // for them, above every panel, sheet, dialog and the tour, as the title
  // bar did, so the window can always be moved off, minimized or closed.
  // The room moves only when the window resizes or, in big screen's
  // overscan, while the bar comes to stick.
  const fullscreen = !!state.fullscreen
  useLayoutEffect(() => {
    const el = room.current
    if (!el) return
    const measure = () => {
      const r = el.getBoundingClientRect()
      setPlace((p) =>
        p && p.top === r.top && p.left === r.left && p.width === r.width && p.height === r.height
          ? p
          : { top: r.top, left: r.left, width: r.width, height: r.height },
      )
    }
    measure()
    const resized = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(measure)
    if (el.parentElement) resized?.observe(el.parentElement)
    window.addEventListener('resize', measure)
    window.addEventListener('scroll', measure, { passive: true })
    return () => {
      resized?.disconnect()
      window.removeEventListener('resize', measure)
      window.removeEventListener('scroll', measure)
    }
  }, [fullscreen])

  if (fullscreen) return null
  return (
    <>
      {/* Room for three 46 px buttons. */}
      <div ref={room} aria-hidden="true" data-testid="window-controls-room" className={cx('w-[138px] shrink-0', className)} />
      {place && createPortal(<Buttons state={state} className="fixed z-[1000]" style={place} />, document.body)}
    </>
  )
}

function Buttons({ state, className, style }: { state: WindowState; className?: string; style?: CSSProperties }) {
  return (
    <div
      data-testid="window-controls"
      style={style}
      // Big screen's focus engine leaves the window's buttons out, so a pad
      // moving about never lands on Close; a pointer still reaches them.
      data-focus-skip=""
      className={cx('flex shrink-0 items-stretch', state.focused ? 'text-ink' : 'text-faint', className)}
    >
      <WindowButton label="Minimize" onClick={() => windowAction('minimize')}>
        <path d="M0.5 5.5h10" />
      </WindowButton>
      <WindowButton label={state.maximized ? 'Restore' : 'Maximize'} onClick={() => windowAction('maximize')}>
        {state.maximized ? (
          <>
            <rect x="0.5" y="2.5" width="8" height="8" rx="1" />
            <path d="M2.5 2.5v-1a1 1 0 0 1 1-1h6a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1h-1" />
          </>
        ) : (
          <rect x="0.5" y="0.5" width="10" height="10" rx="1" />
        )}
      </WindowButton>
      <WindowButton label="Close" close onClick={() => windowAction('close')}>
        <path d="M0.5 0.5l10 10M10.5 0.5l-10 10" />
      </WindowButton>
    </div>
  )
}

function WindowButton({ label, close = false, onClick, children }: { label: string; close?: boolean; onClick: () => void; children: ReactNode }) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
      className={cx(
        'flex w-[46px] items-center justify-center transition-colors focus-visible:outline-offset-[-2px]',
        // Close turns red like every other window's; the others take the UI's raised colour.
        close ? 'hover:bg-[#c42b1c] hover:text-white active:bg-[#b22a1c] active:text-white' : 'hover:bg-raised active:bg-raised-2',
      )}
    >
      <svg width="11" height="11" viewBox="0 0 11 11" fill="none" stroke="currentColor" strokeWidth="1" aria-hidden="true" shapeRendering="geometricPrecision">
        {children}
      </svg>
    </button>
  )
}

/** Where each edge sits, and the pointer it shows. The sides are a few
 *  pixels thick and the corners a little bigger, so they're easy to find. */
const EDGES: Record<Edge, string> = {
  n: 'top-0 inset-x-2.5 h-1 cursor-ns-resize',
  s: 'bottom-0 inset-x-2.5 h-1 cursor-ns-resize',
  e: 'right-0 inset-y-2.5 w-1 cursor-ew-resize',
  w: 'left-0 inset-y-2.5 w-1 cursor-ew-resize',
  ne: 'top-0 right-0 h-2.5 w-2.5 cursor-nesw-resize',
  sw: 'bottom-0 left-0 h-2.5 w-2.5 cursor-nesw-resize',
  nw: 'top-0 left-0 h-2.5 w-2.5 cursor-nwse-resize',
  se: 'bottom-0 right-0 h-2.5 w-2.5 cursor-nwse-resize',
}

function ResizeEdge({ edge }: { edge: Edge }) {
  return (
    <div
      aria-hidden="true"
      data-edge={edge}
      className={cx('fixed z-[1002]', EDGES[edge])}
      onMouseDown={(e) => {
        if (e.button !== 0) return
        e.preventDefault()
        windowAction(`resize:${edge}`)
      }}
    />
  )
}
